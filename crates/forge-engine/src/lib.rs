//! The types engine (spec §15): one daemon per repository, on a Unix socket, that keeps the type
//! checkers' servers running (`ty server` for Python; the project's own TypeScript: `tsc --lsp` for
//! 7 and later, tsserver for 5 and 6) and answers what the checker asks:
//!
//! - the type of an expression, or the declaration a name refers to, or whether a name is a
//!   deprecated use, for the matches of rules with `where` ([`Request::Types`]);
//! - the diagnostics a change added to files, against their base copy ([`Request::Diagnostics`]);
//! - notifications of changed files ([`Request::Changed`]), sessions ([`Request::Register`],
//!   [`Request::Unregister`]: the last session out stops it), [`Request::Ping`], [`Request::Stop`].
//!
//! Protocol: one JSON object per line each way over the socket; a connection may carry several
//! requests. Questions are answered one at a time, in order; pings, registrations and
//! notifications are answered at once.
//!
//! A failure of the engine (it did not start, it is still loading, a binding it cannot ask) is
//! never a failing finding: [`judge`] turns it into a [`Note`] for the person.

mod config;
mod daemon;
mod diag;
mod python;
mod rpc;
mod spec;
mod text;
mod tsscan;
mod typescript;

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use forge_core::{Finding, Note, NoteKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use config::{EngineConfig, PythonConfig, TypeScriptConfig};
pub use daemon::serve;
pub use diag::{Diagnostic, added};
pub use spec::{Binding, Condition, Match, OneOrMany, Spec, Unknown, Verdict};

/// A language with a types engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Python,
    Typescript,
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Lang::Python => "python",
            Lang::Typescript => "typescript",
        })
    }
}

/// A file of the edit-time typecheck: its repository path and its base copy (none for a new file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagFile {
    pub path: String,
    #[serde(default)]
    pub base: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Request {
    Ping,
    Register {
        session: String,
    },
    Unregister {
        session: String,
    },
    /// Files changed on disk (repository-relative). A dependency or configuration file (a lock
    /// file, `pyproject.toml`, `ty.toml`, `package.json`, `tsconfig*.json`) restarts the servers.
    Changed {
        paths: Vec<String>,
    },
    Types {
        lang: Lang,
        matches: Vec<Match>,
    },
    Diagnostics {
        lang: Lang,
        files: Vec<DiagFile>,
        #[serde(default)]
        warnings: bool,
    },
    /// Restarts the servers (the dependencies changed).
    Restart,
    /// Stops the daemon; answered once its servers have exited.
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Status {
    pub pid: u32,
    pub root: PathBuf,
    pub want: String,
    /// The servers running: language → name and version (`ty 0.0.86`, `tsc 7.0.2`, `tsserver 5.9.3`).
    pub servers: BTreeMap<Lang, String>,
    /// Languages whose server could not start, with why.
    pub failed: BTreeMap<Lang, String>,
    /// Languages whose project is still loading.
    pub loading: Vec<Lang>,
    pub sessions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Response {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdicts: Option<Vec<Verdict>>,
    /// The diagnostics the change added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<Diagnostic>>,
    /// The daemon is stopping (a stop, or the last session left).
    #[serde(default)]
    pub stopping: bool,
}

/// Why the engine gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// No daemon answers on the socket.
    NotRunning(String),
    /// The daemon answered with an error (the server did not start, a question failed).
    Failed(String),
    /// No answer in time while the project is still loading.
    Loading,
    /// No answer in time.
    Timeout,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NotRunning(why) => write!(f, "the types engine is not running: {why}"),
            EngineError::Failed(why) => f.write_str(why),
            EngineError::Loading => f.write_str("the types engine is still loading the project"),
            EngineError::Timeout => f.write_str("the types engine gave no answer in time"),
        }
    }
}

/// The longest socket path `bind` takes everywhere (macOS's `sun_path` is 104 bytes, Linux's 108,
/// both with the terminating NUL).
const SUN_PATH_MAX: usize = 103;

/// The socket the daemon binds for `path`: `path` itself when it fits, else a short name in
/// `$TMPDIR` (or `/tmp`) derived from it.
pub fn socket_addr(path: &Path) -> PathBuf {
    if path.as_os_str().len() <= SUN_PATH_MAX {
        return path.to_path_buf();
    }
    let hash = Sha256::digest(path.as_os_str().as_encoded_bytes());
    let name = format!(
        "forge-{}.sock",
        hash.iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|d| d.join(&name).as_os_str().len() <= SUN_PATH_MAX && d.is_dir())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    tmp.join(name)
}

pub(crate) fn log_stdio(log: &Option<PathBuf>) -> Stdio {
    log.as_ref()
        .and_then(|p| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .ok()
        })
        .map_or_else(Stdio::null, Stdio::from)
}

/// A connection to the daemon of one repository.
#[derive(Debug, Clone)]
pub struct Client {
    addr: PathBuf,
    /// How long a request may take.
    pub timeout: Duration,
}

