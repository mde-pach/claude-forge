//! This machine's Docker daemon (machine state, never a repository's): where its socket is, and
//! whether a daemon answers there. `forge apply` names the socket in Claude Code's sandbox
//! settings (§7.2); the test checks ask whether a daemon answers before running tests that need
//! one (§10). Nothing here starts a process: the Docker CLI's context and Podman's machine are
//! read from their files.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a daemon has to answer `GET /_ping`.
const PING_TIMEOUT: Duration = Duration::from_millis(400);

/// Where a Docker API is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A Unix socket.
    Unix(PathBuf),
    /// `tcp://host:port`, as `host:port`.
    Tcp(String),
    /// Another scheme (`ssh://`, `npipe://`): not checked.
    Other(String),
}

/// A socket found on this machine, and the runtime it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Socket {
    pub path: PathBuf,
    /// `DOCKER_HOST`, `rootless Docker`, `Podman`, `Docker context <name>`, `Docker Desktop`,
    /// `OrbStack`, `Colima`, `Rancher Desktop`, `Docker`.
    pub runtime: String,
}

/// What the lookups read: the environment and the home directory. A struct so tests can give
/// their own.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub docker_host: Option<String>,
    pub docker_context: Option<String>,
    pub docker_config: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    pub tmpdir: Option<PathBuf>,
}

impl Env {
    /// This process's environment.
    pub fn current() -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        Env {
            home: var("HOME").map(PathBuf::from),
            docker_host: var("DOCKER_HOST").map(|v| v.to_string_lossy().into_owned()),
            docker_context: var("DOCKER_CONTEXT").map(|v| v.to_string_lossy().into_owned()),
            docker_config: var("DOCKER_CONFIG").map(PathBuf::from),
            xdg_runtime_dir: var("XDG_RUNTIME_DIR").map(PathBuf::from),
            tmpdir: var("TMPDIR").map(PathBuf::from),
        }
    }
}

/// `unix:///p` → a socket; `tcp://h:p` → TCP; anything else as it is.
pub fn parse_host(host: &str) -> Endpoint {
    if let Some(p) = host.strip_prefix("unix://") {
        Endpoint::Unix(PathBuf::from(p))
    } else if let Some(a) = host.strip_prefix("tcp://") {
        Endpoint::Tcp(a.trim_end_matches('/').to_string())
    } else {
        Endpoint::Other(host.to_string())
    }
}

/// The Docker CLI's current context's endpoint, when it is not `default`: `DOCKER_CONTEXT`, else
/// `currentContext` in `config.json`, looked up in `contexts/meta/*/meta.json` by name.
pub fn context_host(env: &Env) -> Option<(String, String)> {
    let dir = env
        .docker_config
        .clone()
        .or_else(|| env.home.as_ref().map(|h| h.join(".docker")))?;
    let name = match &env.docker_context {
        Some(n) => n.clone(),
        None => {
            let cfg: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).ok()?)
                    .ok()?;
            cfg.get("currentContext")?.as_str()?.to_string()
        }
    };
    if name.is_empty() || name == "default" {
        return None;
    }
    for e in std::fs::read_dir(dir.join("contexts/meta")).ok()?.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path().join("meta.json")) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if meta.get("Name").and_then(|v| v.as_str()) == Some(name.as_str()) {
            let host = meta.pointer("/Endpoints/docker/Host")?.as_str()?;
            return Some((name, host.to_string()));
        }
    }
    None
}

