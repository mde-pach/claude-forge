//! The language-server hub (§17.8): one server process per language and repository, shared by
//! every client that speaks LSP to it (Serena in each session, later Claude Code's LSP tool).
//!
//! Clients connect to the hub's socket through `claude-forge lsp <language>`, a stdio shim. The
//! hub:
//! - starts the server on the first client and sends it the first client's `initialize`; later
//!   clients get the same answer, and their `initialized` is not passed on;
//! - gives every request its own id towards the server and the client's back on the answer;
//! - answers the server's requests itself (configuration: none; workspace folders: the root);
//! - keeps one copy of each open document: a client opening a document already open sends its
//!   text (a change when it differs), each change is applied and sent whole, and a document
//!   closes when its last client closes it;
//! - passes the server's notifications (diagnostics, progress, logs) to every client;
//! - starts the server again when it exits while clients remain (the handshake and the open
//!   documents replayed; the requests in flight answered with an error), and stops it, then
//!   itself, after `idle` without a client.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Reads one `Content-Length` framed message; None at the end of the stream.
pub fn read_frame(r: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().ok();
            }
        }
    }
    let mut body = vec![0; len.unwrap_or(0)];
    r.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

pub fn write_frame(w: &mut impl Write, msg: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

/// What the hub runs.
pub struct HubConfig {
    pub root: PathBuf,
    pub socket: PathBuf,
    /// The server's command line.
    pub command: Vec<String>,
    /// Merged into the first client's `initializationOptions` (TypeScript's: which tsserver).
    pub init_options: Option<Value>,
    /// Stops after this long without a client.
    pub idle: Duration,
    pub log: Option<PathBuf>,
    /// The starters' marker (`<state>/lsp-<language>.starting`), removed once the hub answers: a
    /// marker left by a hub that died would make the next client wait for nothing.
    pub starting: Option<PathBuf>,
}

enum Event {
    Client(u64, Arc<Mutex<UnixStream>>),
    FromClient(u64, Value),
    ClientGone(u64),
    FromServer(u64, Value),
    ServerGone(u64),
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    generation: u64,
}

struct Doc {
    text: String,
    version: i64,
    language: String,
    clients: BTreeSet<u64>,
}

struct ClientState {
    out: Arc<Mutex<UnixStream>>,
    ready: bool,
}

/// A request in flight towards the server.
enum Waiting {
    Initialize,
    Client(u64, Value),
}

struct Hub {
    config: HubConfig,
    tx: Sender<Event>,
    clients: BTreeMap<u64, ClientState>,
    server: Option<Server>,
    generation: u64,
    init_params: Option<Value>,
    init_result: Option<Value>,
    /// Clients waiting for the `initialize` answer: (client, its id).
    init_waiting: Vec<(u64, Value)>,
    /// Messages of clients that arrived before the server was ready.
    backlog: VecDeque<(u64, Value)>,
    waiting: HashMap<i64, Waiting>,
    next_id: i64,
    docs: BTreeMap<String, Doc>,
    /// The server answered its `initialize` (the clients' messages wait in the backlog until then).
    server_ready: bool,
    utf8: bool,
    idle_since: Option<Instant>,
    log: Option<std::fs::File>,
    /// Every message's method in the log (`FORGE_LSP_TRACE`).
    trace: bool,
    started: Instant,
}

/// Runs the hub until it has been idle `config.idle`. Fails when another hub answers on the socket.
pub fn serve(config: HubConfig) -> Result<(), String> {
    if UnixStream::connect(&config.socket).is_ok() {
        return Err("a hub already runs on this socket".into());
    }
    let _ = std::fs::remove_file(&config.socket);
    if let Some(dir) = config.socket.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let listener =
        UnixListener::bind(&config.socket).map_err(|e| format!("cannot bind the socket: {e}"))?;
    if let Some(m) = &config.starting {
        let _ = std::fs::remove_dir(m);
    }
    let (tx, rx) = channel();
    let accept_tx = tx.clone();
    thread::spawn(move || {
        let mut next = 0u64;
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            next += 1;
            let id = next;
            let Ok(reader) = conn.try_clone() else {
                continue;
            };
            if accept_tx
                .send(Event::Client(id, Arc::new(Mutex::new(conn))))
                .is_err()
            {
                return;
            }
            let tx = accept_tx.clone();
            thread::spawn(move || {
                let mut r = BufReader::new(reader);
                while let Ok(Some(msg)) = read_frame(&mut r) {
                    if tx.send(Event::FromClient(id, msg)).is_err() {
                        return;
                    }
                }
                let _ = tx.send(Event::ClientGone(id));
            });
        }
    });
    let log = config.log.as_ref().and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
    });
    let socket = config.socket.clone();
    let mut hub = Hub {
        config,
        tx,
        clients: BTreeMap::new(),
        server: None,
        generation: 0,
        init_params: None,
        init_result: None,
        init_waiting: Vec::new(),
        backlog: VecDeque::new(),
        waiting: HashMap::new(),
        next_id: 0,
        docs: BTreeMap::new(),
        server_ready: false,
        utf8: false,
        idle_since: Some(Instant::now()),
        log,
        trace: std::env::var_os("FORGE_LSP_TRACE").is_some_and(|v| !v.is_empty()),
        started: Instant::now(),
    };
    hub.run(rx);
    let _ = std::fs::remove_file(socket);
    Ok(())
}

