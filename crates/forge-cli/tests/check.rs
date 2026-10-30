//! `forge check` end to end, in temporary git repositories: the packs' check examples (each in
//! a repository of its own, as a turn that started at its `head`), forge_do through a fake tool,
//! and what the agent reads.
//!
//! Project tools are left out of PATH, so a check whose tool is missing is skipped, and a skip is
//! read from the run's notes (structured), never from its text.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// The forge under test (`FORGE_TEST_EXE` overrides it: a copy, when the target dir is shared).
fn exe() -> String {
    std::env::var("FORGE_TEST_EXE").unwrap_or_else(|_| env!("CARGO_BIN_EXE_forge").to_string())
}
const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=forge",
            "-c",
            "user.email=forge@localhost",
            "-c",
            "maintenance.auto=false",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git");
    assert!(
        st.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&st.stderr)
    );
}

fn write(dir: &Path, files: &BTreeMap<String, String>) {
    for (p, t) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, t).unwrap();
    }
}

/// A repository holding `head` (committed) and a turn snapshot of that head, outside it.
struct Repo {
    dir: tempfile::TempDir,
    snap: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Repo {
    fn new(head: &BTreeMap<String, String>) -> Repo {
        let dir = tempfile::tempdir().unwrap();
        let snap = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        let mut head = head.clone();
        head.entry(".gitignore".into())
            .or_insert_with(|| "node_modules/\n.venv/\n".into());
        write(dir.path(), &head);
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-qm", "head", "--allow-empty"]);
        let rev = Command::new("git")
            .current_dir(dir.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        std::fs::write(snap.path().join("head"), &rev.stdout).unwrap();
        std::fs::write(snap.path().join("dirty"), "").unwrap();
        Repo { dir, snap, home }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn check(&self, mode: &[&str], rules: &[&str], path: &str) -> (i32, Value) {
        let mut args: Vec<String> = vec!["check".into(), "--json".into()];
        args.extend(mode.iter().map(|s| s.to_string()));
        if !rules.is_empty() {
            args.push(format!("--rules={}", rules.join(",")));
        }
        let out = Command::new(exe())
            .current_dir(self.path())
            .args(&args)
            .env("PATH", path)
            .env("FORGE_HOME", self.home.path())
            .env("FORGE_NO_DOWNLOAD", "1")
            .env_remove("VIRTUAL_ENV")
            .env_remove("CLAUDECODE")
            .env_remove("DATABASE_URL")
            .output()
            .expect("forge check");
        let code = out.status.code().unwrap_or(-1);
        let v: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "{e}: {}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        });
        (code, v)
    }

    fn turn(&self, rules: &[&str]) -> (i32, Value) {
        let snap = self.snap.path().display().to_string();
        self.check(&["--turn", &snap], rules, PATH)
    }
}

fn findings_of<'a>(v: &'a Value, rule: &str) -> Vec<&'a Value> {
    v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == rule)
        .collect()
}

fn notes_of<'a>(v: &'a Value, rule: &str) -> Vec<&'a Value> {
    v["notes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["rule"] == rule)
        .collect()
}

