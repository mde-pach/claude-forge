//! `forge check` end to end, in temporary git repositories: the packs' check examples (each in
//! a repository of its own, as a turn that started at its `head`), forge_do through a fake tool,
//! and what the agent reads.
//!
//! Project tools are left out of PATH, so a check whose tool is missing is skipped, and a skip is
//! read from the run's notes (structured), never from its text.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use forge_packs::model::{Example, Rule, Stage, Target};
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

#[test]
fn unregistered_builtins_are_notes() {
    let r = Repo::new(&files(&[("a.txt", "a\n")]));
    write(
        r.path(),
        &files(&[
            ("a.txt", "b\n"),
            (
                ".forge/rules/y.yaml",
                "id: local/y\ndescription: Something.\nscope: {language: any}\nenforce:\n  - {target: check, stage: lint, run: \"forge_hadolint DL3020\"}\n",
            ),
        ]),
    );
    let (code, v) = r.turn(&["local/y"]);
    // a builtin no crate registered yet: a note, never a failure
    assert_eq!(code, 0, "{v}");
    assert!(
        notes_of(&v, "local/y")
            .iter()
            .any(|n| n["kind"] == "not-enforced"),
        "{v}"
    );
}

/// Every check rule example the machinery here can run behaves as declared: a `match` example
/// fails its check (or gives its advice), a `no_match` example passes. Examples whose tool or
/// builtin is not available are skipped (by the run's notes).
#[test]
fn check_examples() {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).unwrap();
    let mut runs = Vec::new();
    for r in set.packs.values().flat_map(|p| p.rules.iter()) {
        let Some(t) = r.enforce.iter().find_map(|t| match t {
            Target::Check(c) => Some(c.clone()),
            _ => None,
        }) else {
            continue;
        };
        for (kind, list) in [
            ("match", r.examples.as_ref().and_then(|e| e.match_.clone())),
            (
                "no_match",
                r.examples.as_ref().and_then(|e| e.no_match.clone()),
            ),
        ] {
            for (i, ex) in list.unwrap_or_default().into_iter().enumerate() {
                runs.push((r.clone(), t.stage, kind, i + 1, ex));
            }
        }
    }
    let results = Mutex::new(Vec::new());
    let queue = Mutex::new(runs);
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                loop {
                    let Some((rule, stage, kind, n, ex)) = queue.lock().unwrap().pop() else {
                        break;
                    };
                    let res = run_example(&rule, stage, kind, &ex);
                    results
                        .lock()
                        .unwrap()
                        .push((rule.id.clone(), kind, n, res));
                }
            });
        }
    });
    let results = results.into_inner().unwrap();
    let mut pass = 0;
    let mut skip = 0;
    let mut fail = Vec::new();
    let mut leaks = Vec::new();
    for (id, kind, n, res) in &results {
        match res {
            Res::Pass(texts) => {
                pass += 1;
                for t in texts {
                    if !forge_core::internals::find(t).is_empty() {
                        leaks.push(format!("{id}: {t}"));
                    }
                }
            }
            Res::Skip => skip += 1,
            Res::Fail(why) => fail.push(format!("{id}: {kind} example {n}: {why}")),
        }
    }
    eprintln!(
        "check examples: {} run, {pass} as declared, {skip} skipped (tool or builtin not available), {} failed",
        results.len(),
        fail.len()
    );
    fail.sort();
    assert!(fail.is_empty(), "{}", fail.join("\n"));
    assert!(
        leaks.is_empty(),
        "agent text names internals:\n{}",
        leaks.join("\n")
    );
    assert!(pass > 20, "only {pass} examples ran");
}

enum Res {
    Pass(Vec<String>),
    Skip,
    Fail(String),
}

/// `examplePath`: a concrete path inside the rule's scope.
fn example_path(rule: &Rule) -> String {
    use forge_packs::model::Language;
    let ext = match rule.scope.language {
        Language::Python => ".py",
        Language::Typescript => ".ts",
        Language::Tsx => ".tsx",
        Language::Yaml => ".yaml",
        Language::Any => ".txt",
    };
    let stem = "example";
    let roles = forge_check::scope::Roles::defaults();
    let globs = roles.expand(rule.scope.files.as_deref().unwrap_or(&[]));
    let Some(g) = globs.iter().find(|g| !g.starts_with('!')) else {
        return format!("{stem}{ext}");
    };
    let re = |p: &str| regex::Regex::new(p).unwrap();
    let mut p = re(r"\{([^,}]*)[^}]*\}").replace_all(g, "$1").into_owned();
    p = re(r"\.\*$").replace(&p, ext).into_owned();
    p = re(r"(^|/)\*\*/").replace_all(&p, "$1").into_owned();
    p = p.replace("**", stem).replace('*', stem).replace('?', "x");
    p = re(r"\[(!?)(.)[^\]]*\]")
        .replace_all(&p, |c: &regex::Captures| {
            if &c[1] == "!" {
                "x".to_string()
            } else {
                c[2].to_string()
            }
        })
        .into_owned();
    if p.ends_with('/') {
        p.push_str(stem);
    }
    let base = p.rsplit('/').next().unwrap_or(&p).to_string();
    if !re(r"\.[A-Za-z0-9]+$").is_match(&base) {
        p.push_str(ext);
    }
    p
}

fn run_example(rule: &Rule, stage: Stage, kind: &str, ex: &Example) -> Res {
    let (head, changed) = match ex {
        Example::Code(code) => (BTreeMap::new(), files(&[(&example_path(rule), code)])),
        Example::Fixture(f) => (
            f.head
                .clone()
                .map(|h| h.into_iter().collect())
                .unwrap_or_default(),
            f.files.clone().into_iter().collect(),
        ),
        Example::Command(_) => return Res::Skip,
    };
    let repo = Repo::new(&head);
    write(repo.path(), &changed);
    git(repo.path(), &["add", "-A"]);
    let (_, v) = if stage == Stage::Format {
        repo.check(&["--all"], &[&rule.id], PATH)
    } else {
        repo.turn(&[&rule.id])
    };
    let found = findings_of(&v, &rule.id);
    let texts: Vec<String> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["agent_text"].as_str().unwrap_or("").to_string())
        .collect();
    let skipped = notes_of(&v, &rule.id).iter().any(|n| {
        matches!(
            n["kind"].as_str(),
            Some("missing-tool" | "not-enforced" | "forge-failure" | "environment")
        )
    });
    match kind {
        "match" if !found.is_empty() => Res::Pass(texts),
        "match" if skipped => Res::Skip,
        "match" => Res::Fail(format!("did not fail the check: {v}")),
        _ if found.is_empty() => Res::Pass(texts),
        _ if skipped => Res::Skip,
        _ => Res::Fail(format!("failed the check: {v}")),
    }
}
