//! The daemon: the socket, the sessions, and one worker thread that owns the servers and answers
//! questions in order (and loads TypeScript projects when it has nothing else to do).

use std::collections::{BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::python::Python;
use crate::typescript::TypeScript;
use crate::{Client, DiagFile, EngineConfig, Lang, Request, Response, Status, added, socket_addr};

struct Shared {
    status: Mutex<Status>,
    sessions: Mutex<BTreeSet<String>>,
    last: Mutex<Instant>,
    stopping: AtomicBool,
    log: Mutex<Option<std::fs::File>>,
    started: Instant,
}

impl Shared {
    fn log(&self, msg: &str) {
        if let Some(f) = self.log.lock().expect("log").as_mut() {
            let _ = writeln!(f, "{:>9.3} {msg}", self.started.elapsed().as_secs_f64());
        }
    }

    fn touch(&self) {
        *self.last.lock().expect("last") = Instant::now();
    }
}

enum Job {
    Request(Request, Sender<Response>),
    /// The last session left.
    Stop,
}

/// Files whose change means the servers must start again on the new environment.
fn restarts(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "uv.lock"
            | "poetry.lock"
            | "pdm.lock"
            | "Pipfile.lock"
            | "pyproject.toml"
            | "ty.toml"
            | "package.json"
            | "bun.lock"
            | "bun.lockb"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
    ) || (name.starts_with("tsconfig") && name.ends_with(".json"))
}

/// Runs the daemon of `config` until it is stopped, the last session leaves, it is idle for
/// `config.idle_secs`, or its socket or repository disappears (checked every 10 seconds).
/// Fails when another daemon answers on the socket or the socket cannot be bound.
pub fn serve(config: EngineConfig) -> Result<(), String> {
    let addr = socket_addr(&config.socket);
    if Client::new(&config.socket)
        .with_timeout(Duration::from_secs(2))
        .ping()
        .is_ok()
    {
        return Err(format!(
            "a types engine already answers on {}",
            addr.display()
        ));
    }
    if let Some(dir) = addr.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::remove_file(&addr);
    let listener =
        UnixListener::bind(&addr).map_err(|e| format!("cannot bind {}: {e}", addr.display()))?;
    let inode = std::fs::metadata(&addr).map(|m| m.ino()).unwrap_or(0);
    let log = config.log.as_ref().and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
    });
    let shared = Arc::new(Shared {
        status: Mutex::new(Status {
            pid: std::process::id(),
            root: config.root.clone(),
            want: config.want.clone(),
            ..Status::default()
        }),
        sessions: Mutex::default(),
        last: Mutex::new(Instant::now()),
        stopping: AtomicBool::new(false),
        log: Mutex::new(log),
        started: Instant::now(),
    });
    shared.log(&format!(
        "serving {} on {}",
        config.root.display(),
        addr.display()
    ));
    let (jobs, rx) = mpsc::channel::<Job>();
    let (done_tx, done) = mpsc::channel::<()>();
    let worker = {
        let shared = shared.clone();
        let config = config.clone();
        std::thread::spawn(move || {
            Worker::new(config, shared).run(rx);
            let _ = done_tx.send(());
        })
    };
    {
        let shared = shared.clone();
        let jobs = jobs.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if shared.stopping.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                let shared = shared.clone();
                let jobs = jobs.clone();
                std::thread::spawn(move || connection(conn, &shared, &jobs));
            }
        });
    }
    let idle = Duration::from_secs(config.idle_secs);
    let mut checked = Instant::now();
    loop {
        match done.recv_timeout(Duration::from_millis(500)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let last = *shared.last.lock().expect("last");
        if last.elapsed() >= idle {
            shared.log("idle, exiting");
            let _ = jobs.send(Job::Stop);
            continue;
        }
        if checked.elapsed() >= Duration::from_secs(10) {
            checked = Instant::now();
            let ours = std::fs::metadata(&addr).is_ok_and(|m| m.ino() == inode);
            if !ours || !config.root.is_dir() {
                shared.log("orphaned, exiting");
                let _ = jobs.send(Job::Stop);
            }
        }
    }
    let _ = worker.join();
    shared.stopping.store(true, Ordering::SeqCst);
    if std::fs::metadata(&addr).is_ok_and(|m| m.ino() == inode) {
        // wakes the accept loop so it sees the stop
        let _ = UnixStream::connect(&addr);
        let _ = std::fs::remove_file(&addr);
    }
    shared.log("stopped");
    Ok(())
}

