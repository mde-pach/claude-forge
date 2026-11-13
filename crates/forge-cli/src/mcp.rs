//! `claude-forge mcp serena` (§17.8): what the project's `.mcp.json` runs to start Serena.
//!
//! It installs the pinned Serena when it is missing, writes the machine-dependent settings
//! (`.serena/project.local.yml`: the paths of the servers Forge provides), and stays between
//! Claude Code and Serena as a line pass-through. When Serena exits during the session it is
//! started again and the MCP handshake replayed, so the session keeps its tools; the calls that
//! were in flight are answered with an error, to be made again. Each restart is a note.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use forge_core::serena::{self, Provider};
use forge_core::{Note, NoteKind};
use serde_json::{Value, json};

/// Restarts allowed within [`RESTART_WINDOW`] before the launcher gives up.
const RESTARTS_MAX: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(300);
/// The id of a replayed `initialize`, whose answer Claude Code must not see.
const REPLAY_ID: &str = "forge-serena-replay";

pub fn run(server: &str) -> ExitCode {
    if server != serena::SERVER {
        eprintln!(
            "forge: unknown MCP server {server:?} (known: {})",
            serena::SERVER
        );
        return ExitCode::from(2);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = match forge_home::find_repo_root(&cwd) {
        Ok(Some(r)) => r.path().to_path_buf(),
        _ => cwd.clone(),
    };
    let home = match forge_home::Home::from_env() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("forge: {e}");
            return ExitCode::from(2);
        }
    };
    let state = forge_home::RepoId::for_root(&root)
        .ok()
        .map(|id| home.repo_state_dir(&id));
    let note = |text: String| {
        eprintln!("forge: {text}");
        if let Some(s) = &state {
            forge_hooks::state::record_notes(s, &[Note::new(NoteKind::Environment, text)]);
        }
    };
    let cmd = match command(&home, &root, &note) {
        Ok(c) => c,
        Err(e) => {
            note(e);
            return ExitCode::from(1);
        }
    };
    proxy(cmd, &note)
}

