//! Child processes with a time cap (git, the reviewer).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What a capped run gave.
#[derive(Debug)]
pub struct Ran {
    pub ok: bool,
    pub stdout: Vec<u8>,
    /// It ran past its cap and was killed.
    pub timed_out: bool,
}

/// Runs `cmd` with `input` on stdin, killing it after `cap`. None when it cannot start.
pub fn run(cmd: &mut Command, input: Option<&[u8]>, cap: Duration) -> Option<Ran> {
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    {
        use std::os::unix::process::CommandExt;
        // its own group, so a run past its cap is killed with its children
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().ok()?;
    let writer = input.map(|bytes| {
        let mut stdin = child.stdin.take();
        let bytes = bytes.to_vec();
        std::thread::spawn(move || {
            if let Some(s) = stdin.as_mut() {
                let _ = s.write_all(&bytes);
            }
        })
    });
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = Instant::now() + cap;
    let mut sleep = Duration::from_micros(200);
    let (ok, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.success(), false),
            Ok(None) if Instant::now() >= deadline => {
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .stderr(Stdio::null())
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                break (false, true);
            }
            Ok(None) => {
                std::thread::sleep(sleep);
                sleep = (sleep * 2).min(Duration::from_millis(5));
            }
            Err(_) => break (false, false),
        }
    };
    if timed_out {
        // a grandchild may still hold the pipes: the reader threads are left to end with it
        return Some(Ran {
            ok,
            stdout: Vec::new(),
            timed_out,
        });
    }
    if let Some(w) = writer {
        let _ = w.join();
    }
    let stdout = reader.join().unwrap_or_default();
    Some(Ran {
        ok,
        stdout,
        timed_out,
    })
}

/// `git <args>` in `root`; its stdout when it succeeds within `cap`.
pub fn git(root: &Path, args: &[&str], cap: Duration) -> Option<Vec<u8>> {
    git_env(root, args, &[], None, cap)
        .filter(|r| r.ok)
        .map(|r| r.stdout)
}

/// `git <args>` with extra environment and stdin.
pub fn git_env(
    root: &Path,
    args: &[&str],
    env: &[(&str, &std::ffi::OsStr)],
    input: Option<&[u8]>,
    cap: Duration,
) -> Option<Ran> {
    let mut cmd = Command::new(git_exe());
    cmd.args(["-c", "core.quotePath=false", "-c", "maintenance.auto=false"])
        .args(args)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0");
    for (k, v) in env {
        cmd.env(k, v);
    }
    run(&mut cmd, input, cap)
}

fn is_exe(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The git executable: the one on PATH, except macOS's `/usr/bin/git` shim (which looks up the
/// developer tools on every call, ~10 ms), replaced by the git it runs. The answer is kept in the
/// state root (`facts/git-exe`, shared with the checker). `FORGE_GIT` overrides it.
pub fn git_exe() -> &'static Path {
    static EXE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    EXE.get_or_init(|| {
        if let Some(p) = std::env::var_os("FORGE_GIT").filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
        let on_path = std::env::var("PATH")
            .unwrap_or_default()
            .split(':')
            .map(|d| Path::new(d).join("git"))
            .find(|p| is_exe(p));
        let Some(git) = on_path else {
            return PathBuf::from("git");
        };
        if git != Path::new("/usr/bin/git") {
            return git;
        }
        let fact = forge_home::Home::from_env()
            .ok()
            .map(|h| h.state_root().join("facts").join("git-exe"));
        if let Some(f) = &fact {
            if let Ok(p) = std::fs::read_to_string(f) {
                if is_exe(Path::new(p.trim())) {
                    return PathBuf::from(p.trim());
                }
            }
        }
        let real = Command::new(&git)
            .arg("--exec-path")
            .stderr(Stdio::null())
            .output()
            .ok()
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
            .and_then(|d| {
                // <prefix>/libexec/git-core → <prefix>/bin/git
                let bin = d.parent()?.parent()?.join("bin/git");
                is_exe(&bin).then_some(bin)
            });
        match real {
            Some(r) => {
                if let Some(f) = &fact {
                    crate::state::write_atomic(f, r.display().to_string().as_bytes());
                }
                r
            }
            None => git,
        }
    })
}

/// The usual cap of a git call on a hook's path.
pub const GIT_CAP: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_and_input() {
        let r = run(
            Command::new("sh").args(["-c", "cat"]),
            Some(b"hi"),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(r.ok && r.stdout == b"hi");
        let r = run(
            Command::new("sh").args(["-c", "sleep 5"]),
            None,
            Duration::from_millis(50),
        )
        .unwrap();
        assert!(r.timed_out && !r.ok);
    }
}