fn connection(conn: UnixStream, shared: &Shared, jobs: &Sender<Job>) {
    let Ok(w) = conn.try_clone() else { return };
    let mut w = w;
    let mut r = BufReader::new(conn);
    let mut line = String::new();
    loop {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        shared.touch();
        let resp = match serde_json::from_str::<Request>(&line) {
            Err(e) => Response {
                error: Some(format!("unreadable request: {e}")),
                ..Response::default()
            },
            Ok(req) => answer(req, shared, jobs),
        };
        let mut out = serde_json::to_vec(&resp).expect("response serializes");
        out.push(b'\n');
        if w.write_all(&out).is_err() {
            return;
        }
    }
}

fn status(shared: &Shared) -> Status {
    let mut s = shared.status.lock().expect("status").clone();
    s.sessions = shared.sessions.lock().expect("sessions").len();
    s
}

fn answer(req: Request, shared: &Shared, jobs: &Sender<Job>) -> Response {
    let stopping = shared.stopping.load(Ordering::SeqCst);
    match req {
        Request::Ping => Response {
            status: Some(status(shared)),
            stopping,
            ..Response::default()
        },
        Request::Register { session } => {
            shared.sessions.lock().expect("sessions").insert(session);
            Response {
                status: Some(status(shared)),
                stopping,
                ..Response::default()
            }
        }
        Request::Unregister { session } => {
            let last = {
                let mut s = shared.sessions.lock().expect("sessions");
                s.remove(&session) && s.is_empty()
            };
            if last {
                shared.log("the last session ended");
                shared.stopping.store(true, Ordering::SeqCst);
                let _ = jobs.send(Job::Stop);
            }
            Response {
                stopping: last || stopping,
                ..Response::default()
            }
        }
        req => {
            let (tx, rx) = mpsc::channel();
            if jobs.send(Job::Request(req, tx)).is_err() {
                return Response {
                    error: Some("the types engine is stopping".into()),
                    stopping: true,
                    ..Response::default()
                };
            }
            rx.recv().unwrap_or_else(|_| Response {
                error: Some("the types engine is stopping".into()),
                stopping: true,
                ..Response::default()
            })
        }
    }
}

/// An engine, or why it could not start (and when: it is tried again after a while).
enum Slot<T> {
    Empty,
    Up(Box<T>),
    Failed(String, Instant),
}

struct Worker {
    config: EngineConfig,
    shared: Arc<Shared>,
    python: Slot<Python>,
    typescript: Slot<TypeScript>,
    preload: VecDeque<(Lang, PathBuf)>,
}

const RETRY: Duration = Duration::from_secs(30);

impl Worker {
    fn new(config: EngineConfig, shared: Arc<Shared>) -> Worker {
        Worker {
            config,
            shared,
            python: Slot::Empty,
            typescript: Slot::Empty,
            preload: VecDeque::new(),
        }
    }

    fn publish(&self) {
        let mut s = self.shared.status.lock().expect("status");
        s.servers.clear();
        s.failed.clear();
        match &self.python {
            Slot::Up(p) => {
                s.servers.insert(Lang::Python, p.name.clone());
            }
            Slot::Failed(why, _) => {
                s.failed.insert(Lang::Python, why.clone());
            }
            Slot::Empty => {}
        }
        match &self.typescript {
            Slot::Up(t) => {
                s.servers.insert(Lang::Typescript, t.name.clone());
            }
            Slot::Failed(why, _) => {
                s.failed.insert(Lang::Typescript, why.clone());
            }
            Slot::Empty => {}
        }
        let mut loading: Vec<Lang> = self.preload.iter().map(|p| p.0).collect();
        loading.dedup();
        s.loading = loading;
    }