/// Serena on the project, as the session runs it: the pinned install (installed when missing),
/// the local settings written, Serena's home in Forge's cache, piped stdin and stdout.
pub fn command(
    home: &forge_home::Home,
    root: &Path,
    note: &dyn Fn(String),
) -> Result<Command, String> {
    let bin =
        install(home, serena::TOOL).map_err(|e| format!("Serena could not be installed: {e}"))?;
    write_local(home, root, note);
    let serena_home = home.cache_root().join("serena").join("home");
    let _ = std::fs::create_dir_all(&serena_home);
    let mut cmd = Command::new(&bin);
    cmd.args(serena::server_args(&root.to_string_lossy()))
        .current_dir(root)
        .env("SERENA_HOME", &serena_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    Ok(cmd)
}

/// `forge apply`: starts Serena once and asks each language's server for the symbols of one of
/// the project's files, so a session never begins with a server that cannot start. Downloads what
/// Serena runs for a language on the way. `Err`: the languages whose server failed, and why.
pub fn check(home: &forge_home::Home, root: &Path) -> Result<Vec<String>, Vec<String>> {
    let listed = std::fs::read_to_string(root.join(serena::PROJECT_FILE)).unwrap_or_default();
    let langs: Vec<&serena::Language> = serena::LANGUAGES
        .iter()
        .filter(|l| {
            listed
                .lines()
                .any(|line| line.trim() == format!("- {}", l.id))
        })
        .collect();
    if langs.is_empty() {
        return Ok(Vec::new());
    }
    let files = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let notes = std::cell::RefCell::new(Vec::new());
    let mut cmd = command(home, root, &|n| notes.borrow_mut().push(n)).map_err(|e| vec![e])?;
    let mut child = cmd
        .spawn()
        .map_err(|e| vec![format!("Serena could not start: {e}")])?;
    let mut stdin = child.stdin.take().expect("piped");
    let (tx, rx) = channel::<String>();
    let stdout = child.stdout.take().expect("piped");
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    let mut call =
        |id: u64, method: &str, params: Value, wait: Duration| -> Result<Value, String> {
            let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            if !send(&mut stdin, &msg.to_string()) {
                return Err("Serena stopped".into());
            }
            let until = Instant::now() + wait;
            loop {
                let left = until.saturating_duration_since(Instant::now());
                let line = rx
                    .recv_timeout(left)
                    .map_err(|_| format!("no answer within {}s", wait.as_secs()))?;
                let Ok(m) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if m.get("id") == Some(&json!(id)) {
                    if method == "initialize" {
                        let done = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
                        send(&mut stdin, &done.to_string());
                    }
                    return match m.get("error") {
                        Some(e) => Err(e.to_string()),
                        None => Ok(m["result"].clone()),
                    };
                }
            }
        };
    let init = json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "forge", "version": env!("CARGO_PKG_VERSION")}});
    let mut ok = Vec::new();
    let mut failed = Vec::new();
    match call(1, "initialize", init, Duration::from_secs(120)) {
        Err(e) => failed.push(format!("Serena did not start: {e}")),
        Ok(_) => {
            for (n, l) in langs.iter().enumerate() {
                let Some(file) = files.split('\0').find(|f| {
                    f.rsplit_once('.')
                        .is_some_and(|(_, ext)| l.exts.contains(&ext))
                }) else {
                    continue;
                };
                let args =
                    json!({"name": "get_symbols_overview", "arguments": {"relative_path": file}});
                match call(n as u64 + 2, "tools/call", args, Duration::from_secs(600)) {
                    Ok(r) if r.get("isError").and_then(Value::as_bool) != Some(true) => {
                        ok.push(l.id.to_string());
                    }
                    Ok(r) => failed.push(format!(
                        "{}: {}",
                        l.id,
                        r["content"][0]["text"].as_str().unwrap_or("failed")
                    )),
                    Err(e) => failed.push(format!("{}: {e}", l.id)),
                }
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    failed.extend(notes.into_inner());
    if failed.is_empty() {
        Ok(ok)
    } else {
        Err(failed)
    }
}

/// The pinned tool's executable, installed when missing (downloads allowed unless turned off).
fn install(home: &forge_home::Home, tool: &str) -> Result<PathBuf, String> {
    let p = forge_tools::Provisioner::new(home.clone());
    if let Some(path) = p.resolve(tool) {
        return Ok(path);
    }
    if crate::provisioning::downloads_off() {
        return Err(format!(
            "{tool} is not installed and downloads are turned off ({})",
            crate::provisioning::ENV_NO_DOWNLOAD
        ));
    }
    crate::provisioning::use_forge_path();
    p.ensure(tool).map_err(|n| n.to_string())
}

/// `.serena/project.local.yml`: the path of each server Forge provides for the project's languages
/// (those `.serena/project.yml` lists).
fn write_local(home: &forge_home::Home, root: &Path, note: &dyn Fn(String)) {
    let listed = std::fs::read_to_string(root.join(serena::PROJECT_FILE)).unwrap_or_default();
    let mut servers: Vec<(&str, String)> = Vec::new();
    for l in serena::LANGUAGES {
        let Provider::Forge(tool) = l.provider else {
            continue;
        };
        if !listed
            .lines()
            .any(|line| line.trim() == format!("- {}", l.id))
        {
            continue;
        }
        match install(home, tool) {
            Ok(path) => servers.push((l.id, path.to_string_lossy().into_owned())),
            Err(e) => note(format!(
                "Forge's {tool} could not be installed for Serena ({e}); Serena uses its own"
            )),
        }
    }
    let text = serena::local_yml(&servers);
    let path = root.join(serena::LOCAL_FILE);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        let _ = std::fs::create_dir_all(root.join(serena::DIR));
        let _ = std::fs::write(&path, text);
    }
}

enum Event {
    /// A line from Claude Code.
    Client(String),
    /// Claude Code closed its end: the session is over.
    ClientClosed,
    /// A line from Serena (generation, line).
    Server(u64, String),
    /// Serena's output closed (generation).
    ServerClosed(u64),
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    generation: u64,
}

fn spawn(cmd: &mut Command, generation: u64, tx: &Sender<Event>) -> std::io::Result<Running> {
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().expect("piped");
    let stdout = child.stdout.take().expect("piped");
    let tx = tx.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(Event::Server(generation, line)).is_err() {
                return;
            }
        }
        let _ = tx.send(Event::ServerClosed(generation));
    });
    Ok(Running {
        child,
        stdin,
        generation,
    })
}

