//! The rustup-style launcher.
//!
//! The `forge` / `claude-forge` binary a person installs is a launcher. On
//! every run it reads the repo's `forge.lock` and, if it pins a different
//! version, replaces itself (`exec`) with `<cache>/bin/forge-<version>`, so
//! everyone and CI run exactly the same built-in pack set. Argv, stdin,
//! stdout and the environment pass through untouched, and no extra process
//! is left behind.
//!
//! The CLI's `main` calls [`launch_from_env`] first; when it returns, the
//! current process is the right one to run the command.
//!
//! Loops are impossible: before exec we set [`LAUNCHED_ENV`] to the version
//! we are exec'ing. A binary that starts with that marker equal to the locked
//! version never execs again — if its own version differs (a corrupt or
//! mislabelled cache entry) it runs in-process and leaves a note. The marker
//! names a version rather than being a plain flag, so a nested Forge run in
//! another repo with a different pin still launches correctly.

pub mod fetch;
pub mod hook;
pub mod lock;
pub mod note;
pub mod policy;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use forge_home::{Home, RepoRoot};

pub use fetch::{FetchError, Fetcher, NoFetcher};
pub use hook::{hook_script, hook_wrapper};
pub use lock::{Lock, LockError, parse_lock, read_lock};
pub use note::{Note, record_note, take_notes};
pub use policy::{Invocation, MissingAction, on_missing};

/// Set on the exec'd process to the version the launcher exec'd.
pub const LAUNCHED_ENV: &str = "FORGE_LAUNCHED_VERSION";

/// What to do with a lock, before any side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// This process is the right version (or there is no pin): run here.
    /// The note, if any, explains why we run here despite a different pin.
    InProcess(Option<Note>),
    /// Exec this cached binary.
    Exec { version: String, path: PathBuf },
    /// The pinned version is not in the cache.
    Missing { version: String },
}

/// Pure decision. `cached` returns the path of an executable
/// `forge-<version>` in the cache, if there is one.
pub fn resolve_with(
    current: &str,
    lock: Option<&Lock>,
    launched: Option<&OsStr>,
    cached: impl FnOnce(&str) -> Option<PathBuf>,
) -> Decision {
    let Some(locked) = lock.and_then(|l| l.forge.as_deref()) else {
        return Decision::InProcess(None);
    };
    if locked == current {
        return Decision::InProcess(None);
    }
    if launched.is_some_and(|v| v == locked) {
        return Decision::InProcess(Some(Note::ExecVersionMismatch {
            locked: locked.to_owned(),
            running: current.to_owned(),
        }));
    }
    match cached(locked) {
        Some(path) => Decision::Exec {
            version: locked.to_owned(),
            path,
        },
        None => Decision::Missing {
            version: locked.to_owned(),
        },
    }
}

/// [`resolve_with`] against the real cache.
pub fn resolve(
    current: &str,
    lock: Option<&Lock>,
    launched: Option<&OsStr>,
    home: &Home,
) -> Decision {
    resolve_with(current, lock, launched, |v| {
        home.forge_binary(v).ok().filter(|p| is_executable(p))
    })
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Everything [`launch`] needs, so tests can supply it explicitly.
pub struct LaunchInput<'a> {
    /// Version of the running binary (`env!("CARGO_PKG_VERSION")`).
    pub current_version: &'a str,
    /// Full argv including `argv[0]`.
    pub args: Vec<OsString>,
    /// Value of [`LAUNCHED_ENV`] in our environment.
    pub launched: Option<OsString>,
    pub home: &'a Home,
    /// The repo, if we are inside one.
    pub repo: Option<&'a RepoRoot>,
    pub fetcher: &'a dyn Fetcher,
}