    fn python(&mut self) -> Result<&mut Python, String> {
        let dead = matches!(&self.python, Slot::Up(p) if !p.alive());
        let retry = matches!(&self.python, Slot::Failed(_, at) if at.elapsed() > RETRY);
        if dead || retry || matches!(self.python, Slot::Empty) {
            let Some(cfg) = &self.config.python else {
                return Err("no Python types engine is configured".into());
            };
            let t = Instant::now();
            self.python = match Python::start(&self.config.root, cfg, &self.config.log) {
                Ok(p) => {
                    self.shared.log(&format!(
                        "started {} in {} ms",
                        p.name,
                        t.elapsed().as_millis()
                    ));
                    Slot::Up(Box::new(p))
                }
                Err(e) => {
                    self.shared.log(&format!("python failed: {e}"));
                    Slot::Failed(e, Instant::now())
                }
            };
            self.publish();
        }
        match &mut self.python {
            Slot::Up(p) => Ok(p),
            Slot::Failed(e, _) => Err(format!("did not start: {e}")),
            Slot::Empty => unreachable!(),
        }
    }

    fn typescript(&mut self) -> Result<&mut TypeScript, String> {
        let dead = matches!(&self.typescript, Slot::Up(p) if !p.alive());
        let retry = matches!(&self.typescript, Slot::Failed(_, at) if at.elapsed() > RETRY);
        if dead || retry || matches!(self.typescript, Slot::Empty) {
            let Some(cfg) = &self.config.typescript else {
                return Err("no TypeScript types engine is configured".into());
            };
            let t = Instant::now();
            self.typescript = match TypeScript::start(&self.config.root, cfg, &self.config.log) {
                Ok(ts) => {
                    self.shared.log(&format!(
                        "started {} in {} ms",
                        ts.name,
                        t.elapsed().as_millis()
                    ));
                    if cfg.preload {
                        self.preload.retain(|p| p.0 != Lang::Typescript);
                        for f in ts.preload_files() {
                            self.preload.push_back((Lang::Typescript, f));
                        }
                    }
                    Slot::Up(Box::new(ts))
                }
                Err(e) => {
                    self.shared.log(&format!("typescript failed: {e}"));
                    Slot::Failed(e, Instant::now())
                }
            };
            self.publish();
        }
        match &mut self.typescript {
            Slot::Up(p) => Ok(p),
            Slot::Failed(e, _) => Err(format!("did not start: {e}")),
            Slot::Empty => unreachable!(),
        }
    }

    fn stop_engines(&mut self) {
        if let Slot::Up(p) = &mut self.python {
            p.stop();
        }
        if let Slot::Up(t) = &mut self.typescript {
            t.stop();
        }
        self.python = Slot::Empty;
        self.typescript = Slot::Empty;
        self.preload.clear();
        self.publish();
    }

    fn run(mut self, rx: Receiver<Job>) {
        // the configured engines start at once, so the first question finds them ready
        if self.config.python.is_some() {
            let _ = self.python();
        }
        if self.config.typescript.is_some() {
            let _ = self.typescript();
        }
        loop {
            let wait = if self.preload.is_empty() {
                Duration::from_secs(3600)
            } else {
                Duration::ZERO
            };
            match rx.recv_timeout(wait) {
                Ok(Job::Request(Request::Stop, tx)) => {
                    self.shared.log("asked to stop");
                    self.shared.stopping.store(true, Ordering::SeqCst);
                    self.stop_engines();
                    let _ = tx.send(Response {
                        stopping: true,
                        ..Response::default()
                    });
                    return;
                }
                Ok(Job::Stop) => {
                    self.shared.stopping.store(true, Ordering::SeqCst);
                    self.stop_engines();
                    return;
                }
                Ok(Job::Request(req, tx)) => {
                    let resp = self.handle(req);
                    let _ = tx.send(resp);
                }
                Err(RecvTimeoutError::Timeout) => self.preload_one(),
                Err(RecvTimeoutError::Disconnected) => {
                    self.stop_engines();
                    return;
                }
            }
        }
    }