impl Client {
    /// The client of the daemon whose socket is `socket` (as the caller names it).
    pub fn new(socket: &Path) -> Client {
        Client {
            addr: socket_addr(socket),
            timeout: Duration::from_secs(20),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    /// One request and its response.
    pub fn request(&self, req: &Request) -> Result<Response, EngineError> {
        let mut s =
            UnixStream::connect(&self.addr).map_err(|e| EngineError::NotRunning(e.to_string()))?;
        s.set_read_timeout(Some(self.timeout)).ok();
        s.set_write_timeout(Some(self.timeout)).ok();
        let mut line = serde_json::to_vec(req).expect("request serializes");
        line.push(b'\n');
        s.write_all(&line)
            .map_err(|e| EngineError::NotRunning(e.to_string()))?;
        let mut r = BufReader::new(s);
        let mut out = String::new();
        match r.read_line(&mut out) {
            Ok(0) => Err(EngineError::Failed(
                "the types engine closed the connection".into(),
            )),
            Ok(_) => serde_json::from_str(&out)
                .map_err(|e| EngineError::Failed(format!("unreadable answer: {e}"))),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Err(EngineError::Timeout)
            }
            Err(e) => Err(EngineError::Failed(e.to_string())),
        }
    }

    fn checked(&self, req: &Request) -> Result<Response, EngineError> {
        let r = self.request(req)?;
        match r.error {
            Some(e) => Err(EngineError::Failed(e)),
            None => Ok(r),
        }
    }

    pub fn ping(&self) -> Result<Status, EngineError> {
        Ok(self.checked(&Request::Ping)?.status.unwrap_or_default())
    }

    pub fn register(&self, session: &str) -> Result<(), EngineError> {
        self.checked(&Request::Register {
            session: session.to_owned(),
        })
        .map(|_| ())
    }

    /// Unregisters a session; true when it was the last one and the daemon stops.
    pub fn unregister(&self, session: &str) -> Result<bool, EngineError> {
        self.checked(&Request::Unregister {
            session: session.to_owned(),
        })
        .map(|r| r.stopping)
    }

    pub fn changed(&self, paths: &[String]) -> Result<(), EngineError> {
        self.checked(&Request::Changed {
            paths: paths.to_vec(),
        })
        .map(|_| ())
    }

    /// The verdict of each match. A timeout while the project loads is [`EngineError::Loading`].
    pub fn types(&self, lang: Lang, matches: &[Match]) -> Result<Vec<Verdict>, EngineError> {
        let r = self.checked(&Request::Types {
            lang,
            matches: matches.to_vec(),
        });
        match r {
            Ok(r) => Ok(r.verdicts.unwrap_or_default()),
            Err(EngineError::Timeout) => {
                let loading = Client {
                    addr: self.addr.clone(),
                    timeout: Duration::from_secs(1),
                }
                .ping()
                .is_ok_and(|s| s.loading.contains(&lang));
                Err(if loading {
                    EngineError::Loading
                } else {
                    EngineError::Timeout
                })
            }
            Err(e) => Err(e),
        }
    }

    /// The diagnostics each change added (errors; warnings too with `warnings`).
    pub fn diagnostics(
        &self,
        lang: Lang,
        files: &[DiagFile],
        warnings: bool,
    ) -> Result<Vec<Diagnostic>, EngineError> {
        self.checked(&Request::Diagnostics {
            lang,
            files: files.to_vec(),
            warnings,
        })
        .map(|r| r.diagnostics.unwrap_or_default())
    }

    pub fn restart(&self) -> Result<(), EngineError> {
        self.checked(&Request::Restart).map(|_| ())
    }

    /// Stops the daemon and waits until its servers have exited.
    pub fn stop(&self) -> Result<(), EngineError> {
        self.checked(&Request::Stop)?;
        // the socket goes away once the daemon is done
        let until = Instant::now() + Duration::from_secs(5);
        while self.addr.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

/// Runs `cmd` detached from the caller (its own process group, no stdio), so the hook that started
/// it can end.
pub fn spawn_detached(cmd: &mut Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

/// The daemon of `config` runs, with its `want`: started with `start` when no daemon answers, or
/// when the one running has another `want` (stopped first). Starters are serialised by a lock
/// directory next to the socket. Registers `session` when given.
pub fn ensure(
    config: &EngineConfig,
    session: Option<&str>,
    timeout: Duration,
    start: impl FnOnce() -> std::io::Result<()>,
) -> Result<Client, EngineError> {
    let client = Client::new(&config.socket);
    let quick = client.clone().with_timeout(Duration::from_secs(2));
    let running = |s: &Status| s.want == config.want;
    let register = |c: &Client| -> Result<(), EngineError> {
        if let Some(id) = session {
            c.register(id)?;
        }
        Ok(())
    };
    match quick.ping() {
        Ok(s) if running(&s) => {
            register(&client)?;
            return Ok(client);
        }
        Ok(_) => {
            let _ = client.stop();
        }
        Err(_) => {}
    }
    let lock = PathBuf::from(format!("{}.lock", client.addr.display()));
    let until = Instant::now() + timeout;
    loop {
        match std::fs::create_dir(&lock) {
            Ok(()) => {
                let _ = std::fs::write(lock.join("pid"), std::process::id().to_string());
                break;
            }
            Err(_) => {
                // a starter that died leaves its lock: taken over
                let pid = std::fs::read_to_string(lock.join("pid")).unwrap_or_default();
                let alive = pid.trim().parse::<i32>().is_ok_and(|p| {
                    Command::new("kill")
                        .args(["-0", &p.to_string()])
                        .stderr(Stdio::null())
                        .status()
                        .is_ok_and(|s| s.success())
                });
                if !alive && !pid.trim().is_empty() {
                    let _ = std::fs::remove_dir_all(&lock);
                    continue;
                }
                if Instant::now() > until {
                    return Err(EngineError::NotRunning(
                        "another process is still starting it".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    let result = (|| {
        // started meanwhile by the one that held the lock
        if quick.ping().is_ok_and(|s| running(&s)) {
            return Ok(());
        }
        start().map_err(|e| EngineError::NotRunning(format!("cannot start it: {e}")))?;
        while Instant::now() < until {
            if let Ok(s) = quick.ping() {
                if running(&s) {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(EngineError::NotRunning(format!(
            "not started within {}s",
            timeout.as_secs()
        )))
    })();
    let _ = std::fs::remove_dir_all(&lock);
    result?;
    register(&client)?;
    Ok(client)
}

/// What the checker keeps of a `types` question: whether each match stands, and the notes for the
/// person. A failure of the engine never fails a check: the matches it could not confirm are
/// dropped and a note says which rules were not checked; at edit time (`edit`), an engine still
/// loading is silent (the Stop check asks again).
pub fn judge(
    lang: Lang,
    matches: &[Match],
    answer: &Result<Vec<Verdict>, EngineError>,
    edit: bool,
) -> (Vec<bool>, Vec<Note>) {
    let rules = || {
        let mut r: Vec<&str> = matches.iter().map(|m| m.rule.as_str()).collect();
        r.sort_unstable();
        r.dedup();
        r
    };
    match answer {
        Ok(v) if v.len() == matches.len() => {
            let mut notes = Vec::new();
            let keep = v
                .iter()
                .zip(matches)
                .map(|(v, m)| match v {
                    Verdict::Holds => true,
                    Verdict::Fails => false,
                    Verdict::Unchecked { why } => {
                        notes.push(
                            Note::new(NoteKind::NotEnforced, format!("not checked: {why}"))
                                .for_rule(&m.rule),
                        );
                        false
                    }
                })
                .collect();
            (keep, notes)
        }
        Err(EngineError::Loading) if edit => (vec![false; matches.len()], Vec::new()),
        other => {
            let why = match other {
                Ok(_) => "the types engine gave an incomplete answer".to_owned(),
                Err(e) => e.to_string(),
            };
            let notes = rules()
                .into_iter()
                .map(|r| {
                    Note::new(
                        NoteKind::ForgeFailure,
                        format!("not checked: the {lang} types engine: {why}"),
                    )
                    .for_rule(r)
                })
                .collect();
            (vec![false; matches.len()], notes)
        }
    }
}

/// The findings of an edit-time typecheck: one per diagnostic the change added.
pub fn diagnostic_findings(rule: &str, diags: &[Diagnostic]) -> Vec<Finding> {
    diags
        .iter()
        .map(|d| Finding::error(rule, d.key()).at(&d.path, Some(d.line)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_socket_paths_fall_back() {
        let long = PathBuf::from(format!("/{}/engine/repo.sock", "x".repeat(120)));
        let a = socket_addr(&long);
        assert!(a.as_os_str().len() <= SUN_PATH_MAX);
        assert_eq!(a, socket_addr(&long));
        assert_eq!(
            socket_addr(Path::new("/tmp/a.sock")),
            Path::new("/tmp/a.sock")
        );
    }

    #[test]
    fn engine_failures_are_notes() {
        let m = Match {
            file: "a.py".into(),
            rule: "python/x".into(),
            line: 1,
            bindings: BTreeMap::new(),
            conditions: BTreeMap::new(),
        };
        let (keep, notes) = judge(
            Lang::Python,
            std::slice::from_ref(&m),
            &Err(EngineError::Failed("ty server did not start".into())),
            false,
        );
        assert_eq!(keep, [false]);
        assert_eq!(notes[0].kind, NoteKind::ForgeFailure);
        assert!(forge_core::internals::find(&notes[0].text).is_empty());
        let (keep, notes) = judge(Lang::Python, &[m], &Err(EngineError::Loading), true);
        assert_eq!(keep, [false]);
        assert!(notes.is_empty());
    }
}