fn files(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn forbid_on_added_lines_only() {
    let swallow = "import asyncio\n\n\nasync def a(q):\n    try:\n        await q.get()\n    except asyncio.CancelledError:\n        pass\n";
    let r = Repo::new(&files(&[("app.py", swallow)]));
    let twice = format!(
        "{swallow}\n\n{}",
        swallow
            .replace("def a", "def b")
            .replacen("import asyncio\n\n\n", "", 1)
    );
    write(r.path(), &files(&[("app.py", &twice)]));
    let id = "python/no-swallowed-cancel";
    let (code, v) = r.check(&["--changed"], &[id], PATH);
    assert_eq!(code, 1, "{v}");
    let f = findings_of(&v, id);
    assert_eq!(f.len(), 1, "{v}");
    let text = f[0]["agent_text"].as_str().unwrap();
    assert!(text.starts_with("app.py:14 An except handler"), "{text}");
    // the whole project: both
    let (code, v) = r.check(&["--all"], &[id], PATH);
    assert_eq!(code, 1);
    assert_eq!(findings_of(&v, id).len(), 2, "{v}");
}

#[test]
fn deleted_tests_and_counts() {
    let r = Repo::new(&files(&[
        ("tests/test_a.py", "def test_a():\n    assert True\n"),
        ("a.py", "x = 1\n"),
    ]));
    std::fs::remove_file(r.path().join("tests/test_a.py")).unwrap();
    let (code, v) = r.turn(&["python/no-eval"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(findings_of(&v, "forge/deleted-test").len(), 1, "{v}");
}

#[test]
fn forge_do_drives_a_fake_tool_and_notes_what_it_cannot() {
    // a project ruff that says its version and reports one finding per file
    let ruff = "#!/bin/sh\ncase \"$1\" in --version) echo 'ruff 0.6.9'; exit 0 ;; esac\nfor a; do case $a in *.py) echo \"$a:1:1: F401 \\`os\\` imported but unused\"; st=1 ;; esac; done\nexit ${st:-0}\n";
    let r = Repo::new(&files(&[("a.py", "x = 1\n")]));
    write(
        r.path(),
        &files(&[(".venv/bin/ruff", ruff), ("a.py", "import os\n")]),
    );
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        r.path().join(".venv/bin/ruff"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let (code, v) = r.turn(&["python/lint"]);
    assert_eq!(code, 1, "{v}");
    let f = findings_of(&v, "python/lint");
    assert!(!f.is_empty(), "{v}");
    assert!(
        f[0]["agent_text"].as_str().unwrap().starts_with("a.py:1 "),
        "{v}"
    );
    // a version no range covers: not enforced, a note, and the check passes
    let old = ruff.replace("0.6.9", "0.0.1");
    write(r.path(), &files(&[(".venv/bin/ruff", &old)]));
    let (code, v) = r.turn(&["python/lint"]);
    assert_eq!(code, 0, "{v}");
    assert!(findings_of(&v, "python/lint").is_empty());
    let notes = notes_of(&v, "python/lint");
    assert!(notes.iter().any(|n| n["kind"] == "not-enforced"), "{v}");
}

/// Every built-in rule example behaves as declared, through `forge test --builtin` (the guard,
/// the ast-grep and grep patterns, the check stages on fixtures): a `match` example is caught, a
/// `no_match` example passes. Examples whose tool or builtin is not available are skipped, as
/// the results' structure says.
#[test]
fn check_examples() {
    let home = tempfile::tempdir().unwrap();
    let out = Command::new(exe())
        .args(["test", "--builtin", "--json"])
        .current_dir(home.path())
        .env("PATH", PATH)
        .env("FORGE_HOME", home.path())
        .env("FORGE_NO_DOWNLOAD", "1")
        .env_remove("VIRTUAL_ENV")
        .env_remove("CLAUDECODE")
        .env_remove("DATABASE_URL")
        .output()
        .expect("forge test");
    let lines: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let (summary, results) = lines.split_last().expect("a summary");
    let mut fail = Vec::new();
    let mut leaks = Vec::new();
    let mut checks_passed = 0;
    for r in results {
        let label = format!("{} {} {} {}", r["rule"], r["target"], r["kind"], r["index"]);
        match r["verdict"].as_str() {
            Some("fail") => fail.push(format!("{label}: {}", r["why"])),
            Some("pass") if r["target"] == "check" => checks_passed += 1,
            Some("skip") => assert!(r["note"]["kind"].is_string(), "{r}"),
            _ => {}
        }
        for t in r["agent_texts"].as_array().into_iter().flatten() {
            let t = t.as_str().unwrap();
            if !forge_core::internals::find(t).is_empty() {
                leaks.push(format!("{label}: {t}"));
            }
        }
    }
    eprintln!("forge test --builtin: {summary}");
    assert!(fail.is_empty(), "{}", fail.join("\n"));
    assert_eq!(out.status.code(), Some(0), "{summary}");
    assert!(
        leaks.is_empty(),
        "agent text names internals:\n{}",
        leaks.join("\n")
    );
    assert!(summary["passed"].as_u64().unwrap() > 2000, "{summary}");
    assert!(
        checks_passed > 20,
        "only {checks_passed} check examples ran"
    );
}

/// §10.6: failing tests are re-run in the tree as it was at the start of the turn; only new
/// failures fail the check, old ones are "unverified" (exit 3): a note for the person, no
/// finding.
#[test]
fn failing_tests_compared_with_the_start_of_the_turn() {
    let pytest = "#!/bin/sh\ncase \"$1\" in --version) echo 'pytest 8.3.0'; exit 0 ;; esac\necho 'E   OSError: connect ECONNREFUSED 127.0.0.1:5432'\necho 'FAILED tests/test_a.py::test_old - OSError'\nif grep -q BROKEN a.py; then echo 'FAILED tests/test_a.py::test_new - assert 1 == 2'; fi\nexit 1\n";
    let head = files(&[
        ("a.py", "x = 1\n"),
        ("tests/test_a.py", "def test_old():\n    pass\n"),
        (".venv/bin/pytest", pytest),
    ]);
    let r = Repo::new(&head);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        r.path().join(".venv/bin/pytest"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let id = "python/pytest/run";
    write(r.path(), &files(&[("a.py", "x = 2\n")]));
    let (code, v) = r.turn(&[id]);
    assert_eq!(code, 3, "{v}");
    assert!(
        v["unverified"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u.as_str().unwrap().contains("test_old")),
        "{v}"
    );
    assert!(findings_of(&v, id).is_empty(), "{v}");
    assert!(
        v["notes"].as_array().unwrap().iter().any(|n| {
            let t = n["text"].as_str().unwrap();
            n["rule"] == id
                && t.contains("tests/test_a.py::test_old")
                && t.contains("Postgres refused the connection")
        }),
        "{v}"
    );
    write(r.path(), &files(&[("a.py", "x = 2  # BROKEN\n")]));
    let (code, v) = r.turn(&[id]);
    assert_eq!(code, 1, "{v}");
    let m = findings_of(&v, id)[0]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        m.starts_with(
            "1 failing test(s) passed at the start of the turn:\n  tests/test_a.py::test_new"
        ),
        "{m}"
    );
}

/// Advice (§10.7) is given once per turn and never fails the run alone (exit 4).
#[test]
fn advice_once_per_turn() {
    let r = Repo::new(&files(&[("app/a.py", "def f():\n    return 1\n")]));
    write(
        r.path(),
        &files(&[("app/a.py", "def f():\n    return 2\n")]),
    );
    let id = "python/pytest/untested-change";
    let (code, v) = r.turn(&[id]);
    assert_eq!(code, 4, "{v}");
    let f = findings_of(&v, id);
    assert_eq!(f[0]["severity"], "advice", "{v}");
    assert!(
        f[0]["agent_text"]
            .as_str()
            .unwrap()
            .starts_with("Advice: You changed behaviour in app/a.py"),
        "{v}"
    );
    let (code, v) = r.turn(&[id]);
    assert_eq!(code, 0, "reported once: {v}");
}