fn send(stdin: &mut ChildStdin, line: &str) -> bool {
    stdin
        .write_all(line.as_bytes())
        .and_then(|()| stdin.write_all(b"\n"))
        .and_then(|()| stdin.flush())
        .is_ok()
}

fn to_client(out: &Arc<Mutex<std::io::Stdout>>, line: &str) {
    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = o.write_all(line.as_bytes());
    let _ = o.write_all(b"\n");
    let _ = o.flush();
}

fn proxy(mut cmd: Command, note: &dyn Fn(String)) -> ExitCode {
    // Serena's log on stderr reaches Claude Code's MCP log
    cmd.stderr(Stdio::inherit());
    let (tx, rx): (Sender<Event>, Receiver<Event>) = channel();
    let client_tx = tx.clone();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if client_tx.send(Event::Client(line)).is_err() {
                return;
            }
        }
        let _ = client_tx.send(Event::ClientClosed);
    });
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let mut generation = 0;
    let mut server = match spawn(&mut cmd, generation, &tx) {
        Ok(s) => s,
        Err(e) => {
            note(format!("Serena could not start: {e}"));
            return ExitCode::from(1);
        }
    };
    let mut handshake: Option<String> = None;
    let mut initialized: Option<String> = None;
    let mut in_flight: Vec<Value> = Vec::new();
    let mut restarts: Vec<Instant> = Vec::new();
    while let Ok(ev) = rx.recv() {
        match ev {
            Event::Client(line) => {
                if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                    match msg.get("method").and_then(Value::as_str) {
                        Some("initialize") => handshake = Some(line.clone()),
                        Some("notifications/initialized") => initialized = Some(line.clone()),
                        _ => {}
                    }
                    if msg.get("method").is_some() {
                        if let Some(id) = msg.get("id") {
                            in_flight.push(id.clone());
                        }
                    }
                }
                send(&mut server.stdin, &line);
            }
            Event::ClientClosed => break,
            Event::Server(g, line) if g == server.generation => {
                let msg = serde_json::from_str::<Value>(&line).ok();
                let id = msg.as_ref().and_then(|m| {
                    m.get("method")
                        .is_none()
                        .then(|| m.get("id").cloned())
                        .flatten()
                });
                if id.as_ref().and_then(Value::as_str) == Some(REPLAY_ID) {
                    continue;
                }
                if let Some(id) = id {
                    in_flight.retain(|i| *i != id);
                }
                to_client(&out, &line);
            }
            Event::Server(..) => {}
            Event::ServerClosed(g) if g == server.generation => {
                let _ = server.child.wait();
                let now = Instant::now();
                restarts.retain(|t| now.duration_since(*t) < RESTART_WINDOW);
                if restarts.len() >= RESTARTS_MAX {
                    note(format!(
                        "Serena stopped {RESTARTS_MAX} times within {} minutes; it was not started again this session",
                        RESTART_WINDOW.as_secs() / 60
                    ));
                    return ExitCode::from(1);
                }
                restarts.push(now);
                for id in in_flight.drain(..) {
                    let err = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "The code tools restarted; make the call again."}});
                    to_client(&out, &err.to_string());
                }
                generation += 1;
                server = match spawn(&mut cmd, generation, &tx) {
                    Ok(s) => s,
                    Err(e) => {
                        note(format!("Serena could not be started again: {e}"));
                        return ExitCode::from(1);
                    }
                };
                note("Serena stopped during a session and was started again".into());
                if let Some(h) = &handshake {
                    if let Ok(mut m) = serde_json::from_str::<Value>(h) {
                        m["id"] = json!(REPLAY_ID);
                        send(&mut server.stdin, &m.to_string());
                    }
                }
                if let Some(i) = &initialized {
                    send(&mut server.stdin, i);
                }
            }
            Event::ServerClosed(_) => {}
        }
    }
    drop(server.stdin);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if matches!(server.child.try_wait(), Ok(Some(_))) {
            return ExitCode::SUCCESS;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = server.child.kill();
    ExitCode::SUCCESS
}