/// Every well-known socket path, in the order Forge prefers them: rootless Docker and Podman
/// (option C: the daemon does not run as root), then the CLI's context, then Docker Desktop,
/// OrbStack, Colima, Rancher Desktop and the system socket. Whether each exists is not checked.
pub fn candidates(env: &Env) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let mut push = |p: PathBuf, r: &str| out.push((p, r.to_string()));
    if let Some(x) = &env.xdg_runtime_dir {
        push(x.join("docker.sock"), "rootless Docker");
        push(x.join("podman/podman.sock"), "Podman");
    }
    // Podman's machine on macOS: `podman machine inspect`'s ConnectionInfo.PodmanSocket.Path
    // is `$TMPDIR/podman/<machine>-api.sock` (Podman 5) or under ~/.local/share (Podman 4)
    if let Some(t) = &env.tmpdir {
        let mut socks: Vec<PathBuf> = std::fs::read_dir(t.join("podman"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with("-api.sock"))
            .collect();
        socks.sort();
        for p in socks {
            push(p, "Podman");
        }
    }
    if let Some(h) = &env.home {
        push(
            h.join(".local/share/containers/podman/machine/podman.sock"),
            "Podman",
        );
    }
    if let Some((name, host)) = context_host(env)
        && let Endpoint::Unix(p) = parse_host(&host)
    {
        push(p, &format!("Docker context {name}"));
    }
    if let Some(h) = &env.home {
        push(h.join(".docker/run/docker.sock"), "Docker Desktop");
        push(h.join(".docker/desktop/docker.sock"), "Docker Desktop");
        push(h.join(".orbstack/run/docker.sock"), "OrbStack");
        push(h.join(".colima/default/docker.sock"), "Colima");
        push(h.join(".colima/docker.sock"), "Colima");
        push(h.join(".rd/docker.sock"), "Rancher Desktop");
    }
    push(PathBuf::from("/var/run/docker.sock"), "Docker");
    out
}

fn is_socket(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::metadata(p).is_ok_and(|m| m.file_type().is_socket())
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        false
    }
}

/// The socket tests on this machine reach Docker through: `DOCKER_HOST`'s when it names one
/// (taken as it is), else the first [`candidates`] entry that is a socket a daemon answers on,
/// else the first that is a socket (a runtime that is stopped: a stale Docker Desktop socket
/// beside a running OrbStack is passed over).
pub fn socket(env: &Env) -> Option<Socket> {
    if let Some(h) = &env.docker_host {
        return match parse_host(h) {
            Endpoint::Unix(path) => Some(Socket {
                path,
                runtime: "DOCKER_HOST".into(),
            }),
            _ => None,
        };
    }
    let present: Vec<(PathBuf, String)> = candidates(env)
        .into_iter()
        .filter(|(p, _)| is_socket(p))
        .collect();
    let live = present
        .iter()
        .position(|(p, _)| answers(&Endpoint::Unix(p.clone())))
        .unwrap_or(0);
    present
        .into_iter()
        .nth(live)
        .map(|(path, runtime)| Socket { path, runtime })
}

/// The paths a sandbox must allow for `sock`: the socket, the file it links to, and each other
/// well-known path that is the same socket (Docker Desktop's `/var/run/docker.sock` links to
/// `~/.docker/run/docker.sock`; Testcontainers for Python only knows the former).
pub fn sandbox_paths(env: &Env, sock: &Socket) -> Vec<PathBuf> {
    let mut out = vec![sock.path.clone()];
    let real = std::fs::canonicalize(&sock.path).ok();
    if let Some(r) = &real
        && *r != sock.path
    {
        out.push(r.clone());
    }
    if let Some(r) = &real {
        for (p, _) in candidates(env) {
            if !out.contains(&p) && std::fs::canonicalize(&p).ok().as_ref() == Some(r) {
                out.push(p);
            }
        }
    }
    out
}

