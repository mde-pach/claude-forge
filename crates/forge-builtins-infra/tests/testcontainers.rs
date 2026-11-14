//! `forge_tests_docker` (§10.6): tests that use Testcontainers do not run, and fail saying why,
//! when no Docker daemon answers; other tests are not held to it. The probe stays fast.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::{Duration, Instant};

use forge_builtins_infra::{Options, run};
use forge_check::builtins::CheckCtx;
use forge_core::builtin::{BuiltinCtx, Scope};

fn call(root: &Path, lang: &str) -> (Vec<String>, Duration) {
    let ctx = BuiltinCtx {
        root: root.to_path_buf(),
        base: None,
        changed: vec![],
        rule: "python/pytest/run".into(),
        scope: Scope::Turn,
        accepted: vec![],
        notes_file: None,
    };
    let check = CheckCtx {
        cwd: root.to_path_buf(),
        ..CheckCtx::default()
    };
    let started = Instant::now();
    let r = run(
        &Options::default(),
        "tests_docker",
        &ctx,
        &check,
        &[lang.to_string()],
    )
    .expect("builtin");
    (
        r.findings.iter().map(|f| f.agent_text()).collect(),
        started.elapsed(),
    )
}

// one test: it sets DOCKER_HOST for the process
#[test]
fn tests_that_use_testcontainers_need_a_daemon_that_answers() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let sock_dir = tempfile::Builder::new()
        .prefix("tc")
        .tempdir_in("/tmp")
        .unwrap();
    let sock = sock_dir.path().join("d.sock");
    // SAFETY: the only test of this binary; nothing else reads the environment meanwhile
    unsafe { std::env::set_var("DOCKER_HOST", format!("unix://{}", sock.display())) };

    // no Testcontainers: nothing asked of Docker
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"x\"\nversion = \"0\"\n\n[dependency-groups]\ndev = [\"pytest>=8\"]\n",
    )
    .unwrap();
    assert!(call(&root, "python").0.is_empty());

    // Testcontainers and no daemon: the tests do not run, and the finding says what to do
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"x\"\nversion = \"0\"\n\n[dependency-groups]\ndev = [\"pytest>=8\", \"testcontainers[postgres]>=4.10\"]\n",
    )
    .unwrap();
    let (found, took) = call(&root, "python");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("The tests did not run: they use Testcontainers, which needs Docker, and no Docker daemon answers. Start it (`colima start`"),
        "{found:?}"
    );
    assert!(took < Duration::from_millis(500), "took {took:?}");
    // the npm side is its own: this package.json declares nothing
    std::fs::write(root.join("package.json"), "{\"name\":\"x\"}\n").unwrap();
    assert!(call(&root, "typescript").0.is_empty());

    // a daemon that answers: the tests run
    let l = UnixListener::bind(&sock).unwrap();
    let h = std::thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        let mut b = [0u8; 256];
        let _ = c.read(&mut b);
        let _ = c.write_all(b"HTTP/1.1 200 OK\r\nApi-Version: 1.47\r\nContent-Length: 2\r\n\r\nOK");
    });
    let (found, took) = call(&root, "python");
    assert!(found.is_empty(), "{found:?}");
    assert!(took < Duration::from_millis(500), "took {took:?}");
    h.join().unwrap();
}
