//! §15.3 The daemon and its protocol, and §15.1's failure rules, without type checkers:
//! hermetic (no ty, no TypeScript). The answers of real servers are tests/engine.rs.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use forge_core::NoteKind;
use forge_engine::{
    Client, EngineConfig, EngineError, Lang, Match, PythonConfig, TypeScriptConfig, Verdict,
    ensure, judge, serve, socket_addr,
};

fn up(config: &EngineConfig) -> Client {
    let c = config.clone();
    ensure(config, None, Duration::from_secs(10), move || {
        std::thread::spawn(move || {
            let _ = serve(c);
        });
        Ok(())
    })
    .unwrap()
}

fn gone(client: &Client, within: Duration) -> bool {
    let until = Instant::now() + within;
    while Instant::now() < until {
        if client.ping().is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn a_match(rule: &str) -> Match {
    serde_json::from_value(serde_json::json!({
        "file": "a.py", "rule": rule, "line": 3, "bindings": {}, "where": {}
    }))
    .unwrap()
}

fn broken_python(dir: &Path, sock: &str) -> EngineConfig {
    let mut config = EngineConfig::new(dir, dir.join(sock));
    config.python = Some(PythonConfig {
        ty: vec!["/nonexistent/ty".into()],
        ..PythonConfig::default()
    });
    config
}

#[test]
fn a_dependency_file_change_restarts_the_servers_and_a_source_change_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let client = up(&broken_python(dir.path(), "r.sock"));
    assert!(client.types(Lang::Python, &[a_match("p/x")]).is_err());
    assert!(client.ping().unwrap().failed.contains_key(&Lang::Python));
    client.changed(&["src/app.py".into()]).unwrap();
    assert!(
        client.ping().unwrap().failed.contains_key(&Lang::Python),
        "a source file keeps the servers"
    );
    for dep in [
        "uv.lock",
        "sub/poetry.lock",
        "pdm.lock",
        "Pipfile.lock",
        "pyproject.toml",
        "ty.toml",
        "package.json",
        "bun.lock",
        "bun.lockb",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "packages/a/tsconfig.build.json",
    ] {
        assert!(client.types(Lang::Python, &[a_match("p/x")]).is_err());
        client.changed(&[dep.into()]).unwrap();
        assert!(
            client.ping().unwrap().failed.is_empty(),
            "{dep} restarts the servers"
        );
    }
    client.stop().unwrap();
}

#[test]
fn one_daemon_per_socket() {
    let dir = tempfile::tempdir().unwrap();
    let config = EngineConfig::new(dir.path(), dir.path().join("o.sock"));
    let client = up(&config);
    let e = serve(config.clone()).unwrap_err();
    assert!(e.contains("already answers"), "{e}");
    // concurrent starters start one daemon
    client.stop().unwrap();
    let starts = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let config = config.clone();
            let starts = starts.clone();
            std::thread::spawn(move || {
                let c = config.clone();
                ensure(&config, Some("s"), Duration::from_secs(10), move || {
                    starts.fetch_add(1, Ordering::SeqCst);
                    std::thread::spawn(move || {
                        let _ = serve(c);
                    });
                    Ok(())
                })
                .map(|_| ())
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap().unwrap();
    }
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    Client::new(&config.socket).stop().unwrap();
}

#[test]
fn json_lines_several_requests_per_connection() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = EngineConfig::new(dir.path(), dir.path().join("j.sock"));
    config.want = "w".into();
    let client = up(&config);
    let s = UnixStream::connect(socket_addr(&config.socket)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut w = s.try_clone().unwrap();
    let mut r = BufReader::new(s);
    let mut ask = |line: &str| -> serde_json::Value {
        w.write_all(line.as_bytes()).unwrap();
        w.write_all(b"\n").unwrap();
        let mut out = String::new();
        r.read_line(&mut out).unwrap();
        serde_json::from_str(&out).unwrap()
    };
    let a = ask(r#"{"op": "register", "session": "s1"}"#);
    assert_eq!(a["status"]["sessions"], 1);
    let b = ask("not json");
    assert!(
        b["error"].as_str().unwrap().starts_with("unreadable request"),
        "{b}"
    );
    let c = ask(r#"{"op": "ping"}"#);
    assert_eq!(c["status"]["want"], "w");
    let d = ask(r#"{"op": "unregister", "session": "s1"}"#);
    assert_eq!(d["stopping"], true, "the last session out stops it");
    assert!(gone(&client, Duration::from_secs(5)));
}

#[test]
fn a_project_without_its_typescript_is_unchecked_never_passed() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = EngineConfig::new(dir.path(), dir.path().join("t.sock"));
    config.typescript = Some(TypeScriptConfig {
        js_runtime: None,
        preload: true,
    });
    let client = up(&config);
    let m = a_match("typescript/x");
    let answer = client.types(Lang::Typescript, std::slice::from_ref(&m));
    let Err(EngineError::Failed(why)) = &answer else {
        panic!("{answer:?}")
    };
    assert!(
        why.contains("the project has no typescript package"),
        "{why}"
    );
    let (keep, notes) = judge(Lang::Typescript, &[m], &answer, false);
    assert_eq!(keep, [false]);
    assert_eq!(notes[0].kind, NoteKind::ForgeFailure);
    assert_eq!(notes[0].rule.as_deref(), Some("typescript/x"));
    assert!(
        notes[0]
            .text
            .starts_with("not checked: the typescript types engine:"),
        "{}",
        notes[0].text
    );
    client.stop().unwrap();
}

#[test]
fn judge_drops_what_it_cannot_confirm_and_says_so() {
    let (a, b) = (a_match("p/a"), a_match("p/b"));
    let ms = [a.clone(), b.clone()];
    // holds keeps, fails drops, unchecked drops with a not-enforced note for its rule
    let ok = Ok(vec![
        Verdict::Holds,
        Verdict::Unchecked {
            why: "a.py:3: $X is bound to `x`, which is not an expression: its type cannot be asked"
                .into(),
        },
    ]);
    let (keep, notes) = judge(Lang::Python, &ms, &ok, false);
    assert_eq!(keep, [true, false]);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].kind, NoteKind::NotEnforced);
    assert_eq!(notes[0].rule.as_deref(), Some("p/b"));
    assert!(notes[0].text.starts_with("not checked: a.py:3: $X is bound to"));
    // still loading: silent after an edit, a note at Stop (one per rule)
    let (keep, notes) = judge(Lang::Python, &ms, &Err(EngineError::Loading), true);
    assert_eq!((keep, notes.len()), (vec![false, false], 0));
    let (_, notes) = judge(Lang::Python, &ms, &Err(EngineError::Loading), false);
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|n| n.kind == NoteKind::ForgeFailure));
    // an answer of the wrong length is a failure, not a pass
    let (keep, notes) = judge(Lang::Python, &ms, &Ok(vec![Verdict::Holds]), false);
    assert_eq!(keep, [false, false]);
    assert!(notes[0].text.contains("incomplete answer"));
}