    fn preload_one(&mut self) {
        let Some((lang, file)) = self.preload.pop_front() else {
            return;
        };
        let t = Instant::now();
        if lang == Lang::Typescript {
            if let Slot::Up(ts) = &mut self.typescript {
                ts.warm(&file);
            }
        }
        self.shared.log(&format!(
            "loaded the project of {} in {} ms",
            file.display(),
            t.elapsed().as_millis()
        ));
        if self.preload.is_empty() {
            self.publish();
        }
    }

    fn handle(&mut self, req: Request) -> Response {
        let t = Instant::now();
        let r = match req {
            Request::Changed { paths } => {
                if paths.iter().any(|p| restarts(p)) {
                    self.shared
                        .log("dependencies changed: restarting the servers");
                    self.stop_engines();
                } else {
                    let root = self.config.root.clone();
                    for p in &paths {
                        let path = root.join(p);
                        if let Slot::Up(py) = &mut self.python {
                            if p.ends_with(".py") || p.ends_with(".pyi") {
                                py.changed(&path);
                            }
                        }
                        if let Slot::Up(ts) = &mut self.typescript {
                            ts.changed(&path);
                        }
                    }
                }
                Response::default()
            }
            Request::Restart => {
                self.stop_engines();
                Response::default()
            }
            Request::Types { lang, matches } => {
                let n = matches.len();
                let result = match lang {
                    Lang::Python => self.python().and_then(|p| p.answer(&matches)),
                    Lang::Typescript => self.typescript().and_then(|p| p.answer(&matches)),
                };
                self.shared.log(&format!(
                    "{lang}: answered {n} matches in {:.1} ms",
                    t.elapsed().as_secs_f64() * 1000.0
                ));
                match result {
                    Ok(v) => Response {
                        verdicts: Some(v),
                        ..Response::default()
                    },
                    Err(e) => self.failed(lang, e),
                }
            }
            Request::Diagnostics {
                lang,
                files,
                warnings,
            } => {
                let result = self.diagnose(lang, &files);
                self.shared.log(&format!(
                    "{lang}: diagnosed {} file(s) in {:.1} ms",
                    files.len(),
                    t.elapsed().as_secs_f64() * 1000.0
                ));
                match result {
                    Ok(per_file) => Response {
                        diagnostics: Some(
                            per_file
                                .iter()
                                .flat_map(|(now, base)| added(now, base, warnings))
                                .collect(),
                        ),
                        ..Response::default()
                    },
                    Err(e) => self.failed(lang, e),
                }
            }
            Request::Ping
            | Request::Register { .. }
            | Request::Unregister { .. }
            | Request::Stop => Response::default(),
        };
        self.shared.touch();
        r
    }

    fn diagnose(
        &mut self,
        lang: Lang,
        files: &[DiagFile],
    ) -> Result<Vec<crate::diag::FileDiagnostics>, String> {
        match lang {
            Lang::Python => self.python()?.diagnose(files),
            Lang::Typescript => self.typescript()?.diagnose(files),
        }
    }

    fn failed(&mut self, lang: Lang, e: String) -> Response {
        self.shared.log(&format!("{lang}: {e}"));
        // a server that died is started again by the next question
        let name = match lang {
            Lang::Python => match &self.python {
                Slot::Up(p) => p.name.clone(),
                _ => "ty".into(),
            },
            Lang::Typescript => match &self.typescript {
                Slot::Up(t) => t.name.clone(),
                _ => "TypeScript".into(),
            },
        };
        Response {
            error: Some(format!("{name} {e}")),
            ..Response::default()
        }
    }
}