/// Whether a Docker API answers `GET /_ping` at `ep` within [`PING_TIMEOUT`]. An endpoint Forge
/// cannot check (`ssh://`) counts as answering.
pub fn answers(ep: &Endpoint) -> bool {
    fn ping(mut s: impl Read + Write) -> bool {
        if s.write_all(b"GET /_ping HTTP/1.0\r\nHost: docker\r\n\r\n")
            .is_err()
        {
            return false;
        }
        let mut buf = [0u8; 64];
        let mut got = 0;
        while got < 16 {
            match s.read(&mut buf[got..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => got += n,
            }
        }
        let head = String::from_utf8_lossy(&buf[..got]);
        head.starts_with("HTTP/1.") && head.split_whitespace().nth(1) == Some("200")
    }
    match ep {
        #[cfg(unix)]
        Endpoint::Unix(p) => {
            let Ok(s) = std::os::unix::net::UnixStream::connect(p) else {
                return false;
            };
            let _ = s.set_read_timeout(Some(PING_TIMEOUT));
            let _ = s.set_write_timeout(Some(PING_TIMEOUT));
            ping(s)
        }
        #[cfg(not(unix))]
        Endpoint::Unix(_) => false,
        Endpoint::Tcp(addr) => {
            use std::net::ToSocketAddrs;
            let Some(a) = addr.to_socket_addrs().ok().and_then(|mut a| a.next()) else {
                return false;
            };
            let Ok(s) = std::net::TcpStream::connect_timeout(&a, PING_TIMEOUT) else {
                return false;
            };
            let _ = s.set_read_timeout(Some(PING_TIMEOUT));
            let _ = s.set_write_timeout(Some(PING_TIMEOUT));
            ping(s)
        }
        Endpoint::Other(_) => true,
    }
}

/// Whether a Docker daemon answers on this machine: at `DOCKER_HOST` when it is set, else at
/// one of the [`candidates`] sockets that exist.
pub fn daemon_answers(env: &Env) -> bool {
    if let Some(h) = &env.docker_host {
        return answers(&parse_host(h));
    }
    candidates(env)
        .into_iter()
        .filter(|(p, _)| is_socket(p))
        .any(|(p, _)| answers(&Endpoint::Unix(p)))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn env(home: &Path) -> Env {
        Env {
            home: Some(home.to_path_buf()),
            ..Env::default()
        }
    }

    #[test]
    fn hosts_parse() {
        assert_eq!(
            parse_host("unix:///var/run/docker.sock"),
            Endpoint::Unix("/var/run/docker.sock".into())
        );
        assert_eq!(
            parse_host("tcp://127.0.0.1:2375"),
            Endpoint::Tcp("127.0.0.1:2375".into())
        );
        assert!(matches!(parse_host("ssh://me@box"), Endpoint::Other(_)));
    }

    #[test]
    fn the_context_names_the_socket() {
        let t = tempfile_dir();
        let d = t.join(".docker");
        std::fs::create_dir_all(d.join("contexts/meta/abc")).unwrap();
        std::fs::write(d.join("config.json"), r#"{"currentContext":"colima"}"#).unwrap();
        std::fs::write(
            d.join("contexts/meta/abc/meta.json"),
            r#"{"Name":"colima","Endpoints":{"docker":{"Host":"unix:///x/colima.sock"}}}"#,
        )
        .unwrap();
        assert_eq!(
            context_host(&env(&t)),
            Some(("colima".into(), "unix:///x/colima.sock".into()))
        );
        std::fs::write(d.join("config.json"), r#"{"currentContext":"default"}"#).unwrap();
        assert_eq!(context_host(&env(&t)), None);
        let _ = std::fs::remove_dir_all(&t);
    }

    /// A daemon that answers the ping on `n` connections.
    fn serve(l: UnixListener, n: usize) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            for _ in 0..n {
                let (mut c, _) = l.accept().unwrap();
                let mut b = [0u8; 256];
                let _ = c.read(&mut b);
                let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
            }
        })
    }

    #[test]
    fn a_live_socket_is_preferred_rootless_first() {
        let t = tempfile_dir();
        let run = t.join("run");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(t.join(".docker/run")).unwrap();
        let desktop = t.join(".docker/run/docker.sock");
        let h = serve(UnixListener::bind(&desktop).unwrap(), 1);
        let mut e = env(&t);
        assert_eq!(socket(&e).unwrap().runtime, "Docker Desktop");
        h.join().unwrap();
        let rootless = run.join("docker.sock");
        let h = serve(UnixListener::bind(&rootless).unwrap(), 2);
        e.xdg_runtime_dir = Some(run.clone());
        let s = socket(&e).unwrap();
        assert_eq!(
            (s.path.clone(), s.runtime.as_str()),
            (rootless.clone(), "rootless Docker")
        );
        assert!(answers(&Endpoint::Unix(rootless)));
        h.join().unwrap();
        // no socket there: no answer, at once
        let started = std::time::Instant::now();
        assert!(!answers(&Endpoint::Unix(t.join("missing.sock"))));
        assert!(started.elapsed() < Duration::from_millis(100));
        e.docker_host = Some("unix:///nowhere.sock".into());
        assert_eq!(socket(&e).unwrap().path, PathBuf::from("/nowhere.sock"));
        assert!(!daemon_answers(&e));
        let _ = std::fs::remove_dir_all(&t);
    }

    fn tempfile_dir() -> PathBuf {
        // short: a Unix socket path is limited to about 100 bytes
        let p = std::env::temp_dir().join(format!(
            "fd{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
