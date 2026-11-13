//! End-to-end launcher behaviour with real `exec`.
//!
//! The test binary re-runs itself as the launcher: with `FORGE_TEST_ROLE`
//! set, `role_launcher` calls `launch_from_env` and, if it returns, prints
//! what an in-process run would see. The other tests drive that child.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use forge_home::{Home, RepoId};
use forge_launcher::{LAUNCHED_ENV, NoFetcher, launch_from_env, take_notes};

#[test]
fn role_launcher() {
    let Ok(current) = std::env::var("FORGE_TEST_ROLE") else {
        return;
    };
    let notes = launch_from_env(&current, &NoFetcher);
    let kinds: Vec<&str> = notes.iter().map(|n| n.kind()).collect();
    println!(
        "IN-PROCESS {current} notes={kinds:?} launched={:?}",
        std::env::var(LAUNCHED_ENV).ok()
    );
    std::process::exit(0);
}

struct Fixture {
    _t: tempfile::TempDir,
    home: Home,
    repo: std::path::PathBuf,
}

fn fixture(lock_version: &str) -> Fixture {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let home = Home::new(base.join("cache"), base.join("cache/state"));
    let repo = base.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(
        repo.join("forge.lock"),
        format!("forge = \"{lock_version}\"\n"),
    )
    .unwrap();
    std::fs::create_dir_all(home.bin_dir()).unwrap();
    Fixture { _t: t, home, repo }
}

fn run_launcher(f: &Fixture, current: &str, stdin: &str) -> (i32, String) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "role_launcher",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("FORGE_TEST_ROLE", current)
        .env("FORGE_HOME", f.home.cache_root())
        .env("CLAUDE_PROJECT_DIR", &f.repo)
        .env_remove(LAUNCHED_ENV)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn write_exe(p: &Path, body: &str) {
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn notes(f: &Fixture) -> Vec<String> {
    take_notes(&f.home, &RepoId::for_root(&f.repo).unwrap()).unwrap()
}

#[test]
fn same_version_runs_in_process() {
    let f = fixture("1.0.0");
    let (code, out) = run_launcher(&f, "1.0.0", "");
    assert_eq!(code, 0);
    assert!(
        out.contains("IN-PROCESS 1.0.0 notes=[] launched=None"),
        "{out}"
    );
}

#[test]
fn execs_locked_version_passing_argv_stdin_env_and_status() {
    let f = fixture("2.0.0");
    write_exe(
        &f.home.forge_binary("2.0.0").unwrap(),
        "#!/bin/sh\necho \"TARGET $FORGE_LAUNCHED_VERSION $*\"; cat; exit 7\n",
    );
    let (code, out) = run_launcher(&f, "1.0.0", "event-json");
    assert_eq!(code, 7, "{out}");
    // libtest's own header precedes the exec; the target's output ends it.
    assert!(
        out.ends_with(
            " TARGET 2.0.0 --exact role_launcher --nocapture --test-threads=1\nevent-json"
        ),
        "{out:?}"
    );
    assert!(notes(&f).is_empty());
}

#[test]
fn mislabelled_cache_binary_does_not_loop() {
    let f = fixture("2.0.0");
    // forge-2.0.0 is really another copy of the 1.0.0 launcher.
    std::os::unix::fs::symlink(
        std::env::current_exe().unwrap(),
        f.home.forge_binary("2.0.0").unwrap(),
    )
    .unwrap();
    let (code, out) = run_launcher(&f, "1.0.0", "");
    assert_eq!(code, 0);
    assert!(
        out.contains("IN-PROCESS 1.0.0 notes=[\"exec-version-mismatch\"] launched=Some(\"2.0.0\")"),
        "{out}"
    );
    assert_eq!(notes(&f).len(), 1);
}

#[test]
fn missing_version_runs_installed_and_leaves_note() {
    let f = fixture("3.0.0");
    let (code, out) = run_launcher(&f, "1.0.0", "");
    assert_eq!(code, 0);
    assert!(out.contains("notes=[\"version-unavailable\"]"), "{out}");
    let n = notes(&f);
    assert_eq!(n.len(), 1);
    assert!(
        n[0].contains("pins Forge 3.0.0") && n[0].contains("Forge 1.0.0 ran instead"),
        "{n:?}"
    );
}

#[test]
fn bad_lock_runs_installed_and_leaves_note() {
    let f = fixture("2.0");
    let (_, out) = run_launcher(&f, "1.0.0", "");
    assert!(out.contains("notes=[\"bad-lock\"]"), "{out}");
}
