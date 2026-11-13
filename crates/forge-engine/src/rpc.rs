//! A language server process and its JSON messages: LSP (JSON-RPC with `Content-Length` frames both
//! ways) or tsserver (newline-delimited requests, `Content-Length` framed responses).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Framing {
    Lsp,
    TsServer,
}

/// Answers a request the server sends (LSP `workspace/configuration`, registrations).
pub(crate) type Handler = Box<dyn Fn(&str, &Value) -> Value + Send + Sync>;

type Waiting = Arc<Mutex<HashMap<i64, Sender<Value>>>>;

pub(crate) struct Rpc {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Mutex<Child>,
    waiting: Waiting,
    next: AtomicI64,
    alive: Arc<AtomicBool>,
    framing: Framing,
    /// How long a request may take.
    pub timeout: Duration,
}

/// A request sent, whose answer is waited for with [`Pending::wait`]: requests can be pipelined.
pub(crate) struct Pending {
    rx: Receiver<Value>,
    timeout: Duration,
    what: String,
}

impl Pending {
    /// The result (`null` for an error response).
    pub fn wait(self) -> Result<Value, String> {
        self.rx.recv_timeout(self.timeout).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => format!("{} got no answer in time", self.what),
            mpsc::RecvTimeoutError::Disconnected => "the language server exited".to_owned(),
        })
    }
}

fn write_frame(w: &mut impl Write, framing: Framing, msg: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    match framing {
        Framing::Lsp => {
            write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
            w.write_all(&body)?;
        }
        Framing::TsServer => {
            w.write_all(&body)?;
            w.write_all(b"\n")?;
        }
    }
    w.flush()
}

impl Rpc {
    /// Starts `cmd` with piped stdio; `stderr` gets the server's stderr (a log file, or null).
    pub fn spawn(
        mut cmd: Command,
        framing: Framing,
        stderr: Stdio,
        handler: Option<Handler>,
    ) -> Result<Rpc, String> {
        let program = cmd.get_program().to_string_lossy().into_owned();
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .map_err(|e| format!("cannot run {program}: {e}"))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().expect("piped stdin")));
        let stdout = child.stdout.take().expect("piped stdout");
        let waiting: Waiting = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        {
            let waiting = waiting.clone();
            let alive = alive.clone();
            let stdin = stdin.clone();
            std::thread::spawn(move || {
                read_loop(stdout, framing, &waiting, &stdin, handler.as_ref());
                alive.store(false, Ordering::SeqCst);
                waiting.lock().expect("waiting").clear();
            });
        }
        Ok(Rpc {
            stdin,
            child: Mutex::new(child),
            waiting,
            next: AtomicI64::new(0),
            alive,
            framing,
            timeout: Duration::from_secs(60),
        })
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn write(&self, msg: &Value) -> Result<(), String> {
        let mut w = self.stdin.lock().expect("stdin");
        write_frame(&mut *w, self.framing, msg).map_err(|_| "the language server exited".to_owned())
    }

    /// An LSP request, or a tsserver command (`method` is the command, `params` its arguments).
    pub fn request(&self, method: &str, params: Value) -> Pending {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = mpsc::channel();
        self.waiting.lock().expect("waiting").insert(id, tx);
        let msg = match self.framing {
            Framing::Lsp => json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
            Framing::TsServer => {
                json!({"seq": id, "type": "request", "command": method, "arguments": params})
            }
        };
        if self.write(&msg).is_err() {
            self.waiting.lock().expect("waiting").remove(&id);
        }
        Pending {
            rx,
            timeout: self.timeout,
            what: method.to_owned(),
        }
    }

    /// An LSP notification, or a tsserver command whose answer is not waited for.
    pub fn notify(&self, method: &str, params: Value) {
        let msg = match self.framing {
            Framing::Lsp => json!({"jsonrpc": "2.0", "method": method, "params": params}),
            Framing::TsServer => {
                let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
                json!({"seq": id, "type": "request", "command": method, "arguments": params})
            }
        };
        let _ = self.write(&msg);
    }

    /// Ends the server: LSP `shutdown` + `exit` (answered within a second), then a kill.
    pub fn stop(&self) {
        if self.alive() && self.framing == Framing::Lsp {
            let p = self.request("shutdown", Value::Null);
            let _ = p.rx.recv_timeout(Duration::from_secs(1));
            self.notify("exit", Value::Null);
        }
        let mut child = self.child.lock().expect("child");
        for _ in 0..20 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Rpc {
    fn drop(&mut self) {
        let mut child = self.child.lock().expect("child");
        if !matches!(child.try_wait(), Ok(Some(_))) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn read_loop(
    stdout: impl Read,
    framing: Framing,
    waiting: &Waiting,
    stdin: &Mutex<ChildStdin>,
    handler: Option<&Handler>,
) {
    let mut r = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        let mut length: Option<usize> = None;
        loop {
            line.clear();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let t = line.trim();
            if t.is_empty() {
                if length.is_some() {
                    break;
                }
                continue;
            }
            if let Some((k, v)) = t.split_once(':') {
                if k.eq_ignore_ascii_case("content-length") {
                    length = v.trim().parse().ok();
                }
            }
        }
        let mut body = vec![0; length.unwrap_or(0)];
        if r.read_exact(&mut body).is_err() {
            return;
        }
        let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        match framing {
            Framing::Lsp => {
                let id = msg.get("id").and_then(Value::as_i64);
                let request = msg.get("id").is_some_and(|i| !i.is_null());
                if let (Some(method), true) = (msg.get("method").and_then(Value::as_str), request) {
                    let params = msg.get("params").cloned().unwrap_or(Value::Null);
                    let result = handler.map(|h| h(method, &params)).unwrap_or(Value::Null);
                    let reply = json!({"jsonrpc": "2.0", "id": msg["id"], "result": result});
                    let mut w = stdin.lock().expect("stdin");
                    let _ = write_frame(&mut *w, framing, &reply);
                } else if let Some(id) = id {
                    let tx = waiting.lock().expect("waiting").remove(&id);
                    if let Some(tx) = tx {
                        let _ = tx.send(msg.get("result").cloned().unwrap_or(Value::Null));
                    }
                }
            }
            Framing::TsServer => {
                if msg.get("type").and_then(Value::as_str) != Some("response") {
                    continue;
                }
                let Some(id) = msg.get("request_seq").and_then(Value::as_i64) else {
                    continue;
                };
                if let Some(tx) = waiting.lock().expect("waiting").remove(&id) {
                    let ok = msg.get("success").and_then(Value::as_bool) == Some(true);
                    let body = if ok {
                        msg.get("body").cloned().unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    };
                    let _ = tx.send(body);
                }
            }
        }
    }
}