/// Runs the launcher. Returns only when this process should run the
/// command; on a successful exec it never returns. Returned notes have
/// already been recorded in the repo's state dir; the CLI may also show
/// them directly when a person is at the terminal.
pub fn launch(input: LaunchInput<'_>) -> Vec<Note> {
    let Some(repo) = input.repo else {
        return Vec::new();
    };
    let note = |n: Note| {
        // Best effort: failing to write a note must never break the hook.
        let _ = record_note(input.home, &repo.id(), &n);
        vec![n]
    };
    let lock = match read_lock(&repo.lock_path()) {
        Ok(l) => l,
        Err(e) => {
            return note(Note::BadLock {
                error: e.to_string(),
            });
        }
    };
    let decision = resolve(
        input.current_version,
        lock.as_ref(),
        input.launched.as_deref(),
        input.home,
    );
    let (version, path) = match decision {
        Decision::InProcess(None) => return Vec::new(),
        Decision::InProcess(Some(n)) => return note(n),
        Decision::Exec { version, path } => (version, path),
        Decision::Missing { version } => {
            let inv = Invocation::from_args(&input.args);
            let unavailable =
                |fetching, fetch_error: Option<FetchError>| Note::VersionUnavailable {
                    locked: version.clone(),
                    running: input.current_version.to_owned(),
                    fetching,
                    fetch_error: fetch_error.map(|e| e.to_string()),
                };
            match on_missing(&inv) {
                MissingAction::RunInstalled => return note(unavailable(false, None)),
                MissingAction::FetchInBackground => {
                    let r = input.fetcher.fetch_in_background(input.home, &version);
                    return note(unavailable(r.is_ok(), r.err()));
                }
                MissingAction::FetchThenExec => match input.fetcher.fetch(input.home, &version) {
                    Ok(path) => (version.clone(), path),
                    Err(e) => return note(unavailable(false, Some(e))),
                },
            }
        }
    };
    let error = exec(&path, &input.args, &version);
    note(Note::ExecFailed { path, error })
}

/// [`launch`] with everything read from the process environment.
/// Without a resolvable home or repo it simply runs in-process.
pub fn launch_from_env(current_version: &str, fetcher: &dyn Fetcher) -> Vec<Note> {
    let Ok(home) = Home::from_env() else {
        return Vec::new();
    };
    let repo = forge_home::find_repo_root_from_env().ok().flatten();
    launch(LaunchInput {
        current_version,
        args: std::env::args_os().collect(),
        launched: std::env::var_os(LAUNCHED_ENV),
        home: &home,
        repo: repo.as_ref(),
        fetcher,
    })
}

/// Replaces this process with `path`, passing argv (including `argv[0]`)
/// through and marking the environment. Returns only on failure.
#[cfg(unix)]
fn exec(path: &Path, args: &[OsString], version: &str) -> String {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(path);
    if let Some(argv0) = args.first() {
        cmd.arg0(argv0);
    }
    let err = cmd
        .args(args.iter().skip(1))
        .env(LAUNCHED_ENV, version)
        .exec();
    err.to_string()
}

/// Without `exec`, run the child to completion and exit with its status.
#[cfg(not(unix))]
fn exec(path: &Path, args: &[OsString], version: &str) -> String {
    match std::process::Command::new(path)
        .args(args.iter().skip(1))
        .env(LAUNCHED_ENV, version)
        .status()
    {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(v: &str) -> Lock {
        Lock {
            forge: Some(v.into()),
        }
    }

    fn cached(v: &str) -> Option<PathBuf> {
        (v == "2.0.0").then(|| PathBuf::from("/c/bin/forge-2.0.0"))
    }

    #[test]
    fn decisions() {
        assert_eq!(
            resolve_with("1.0.0", None, None, cached),
            Decision::InProcess(None)
        );
        assert_eq!(
            resolve_with("1.0.0", Some(&Lock::default()), None, cached),
            Decision::InProcess(None)
        );
        assert_eq!(
            resolve_with("1.0.0", Some(&lock("1.0.0")), None, cached),
            Decision::InProcess(None)
        );
        assert_eq!(
            resolve_with("1.0.0", Some(&lock("2.0.0")), None, cached),
            Decision::Exec {
                version: "2.0.0".into(),
                path: "/c/bin/forge-2.0.0".into()
            }
        );
        assert_eq!(
            resolve_with("1.0.0", Some(&lock("3.0.0")), None, cached),
            Decision::Missing {
                version: "3.0.0".into()
            }
        );
    }

    #[test]
    fn no_loop_after_exec() {
        let d = resolve_with(
            "1.0.0",
            Some(&lock("2.0.0")),
            Some(OsStr::new("2.0.0")),
            cached,
        );
        assert!(matches!(
            d,
            Decision::InProcess(Some(Note::ExecVersionMismatch { .. }))
        ));
        // A marker for another version (nested run in another repo) does not block.
        let d = resolve_with(
            "1.0.0",
            Some(&lock("2.0.0")),
            Some(OsStr::new("9.0.0")),
            cached,
        );
        assert!(matches!(d, Decision::Exec { .. }));
    }

    #[test]
    fn non_executable_cache_entry_is_missing() {
        let t = tempfile::tempdir().unwrap();
        let home = Home::new(t.path().into(), t.path().join("state"));
        std::fs::create_dir_all(home.bin_dir()).unwrap();
        std::fs::write(home.forge_binary("2.0.0").unwrap(), "").unwrap();
        assert!(matches!(
            resolve("1.0.0", Some(&lock("2.0.0")), None, &home),
            Decision::Missing { .. }
        ));
    }
}