impl Hub {
    fn log(&mut self, msg: &str) {
        let t = self.started.elapsed().as_secs_f64();
        if let Some(f) = self.log.as_mut() {
            let _ = writeln!(f, "{t:>9.3} {msg}");
        }
    }

    fn run(&mut self, rx: Receiver<Event>) {
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(Event::Client(id, out)) => {
                    self.log(&format!("client {id} connected"));
                    self.clients.insert(id, ClientState { out, ready: false });
                    self.idle_since = None;
                }
                Ok(Event::FromClient(id, msg)) => self.on_client(id, msg),
                Ok(Event::ClientGone(id)) => self.client_gone(id),
                Ok(Event::FromServer(g, msg)) if g == self.generation => self.on_server(msg),
                Ok(Event::ServerGone(g)) if g == self.generation => self.server_gone(),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if let Some(since) = self.idle_since {
                if since.elapsed() >= self.config.idle {
                    self.log("idle: stopping");
                    self.stop_server();
                    return;
                }
            }
        }
    }

    fn send_client(&self, id: u64, msg: &Value) {
        if let Some(c) = self.clients.get(&id) {
            let mut out = c.out.lock().unwrap_or_else(|e| e.into_inner());
            let _ = write_frame(&mut *out, msg);
        }
    }

    fn send_server(&mut self, msg: &Value) {
        if let Some(s) = self.server.as_mut() {
            let _ = write_frame(&mut s.stdin, msg);
        }
    }

    fn start_server(&mut self) -> bool {
        let Some((prog, args)) = self.config.command.split_first() else {
            return false;
        };
        let child = Command::new(prog)
            .args(args)
            .current_dir(&self.config.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("the server could not start: {e}");
                self.log(&msg);
                return false;
            }
        };
        self.generation += 1;
        let generation = self.generation;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let tx = self.tx.clone();
        thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            while let Ok(Some(msg)) = read_frame(&mut r) {
                if tx.send(Event::FromServer(generation, msg)).is_err() {
                    return;
                }
            }
            let _ = tx.send(Event::ServerGone(generation));
        });
        self.server = Some(Server {
            child,
            stdin,
            generation,
        });
        self.server_ready = false;
        self.log(&format!("server started ({generation})"));
        true
    }

    fn stop_server(&mut self) {
        if let Some(mut s) = self.server.take() {
            let _ = write_frame(
                &mut s.stdin,
                &json!({"jsonrpc": "2.0", "id": -1, "method": "shutdown"}),
            );
            let _ = write_frame(&mut s.stdin, &json!({"jsonrpc": "2.0", "method": "exit"}));
            drop(s.stdin);
            let until = Instant::now() + Duration::from_secs(3);
            while Instant::now() < until {
                if matches!(s.child.try_wait(), Ok(Some(_))) {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
            let _ = s.child.kill();
            let _ = s.child.wait();
            let _ = s.generation;
        }
    }

    /// Sends `initialize` (the first client's) to a newly started server.
    fn handshake(&mut self) {
        let Some(mut params) = self.init_params.clone() else {
            return;
        };
        // the server watches this process, not the first client's: servers that exit when it does
        // (TypeScript's) would stop with the first session
        if let Some(p) = params.as_object_mut() {
            p.insert("processId".into(), json!(std::process::id()));
        }
        if let (Some(extra), Some(p)) = (&self.config.init_options, params.as_object_mut()) {
            let opts = p
                .entry("initializationOptions")
                .or_insert_with(|| json!({}));
            merge(opts, extra);
        }
        self.next_id += 1;
        let id = self.next_id;
        self.waiting.insert(id, Waiting::Initialize);
        self.send_server(
            &json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": params}),
        );
    }

    fn on_client(&mut self, client: u64, msg: Value) {
        let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
        let id = msg.get("id").cloned();
        if self.trace {
            self.log(&format!(
                "< {client} {} {}",
                method.as_deref().unwrap_or("(answer)"),
                id.as_ref().map(Value::to_string).unwrap_or_default()
            ));
        }
        match method.as_deref() {
            Some("initialize") => {
                let id = id.unwrap_or(Value::Null);
                if self.init_params.is_none() {
                    self.init_params = msg.get("params").cloned();
                }
                // no server (the first client, or it stopped since): start it with the handshake
                if self.server.is_none() && self.start_server() {
                    self.handshake();
                }
                if let Some(result) = &self.init_result {
                    self.send_client(
                        client,
                        &json!({"jsonrpc": "2.0", "id": id, "result": result}),
                    );
                    if let Some(c) = self.clients.get_mut(&client) {
                        c.ready = true;
                    }
                    return;
                }
                self.init_waiting.push((client, id));
                return;
            }
            Some("initialized") | Some("exit") => return,
            Some("shutdown") => {
                self.send_client(client, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
                return;
            }
            _ => {}
        }
        if !self.server_ready {
            self.backlog.push_back((client, msg));
            return;
        }
        match method.as_deref() {
            Some("textDocument/didOpen") => self.did_open(client, &msg),
            Some("textDocument/didChange") => self.did_change(&msg),
            Some("textDocument/didClose") => self.did_close(client, &msg),
            Some("$/cancelRequest") => {
                let want = msg["params"]["id"].clone();
                let hub_id = self.waiting.iter().find_map(|(h, w)| match w {
                    Waiting::Client(c, i) if *c == client && *i == want => Some(*h),
                    _ => None,
                });
                if let Some(h) = hub_id {
                    self.send_server(&json!({"jsonrpc": "2.0", "method": "$/cancelRequest", "params": {"id": h}}));
                }
            }
            Some(_) if id.is_some() => {
                self.next_id += 1;
                let hub_id = self.next_id;
                self.waiting
                    .insert(hub_id, Waiting::Client(client, id.unwrap_or(Value::Null)));
                let mut m = msg;
                m["id"] = json!(hub_id);
                self.send_server(&m);
            }
            Some(_) => self.send_server(&msg),
            None => {
                // a client's answer to a request of the server's: the hub answers those itself
            }
        }
    }

    fn did_open(&mut self, client: u64, msg: &Value) {
        let td = &msg["params"]["textDocument"];
        let Some(uri) = td["uri"].as_str().map(str::to_owned) else {
            return;
        };
        let text = td["text"].as_str().unwrap_or("").to_owned();
        let language = td["languageId"].as_str().unwrap_or("").to_owned();
        match self.docs.get_mut(&uri) {
            Some(doc) => {
                doc.clients.insert(client);
                if doc.text != text {
                    doc.text = text;
                    doc.version += 1;
                    let change = json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
                        "textDocument": {"uri": uri, "version": doc.version},
                        "contentChanges": [{"text": doc.text}]}});
                    self.send_server(&change);
                }
            }
            None => {
                let doc = Doc {
                    text,
                    version: 1,
                    language,
                    clients: BTreeSet::from([client]),
                };
                let open = json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
                    "textDocument": {"uri": uri, "languageId": doc.language, "version": doc.version, "text": doc.text}}});
                self.docs.insert(uri, doc);
                self.send_server(&open);
            }
        }
    }

    fn did_change(&mut self, msg: &Value) {
        let Some(uri) = msg["params"]["textDocument"]["uri"].as_str() else {
            return;
        };
        let utf8 = self.utf8;
        let Some(doc) = self.docs.get_mut(uri) else {
            return;
        };
        for change in msg["params"]["contentChanges"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(text) = change["text"].as_str() else {
                continue;
            };
            match change.get("range") {
                Some(range) => {
                    let start = offset(&doc.text, &range["start"], utf8);
                    let end = offset(&doc.text, &range["end"], utf8);
                    if start <= end {
                        doc.text.replace_range(start..end, text);
                    }
                }
                None => doc.text = text.to_owned(),
            }
        }
        doc.version += 1;
        let change = json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
            "textDocument": {"uri": uri, "version": doc.version},
            "contentChanges": [{"text": doc.text}]}});
        self.send_server(&change);
    }

    fn did_close(&mut self, client: u64, msg: &Value) {
        let Some(uri) = msg["params"]["textDocument"]["uri"]
            .as_str()
            .map(str::to_owned)
        else {
            return;
        };
        self.release(client, &uri);
    }

    /// `client` no longer has `uri` open: the server's copy closes with its last client.
    fn release(&mut self, client: u64, uri: &str) {
        let Some(doc) = self.docs.get_mut(uri) else {
            return;
        };
        doc.clients.remove(&client);
        if doc.clients.is_empty() {
            self.docs.remove(uri);
            self.send_server(&json!({"jsonrpc": "2.0", "method": "textDocument/didClose", "params": {"textDocument": {"uri": uri}}}));
        }
    }

    fn client_gone(&mut self, client: u64) {
        self.log(&format!("client {client} left"));
        let uris: Vec<String> = self
            .docs
            .iter()
            .filter(|(_, d)| d.clients.contains(&client))
            .map(|(u, _)| u.clone())
            .collect();
        for u in uris {
            self.release(client, &u);
        }
        self.waiting
            .retain(|_, w| !matches!(w, Waiting::Client(c, _) if *c == client));
        self.backlog.retain(|(c, _)| *c != client);
        self.init_waiting.retain(|(c, _)| *c != client);
        self.clients.remove(&client);
        if self.clients.is_empty() {
            self.idle_since = Some(Instant::now());
        }
    }

    fn on_server(&mut self, msg: Value) {
        let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
        let id = msg.get("id").cloned();
        if self.trace {
            self.log(&format!(
                "> {} {}",
                method.as_deref().unwrap_or("(answer)"),
                id.as_ref().map(Value::to_string).unwrap_or_default()
            ));
        }
        match (method, id) {
            // an answer
            (None, Some(id)) => {
                let Some(hub_id) = id.as_i64() else { return };
                match self.waiting.remove(&hub_id) {
                    Some(Waiting::Initialize) => self.initialized(msg),
                    Some(Waiting::Client(client, their)) => {
                        let mut m = msg;
                        m["id"] = their;
                        self.send_client(client, &m);
                    }
                    None => {}
                }
            }
            // a request of the server's
            (Some(method), Some(id)) => {
                let result = match method.as_str() {
                    "workspace/configuration" => {
                        let n = msg["params"]["items"].as_array().map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; n])
                    }
                    "workspace/workspaceFolders" => json!([{
                        "uri": file_uri(&self.config.root),
                        "name": self.config.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    }]),
                    _ => Value::Null,
                };
                self.send_server(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
            }
            // a notification: every client
            (Some(_), None) => {
                let ready: Vec<u64> = self
                    .clients
                    .iter()
                    .filter(|(_, c)| c.ready)
                    .map(|(id, _)| *id)
                    .collect();
                for c in ready {
                    self.send_client(c, &msg);
                }
            }
            (None, None) => {}
        }
    }

    fn initialized(&mut self, msg: Value) {
        let result = msg.get("result").cloned().unwrap_or(Value::Null);
        self.utf8 = result["capabilities"]["positionEncoding"] == "utf-8";
        self.send_server(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
        // documents open before a restart
        let opens: Vec<Value> = self
            .docs
            .iter()
            .map(|(uri, d)| json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
                "textDocument": {"uri": uri, "languageId": d.language, "version": d.version, "text": d.text}}}))
            .collect();
        for o in opens {
            self.send_server(&o);
        }
        let first = self.init_result.is_none();
        self.init_result = Some(result.clone());
        self.server_ready = true;
        let waiting: Vec<(u64, Value)> = self.init_waiting.drain(..).collect();
        for (client, id) in waiting {
            if first || msg.get("error").is_some() {
                let mut m = msg.clone();
                m["id"] = id;
                self.send_client(client, &m);
            } else {
                self.send_client(
                    client,
                    &json!({"jsonrpc": "2.0", "id": id, "result": result}),
                );
            }
            if let Some(c) = self.clients.get_mut(&client) {
                c.ready = true;
            }
        }
        let backlog: Vec<(u64, Value)> = self.backlog.drain(..).collect();
        for (client, m) in backlog {
            self.on_client(client, m);
        }
    }

    fn server_gone(&mut self) {
        if let Some(mut s) = self.server.take() {
            let _ = s.child.wait();
        }
        self.server_ready = false;
        self.log("server exited");
        // the requests in flight will not be answered
        let lost: Vec<(u64, Value)> = self
            .waiting
            .drain()
            .filter_map(|(_, w)| match w {
                Waiting::Client(c, id) => Some((c, id)),
                Waiting::Initialize => None,
            })
            .collect();
        for (client, id) in lost {
            self.send_client(client, &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "The language server restarted; make the request again."}}));
        }
        if self.clients.is_empty() || self.init_params.is_none() {
            return;
        }
        if self.start_server() {
            self.handshake();
        }
    }
}

