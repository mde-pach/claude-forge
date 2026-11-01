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
    let r = Repo::new(&files(&[
        ("a.py", "x = 1\n"),
        ("ruff.toml", "line-length = 100\n"),
    ]));
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

/// Forge's pinned ty runs with the levels the active `tool: ty` rules need (`--error <rule>`)
/// where the project leaves ty to its defaults; a project's own ty config is left to decide.
#[test]
fn ty_runs_at_the_active_rules_levels_unless_the_project_configures_it() {
    // a ty that reports redundant-cast as a warning unless told it is an error
    let ty = "#!/bin/sh\ncase \"$1\" in --version) echo 'ty 0.0.86'; exit 0 ;; esac\ngrep -q 'cast(' a.py || exit 0\ncase \" $* \" in *' --error redundant-cast '*) echo 'a.py:1:5: error[redundant-cast] Value is already of type `int`'; exit 1 ;; esac\necho 'a.py:1:5: warning[redundant-cast] Value is already of type `int`'; exit 0\n";
    let r = Repo::new(&files(&[("a.py", "x = 1\n")]));
    let bin = tempfile::tempdir().unwrap();
    write(bin.path(), &files(&[("ty", ty)]));
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        bin.path().join("ty"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let path = format!("{}:{PATH}", bin.path().display());
    let snap = r.snap.path().display().to_string();
    write(r.path(), &files(&[("a.py", "x = cast(int, 1)\n")]));
    let rules = ["python/typecheck", "python/no-useless-cast"];
    let (code, v) = r.check(&["--turn", &snap], &rules, &path);
    assert_eq!(code, 1, "{v}");
    assert!(
        findings_of(&v, "python/typecheck")[0]["agent_text"]
            .as_str()
            .unwrap()
            .contains("error[redundant-cast]"),
        "{v}"
    );
    // [tool.ty]: the project's config decides (a warning there passes)
    write(
        r.path(),
        &files(&[(
            "pyproject.toml",
            "[project]\nname = \"x\"\n\n[tool.ty.rules]\n",
        )]),
    );
    let (code, v) = r.check(&["--turn", &snap], &rules, &path);
    assert_eq!(code, 0, "{v}");
}

/// Where the project does not configure ruff, `forge_do ruff` runs Forge's ruff with Forge's
/// config (ruff's classic rules and the active `tool: ruff` rules' snippets, as `--config`
/// overrides); where it does, the project's ruff and config decide.
#[test]
fn ruff_selects_the_active_rules_codes_unless_the_project_configures_it() {
    // Forge's ruff (found on PATH, downloads being off) reports S307 for eval( only when the
    // command line selects it; the project's own reports nothing
    let ruff = "#!/bin/sh\ncase \"$1\" in --version) echo 'ruff 0.17.1'; exit 0 ;; esac\ncase \" $* \" in *'\"S307\"'*) grep -q 'eval(' a.py && { echo 'a.py:1:5: S307 Use of possibly insecure function'; exit 1; } ;; esac\nexit 0\n";
    let quiet = "#!/bin/sh\ncase \"$1\" in --version) echo 'ruff 0.17.1'; exit 0 ;; esac\nexit 0\n";
    let r = Repo::new(&files(&[("a.py", "x = 1\n")]));
    let bin = tempfile::tempdir().unwrap();
    write(bin.path(), &files(&[("ruff", ruff)]));
    write(r.path(), &files(&[(".venv/bin/ruff", quiet)]));
    use std::os::unix::fs::PermissionsExt;
    for f in [bin.path().join("ruff"), r.path().join(".venv/bin/ruff")] {
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{PATH}", bin.path().display());
    let snap = r.snap.path().display().to_string();
    write(r.path(), &files(&[("a.py", "x = eval('1')\n")]));
    let rules = ["python/lint", "python/no-eval"];
    let (code, v) = r.check(&["--turn", &snap], &rules, &path);
    assert_eq!(code, 1, "{v}");
    assert!(
        findings_of(&v, "python/lint")[0]["agent_text"]
            .as_str()
            .unwrap()
            .contains("S307"),
        "{v}"
    );
    // [tool.ruff]: the project's config decides (it selects nothing more here)
    write(
        r.path(),
        &files(&[(
            "pyproject.toml",
            "[project]\nname = \"x\"\n\n[tool.ruff]\nline-length = 100\n",
        )]),
    );
    let (code, v) = r.check(&["--turn", &snap], &rules, &path);
    assert_eq!(code, 0, "{v}");
}

/// Forge's own ruff (the project does not configure ruff) lints but never formats: the format
/// check does not run it. The project's ruff config turns formatting on.
#[test]
fn forges_own_ruff_does_not_format() {
    // any ruff here reports every file unformatted, and logs what it was asked
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("log");
    let ruff = format!(
        "#!/bin/sh\ncase \"$1\" in --version) echo 'ruff 0.17.1'; exit 0 ;; esac\necho \"$*\" >> '{}'\ncase \"$1\" in format) echo 'Would reformat: a.py'; exit 1 ;; esac\nexit 0\n",
        log.display()
    );
    let r = Repo::new(&files(&[("a.py", "x = 1\n")]));
    write(bin.path(), &files(&[("ruff", &ruff)]));
    write(r.path(), &files(&[(".venv/bin/ruff", &ruff)]));
    use std::os::unix::fs::PermissionsExt;
    for f in [bin.path().join("ruff"), r.path().join(".venv/bin/ruff")] {
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{PATH}", bin.path().display());
    let snap = r.snap.path().display().to_string();
    write(r.path(), &files(&[("a.py", "x=2\n")]));
    let rules = ["python/format", "python/lint"];
    let (code, v) = r.check(&["--turn", &snap], &rules, &path);
    assert_eq!(code, 0, "{v}");
    let asked = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!asked.lines().any(|l| l.starts_with("format")), "{asked}");
    assert!(
        asked.lines().any(|l| l.starts_with("check")),
        "it lints: {asked}"
    );
    assert!(
        notes_of(&v, "python/format").iter().any(|n| n["text"]
            .as_str()
            .unwrap()
            .starts_with("not run (ruff format")),
        "{v}"
    );
    // the project's config: its ruff formats
    write(r.path(), &files(&[("ruff.toml", "line-length = 100\n")]));
    let (code, v) = r.check(&["--turn", &snap], &rules, &path);
    assert_eq!(code, 1, "{v}");
    assert!(!findings_of(&v, "python/format").is_empty(), "{v}");
}

/// uv's cooldown in a change: what the change broke is a finding; a setting the project already
/// lacked is not the agent's to fix (forge apply offers it), even when the change touches the
/// lock; the whole project (`--all`) reports it.
#[test]
fn the_uv_cooldown_blocks_only_what_the_change_broke() {
    let rule = ["python/uv/exclude-newer"];
    let bare = "[project]\nname = \"x\"\nversion = \"0\"\nrequires-python = \">=3.12\"\n";
    let r = Repo::new(&files(&[("pyproject.toml", bare)]));
    write(r.path(), &files(&[("uv.lock", "version = 1\n")]));
    let (code, v) = r.turn(&rule);
    assert_eq!(code, 0, "{v}");
    assert!(findings_of(&v, rule[0]).is_empty(), "{v}");
    let (code, v) = r.check(&["--all"], &rule, PATH);
    assert_eq!(code, 1, "{v}");
    assert_eq!(findings_of(&v, rule[0]).len(), 2, "{v}");
    // a change that drops the cooldown the project had
    let good =
        format!("{bare}\n[tool.uv]\nexclude-newer = \"7 days\"\nrequired-version = \">=0.9.17\"\n");
    let r = Repo::new(&files(&[("pyproject.toml", &good)]));
    write(
        r.path(),
        &files(&[(
            "pyproject.toml",
            &format!("{bare}\n[tool.uv]\nrequired-version = \">=0.9.17\"\n"),
        )]),
    );
    let (code, v) = r.turn(&rule);
    assert_eq!(code, 1, "{v}");
    let f = findings_of(&v, rule[0]);
    assert_eq!(f.len(), 1, "{v}");
    assert!(
        f[0]["agent_text"]
            .as_str()
            .unwrap()
            .contains("exclude-newer is unset"),
        "{v}"
    );
}

/// A pyproject.toml the agent's turn creates gets uv's cooldown from Forge, as advice (the keys
/// are mechanical); a commit that creates one without it is still refused.
#[test]
fn the_uv_cooldown_is_added_to_a_created_pyproject() {
    let rule = ["python/uv/exclude-newer"];
    let bare = "[project]\nname = \"x\"\nversion = \"0\"\nrequires-python = \">=3.12\"\n";
    let r = Repo::new(&files(&[("main.py", "print(1)\n")]));
    write(
        r.path(),
        &files(&[("pyproject.toml", bare), ("uv.lock", "version = 1\n")]),
    );
    let (code, v) = r.turn(&rule);
    assert_eq!(code, 4, "advice only: {v}");
    let f = findings_of(&v, rule[0]);
    assert_eq!(f.len(), 1, "{v}");
    assert_eq!(f[0]["severity"], "advice", "{v}");
    assert_eq!(
        f[0]["message"], "pyproject.toml: added [tool.uv] exclude-newer and required-version.",
        "{v}"
    );
    assert_eq!(
        std::fs::read_to_string(r.path().join("pyproject.toml")).unwrap(),
        format!("{bare}\n[tool.uv]\nexclude-newer = \"7 days\"\nrequired-version = \">=0.9.17\"\n")
    );
    // at commit, nothing is written: the missing settings are findings
    let r = Repo::new(&files(&[("main.py", "print(1)\n")]));
    write(
        r.path(),
        &files(&[("pyproject.toml", bare), ("uv.lock", "version = 1\n")]),
    );
    git(r.path(), &["add", "-A"]);
    let (code, v) = r.check(&["--changed"], &rule, PATH);
    assert_eq!(code, 1, "{v}");
    assert_eq!(findings_of(&v, rule[0]).len(), 2, "{v}");
    assert_eq!(
        std::fs::read_to_string(r.path().join("pyproject.toml")).unwrap(),
        bare
    );
}

/// python/pip/pinned-requirements follows the file's own convention: an unpinned entry added to
/// a file that pins every entry (or to a new file) is a finding; a file that writes ranges keeps
/// writing them.
#[test]
fn requirements_keep_their_own_convention() {
    let rule = ["python/pip/pinned-requirements"];
    let ranged = "django>=5.0\nrequests\n";
    let r = Repo::new(&files(&[("requirements.txt", ranged)]));
    write(
        r.path(),
        &files(&[("requirements.txt", &format!("{ranged}httpx>=0.27\n"))]),
    );
    let (code, v) = r.turn(&rule);
    assert_eq!(code, 0, "{v}");
    assert!(findings_of(&v, rule[0]).is_empty(), "{v}");
    let pinned = "django==5.1.4\nrequests==2.32.5\n";
    let r = Repo::new(&files(&[("requirements.txt", pinned)]));
    write(
        r.path(),
        &files(&[("requirements.txt", &format!("{pinned}httpx>=0.27\n"))]),
    );
    let (code, v) = r.turn(&rule);
    assert_eq!(code, 1, "{v}");
    let f = findings_of(&v, rule[0]);
    assert_eq!(f.len(), 1, "{v}");
    assert_eq!(f[0]["path"], "requirements.txt", "{v}");
    assert_eq!(f[0]["line"], 3, "{v}");
    assert_eq!(
        f[0]["message"],
        "`httpx>=0.27` has no exact version: pin it (`httpx==<version>`), as the file's other entries are.",
        "{v}"
    );
    // the whole project has no change to hold to a convention
    let (code, v) = r.check(&["--all"], &rule, PATH);
    assert_eq!(code, 0, "{v}");
}

/// uv.lock is kept in sync rather than reported: a stale lock is relocked (advice in the turn; at
/// commit, a refusal to commit again with it; in CI, a finding and nothing written); a lock the
/// project never had is not the change's; a pyproject.toml that does not resolve is a finding.
#[test]
fn the_uv_lock_is_kept_in_sync() {
    let uv = "#!/bin/sh\ncase \"$1\" in --version) echo 'uv 0.13.1'; exit 0 ;; esac\nif [ \"$1 $2\" = 'lock --check' ]; then grep -q synced uv.lock 2>/dev/null && exit 0; echo 'error: The lockfile at `uv.lock` needs to be updated' >&2; exit 1; fi\nif [ \"$1\" = lock ]; then [ -f FAIL ] && { echo 'error: No solution found when resolving dependencies' >&2; exit 1; }; echo synced > uv.lock; exit 0; fi\nexit 0\n";
    let bin = tempfile::tempdir().unwrap();
    write(bin.path(), &files(&[("uv", uv)]));
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        bin.path().join("uv"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let path = format!("{}:{PATH}", bin.path().display());
    let rule = ["python/uv/lock-sync"];
    let py = "[project]\nname = \"x\"\nversion = \"0\"\nrequires-python = \">=3.12\"\ndependencies = []\n";
    let edited = py.replace("[]", "[\"httpx\"]");
    // no lock, before or after: setup, not the change's
    let r = Repo::new(&files(&[("pyproject.toml", py)]));
    write(r.path(), &files(&[("pyproject.toml", &edited)]));
    let snap = r.snap.path().display().to_string();
    let (code, v) = r.check(&["--turn", &snap], &rule, &path);
    assert_eq!(code, 0, "{v}");
    assert!(findings_of(&v, rule[0]).is_empty(), "{v}");
    assert!(!r.path().join("uv.lock").exists());
    // CI: the missing lock is reported
    let (code, v) = r.check(&["--all"], &rule, &path);
    assert_eq!(code, 1, "{v}");
    // a stale lock in the turn: relocked, advice
    let r = Repo::new(&files(&[("pyproject.toml", py), ("uv.lock", "synced\n")]));
    write(
        r.path(),
        &files(&[("pyproject.toml", &edited), ("uv.lock", "old\n")]),
    );
    let snap = r.snap.path().display().to_string();
    let (code, v) = r.check(&["--turn", &snap], &rule, &path);
    assert_eq!(code, 4, "advice only: {v}");
    let f = findings_of(&v, rule[0]);
    assert_eq!(f.len(), 1, "{v}");
    assert_eq!(f[0]["severity"], "advice", "{v}");
    assert_eq!(
        std::fs::read_to_string(r.path().join("uv.lock")).unwrap(),
        "synced\n"
    );
    // at commit: relocked, and the commit is refused once
    write(r.path(), &files(&[("uv.lock", "old\n")]));
    let (code, v) = r.check(&["--changed"], &rule, &path);
    assert_eq!(code, 1, "{v}");
    assert!(
        findings_of(&v, rule[0])[0]["agent_text"]
            .as_str()
            .unwrap()
            .contains("Add it and commit again"),
        "{v}"
    );
    // CI never writes
    write(r.path(), &files(&[("uv.lock", "old\n")]));
    let (code, v) = r.check(&["--all"], &rule, &path);
    assert_eq!(code, 1, "{v}");
    assert_eq!(
        std::fs::read_to_string(r.path().join("uv.lock")).unwrap(),
        "old\n"
    );
    // a pyproject.toml that does not resolve
    write(r.path(), &files(&[("FAIL", "")]));
    let (code, v) = r.check(&["--turn", &snap], &rule, &path);
    assert_eq!(code, 1, "{v}");
    assert!(
        findings_of(&v, rule[0])[0]["agent_text"]
            .as_str()
            .unwrap()
            .contains("No solution found"),
        "{v}"
    );
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

impl Repo {
    /// Commits what is in the tree, on the current branch.
    fn commit(&self, msg: &str) {
        git(self.path(), &["add", "-A"]);
        git(self.path(), &["commit", "-qm", msg]);
    }

    /// A turn starting now: the snapshot's head is HEAD, nothing uncommitted.
    fn start_turn(&self) {
        let rev = Command::new("git")
            .current_dir(self.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        std::fs::write(self.snap.path().join("head"), &rev.stdout).unwrap();
        std::fs::write(self.snap.path().join("dirty"), "").unwrap();
    }
}

/// `base: branch` (§6.2): a chart bumped once on the branch needs no second bump when the turn
/// changes it again; a chart the branch changes without a bump is still reported.
#[test]
fn a_chart_is_bumped_once_per_branch() {
    let id = "kubernetes/helm/version-bump";
    let chart = |v: &str| format!("apiVersion: v2\nname: web\nversion: {v}\n");
    let r = Repo::new(&files(&[
        ("ct.yaml", "target-branch: main\n"),
        ("charts/web/Chart.yaml", &chart("1.2.0")),
        ("charts/web/values.yaml", "replicas: 1\n"),
    ]));
    git(r.path(), &["checkout", "-qb", "feat"]);
    write(
        r.path(),
        &files(&[
            ("charts/web/Chart.yaml", &chart("1.2.1")),
            ("charts/web/values.yaml", "replicas: 2\n"),
        ]),
    );
    r.commit("bump web");
    r.start_turn();
    write(
        r.path(),
        &files(&[("charts/web/values.yaml", "replicas: 3\n")]),
    );
    let (code, v) = r.turn(&[id]);
    assert_eq!(code, 0, "{v}");
    assert!(findings_of(&v, id).is_empty(), "{v}");
    // the commit-time check agrees
    let (code, v) = r.check(&["--changed"], &[id], PATH);
    assert_eq!(code, 0, "{v}");
    // on a branch that never bumped it, the turn's change is reported
    git(r.path(), &["checkout", "-q", "--", "."]);
    git(r.path(), &["checkout", "-qb", "other", "main"]);
    r.start_turn();
    write(
        r.path(),
        &files(&[("charts/web/values.yaml", "replicas: 3\n")]),
    );
    let (code, v) = r.turn(&[id]);
    assert_eq!(code, 1, "{v}");
    let f = findings_of(&v, id);
    assert!(
        f[0]["agent_text"]
            .as_str()
            .unwrap()
            .contains("chart charts/web changed but its Chart.yaml version is still 1.2.0"),
        "{v}"
    );
}

/// `base: branch`: a migration this branch added and that is not merged yet may be edited; one
/// already on the default branch may not.
#[test]
fn a_migration_the_branch_added_may_be_edited() {
    let id = "sql/applied-migrations";
    let init = "supabase/migrations/20260101000000_init.sql";
    let paid = "supabase/migrations/20260102000000_paid.sql";
    let r = Repo::new(&files(&[
        ("supabase/config.toml", "project_id = \"shop\"\n"),
        (init, "CREATE TABLE orders (id bigint PRIMARY KEY);\n"),
    ]));
    git(r.path(), &["checkout", "-qb", "feat"]);
    write(
        r.path(),
        &files(&[(paid, "ALTER TABLE orders ADD COLUMN paid boolean;\n")]),
    );
    r.commit("add paid");
    r.start_turn();
    write(
        r.path(),
        &files(&[(
            paid,
            "ALTER TABLE orders ADD COLUMN paid boolean NOT NULL DEFAULT false;\n",
        )]),
    );
    let (code, v) = r.turn(&[id]);
    assert_eq!(code, 0, "{v}");
    assert!(findings_of(&v, id).is_empty(), "{v}");
    let (code, v) = r.check(&["--changed"], &[id], PATH);
    assert_eq!(code, 0, "{v}");
    // the default branch's migration, edited in the turn
    write(
        r.path(),
        &files(&[(
            init,
            "CREATE TABLE orders (id bigint PRIMARY KEY, total int);\n",
        )]),
    );
    let (code, v) = r.turn(&[id]);
    assert_eq!(code, 1, "{v}");
    let f = findings_of(&v, id);
    assert_eq!(f.len(), 1, "{v}");
    assert!(
        f[0]["agent_text"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{init} an existing migration was modified")),
        "{v}"
    );
    let (code, _) = r.check(&["--changed"], &[id], PATH);
    assert_eq!(code, 1);
}

/// §10.5: `accept:` holds for any rule's finding, by rule and file; a commit the check refuses
/// tells the person the entry to add (their output only: `--json` and the agent never get it).
#[test]
fn a_refused_commit_names_the_accept_entry() {
    let id = "python/no-swallowed-cancel";
    let r = Repo::new(&files(&[("app/a.py", "x = 1\n")]));
    let swallow = "import asyncio\n\n\nasync def a(q):\n    try:\n        await q.get()\n    except asyncio.CancelledError:\n        pass\n";
    write(r.path(), &files(&[("app/a.py", swallow)]));
    let person = || {
        let out = Command::new(exe())
            .current_dir(r.path())
            .args(["check", "--changed", &format!("--rules={id}")])
            .env("PATH", PATH)
            .env("FORGE_HOME", r.home.path())
            .env("FORGE_NO_DOWNLOAD", "1")
            .env_remove("VIRTUAL_ENV")
            .env_remove("CLAUDECODE")
            .output()
            .expect("forge check");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };
    let (code, text) = person();
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains(
            "accept:\n  - {rule: python/no-swallowed-cancel, file: \"app/a.py\", reason: \"<why>\"}"
        ),
        "{text}"
    );
    let (_, v) = r.check(&["--changed"], &[id], PATH);
    assert!(!v.to_string().contains("<why>"), "{v}");
    write(
        r.path(),
        &files(&[(
            "forge.yaml",
            "forge: \"0.6\"\naccept:\n  - {rule: python/no-swallowed-cancel, file: \"app/a.py\", reason: \"a shutdown path\"}\n",
        )]),
    );
    let (code, text) = person();
    assert_eq!(code, Some(0), "{text}");
}