#[test]
fn the_daemon_exits_when_its_repository_disappears() {
    let dir = tempfile::tempdir().unwrap();
    let root: PathBuf = dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let config = EngineConfig::new(&root, dir.path().join("g.sock"));
    let client = up(&config);
    client.register("s").unwrap();
    std::fs::remove_dir(&root).unwrap();
    // checked every 10 seconds, requests or not
    assert!(
        gone(&client, Duration::from_secs(15)),
        "still answering after its repository went away"
    );
}

/// The `where` schema (§15.1): rules are rejected when they are loaded.
#[test]
fn where_conditions_are_checked_when_rules_load() {
    let base = "id: local/q\ndescription: No q.\nscope: {language: LANG}\nenforce:\n  - target: ast-grep\n    mode: MODE\n    rule: {pattern: \"$S.q()\"}\n    WHERE\nexamples:\n  match: [\"a.q()\"]\n  no_match: [\"q()\"]\n";
    let load = |lang: &str, mode: &str, where_: &str| -> Result<(), String> {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join(".forge/rules");
        std::fs::create_dir_all(&dir).unwrap();
        let text = base
            .replace("LANG", lang)
            .replace("MODE", mode)
            .replace("WHERE", where_);
        std::fs::write(dir.join("q.yaml"), text).unwrap();
        forge_packs::load(Some(t.path()), forge_packs::Options { cache_dir: None })
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    let ty = "where: {S: {type: {package: sqlalchemy, name: Session}}}";
    load("python", "forbid", ty).unwrap();
    load("python", "advise", ty).unwrap();
    load(
        "python",
        "forbid",
        "where: {S: {declared: [{module: os, name: system}, {package: subprocess}]}}",
    )
    .unwrap();
    load(
        "python",
        "forbid",
        "where: {S: {not: {type: {module: sqlalchemy.orm}}}}",
    )
    .unwrap();
    load("typescript", "forbid", "where: {S: {deprecated: true}}").unwrap();
    let rejected = [
        ("python", "no-increase", ty, "forbid or advise"),
        ("python", "forbid", "where: {S: {type: {name: Session}}}", ""),
        ("python", "forbid", "where: {S: {deprecated: true}}", "TypeScript only"),
        ("typescript", "forbid", "where: {S: {deprecated: false}}", ""),
        (
            "python",
            "forbid",
            "needs: [{ecosystem: pypi, name: django-stubs}]",
            "needs requires where",
        ),
    ];
    for (lang, mode, w, why) in rejected {
        let e = load(lang, mode, w).expect_err(w);
        assert!(e.contains(why), "{w}: {e}");
    }
}