/// `extra` merged into `into`: objects key by key, anything else replaced.
fn merge(into: &mut Value, extra: &Value) {
    match (into, extra) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (slot, v) => *slot = v.clone(),
    }
}

/// The byte offset of an LSP position (`line`, `character` in UTF-16 code units, or bytes when
/// the server chose UTF-8), clamped to the text.
fn offset(text: &str, pos: &Value, utf8: bool) -> usize {
    let line = pos["line"].as_u64().unwrap_or(0) as usize;
    let character = pos["character"].as_u64().unwrap_or(0) as usize;
    let mut start = 0;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line_text = &text[start..end];
    if utf8 {
        let mut c = character.min(line_text.len());
        while !line_text.is_char_boundary(c) {
            c -= 1;
        }
        return start + c;
    }
    let mut units = 0;
    for (i, ch) in line_text.char_indices() {
        if units >= character {
            return start + i;
        }
        units += ch.len_utf16();
    }
    end
}

fn file_uri(p: &Path) -> String {
    format!("file://{}", p.display())
}

/// The shim: connects to the hub at `socket`, starting it with `start` when none answers, and
/// passes stdin and stdout through until either side ends.
pub fn shim(socket: &Path, start: impl FnOnce() -> std::io::Result<()>) -> Result<(), String> {
    let stream = match UnixStream::connect(socket) {
        Ok(s) => s,
        Err(_) => {
            start().map_err(|e| format!("the hub could not start: {e}"))?;
            let until = Instant::now() + Duration::from_secs(15);
            loop {
                match UnixStream::connect(socket) {
                    Ok(s) => break s,
                    Err(e) if Instant::now() >= until => {
                        return Err(format!("the hub does not answer: {e}"));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(50)),
                }
            }
        }
    };
    let mut up = stream.try_clone().map_err(|e| e.to_string())?;
    let writer = thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut up);
        let _ = up.shutdown(std::net::Shutdown::Write);
    });
    // stdout is line-buffered and a message body ends without a newline: flush every chunk, or
    // the last answer waits for the next message
    let mut down = stream;
    let mut out = std::io::stdout().lock();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match std::io::Read::read(&mut down, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if out.write_all(&buf[..n]).and_then(|()| out.flush()).is_err() {
                    break;
                }
            }
        }
    }
    drop(writer);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_in_utf16_and_utf8() {
        let t = "ab\nx€y\n";
        assert_eq!(offset(t, &json!({"line": 1, "character": 2}), false), 3 + 4);
        assert_eq!(offset(t, &json!({"line": 1, "character": 4}), true), 3 + 4);
        assert_eq!(
            offset(t, &json!({"line": 9, "character": 0}), false),
            t.len()
        );
        assert_eq!(offset(t, &json!({"line": 0, "character": 99}), false), 2);
    }

    #[test]
    fn initialization_options_are_merged() {
        let mut a = json!({"tsserver": {"path": "/serena/ts", "logVerbosity": "off"}, "x": 1});
        merge(
            &mut a,
            &json!({"tsserver": {"path": "/app/node_modules/typescript/lib/tsserver.js"}}),
        );
        assert_eq!(
            a,
            json!({"tsserver": {"path": "/app/node_modules/typescript/lib/tsserver.js", "logVerbosity": "off"}, "x": 1})
        );
    }

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &json!({"a": 1})).unwrap();
        write_frame(&mut buf, &json!({"b": "é"})).unwrap();
        let mut r = std::io::Cursor::new(buf);
        assert_eq!(read_frame(&mut r).unwrap(), Some(json!({"a": 1})));
        assert_eq!(read_frame(&mut r).unwrap(), Some(json!({"b": "é"})));
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }
}
