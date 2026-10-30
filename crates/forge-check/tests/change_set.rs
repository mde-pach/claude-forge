//! The change set on temporary git repositories, and the edit-time run's overhead.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use forge_check::change::{Added, ChangeSet, Mode};
use forge_check::{ActiveRule, Checker, Options};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=forge",
            "-c",
            "user.email=forge@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "maintenance.auto=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn put(dir: &Path, path: &str, text: &str) {
    let f = dir.join(path);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    for (p, t) in files {
        put(d.path(), p, t);
    }
    git(d.path(), &["add", "-A"]);
    git(d.path(), &["commit", "-qm", "head", "--allow-empty"]);
    d
}

fn lines(a: &Added) -> Vec<u32> {
    match a {
        Added::Whole => vec![0],
        Added::Lines(l) => l.iter().copied().collect(),
    }
}

#[test]
fn changed_against_head() {
    let d = repo(&[("a.py", "a\nb\n"), ("gone.py", "x\n"), ("keep.py", "k\n")]);
    put(d.path(), "a.py", "a\nnew\nb\n");
    put(d.path(), "new.py", "n\n");
    put(d.path(), "dir/ünï.py", "u\n");
    std::fs::remove_file(d.path().join("gone.py")).unwrap();
    // never part of a change: nested worktrees, dependencies, Forge's own directory
    put(d.path(), ".claude/worktrees/agent/x.py", "w\n");
    put(d.path(), "node_modules/p/i.js", "m\n");
    put(d.path(), ".forge/rules/r.yaml", "r\n");
    let c = ChangeSet::compute(d.path(), Mode::Changed).unwrap();
    assert_eq!(c.changed, ["a.py", "dir/ünï.py", "gone.py", "new.py"]);
    assert_eq!(c.deleted, ["gone.py"]);
    assert_eq!(c.inbase, ["a.py", "gone.py"]);
    assert_eq!(lines(&c.added["a.py"]), [2]);
    assert_eq!(c.added["new.py"], Added::Whole);
    assert_eq!(
        std::fs::read_to_string(c.base_copy("a.py").unwrap()).unwrap(),
        "a\nb\n"
    );
    assert!(c.base_copy("new.py").is_none());
}

#[test]
fn turn_against_its_snapshot() {
    let d = repo(&[("a.py", "a\n"), ("b.py", "b\n")]);
    // before the turn: the user's own uncommitted work in b.py and an untracked note
    put(d.path(), "b.py", "b\nuser\n");
    put(d.path(), "notes.txt", "mine\n");
    let snap = tempfile::tempdir().unwrap();
    std::fs::write(
        snap.path().join("head"),
        git(d.path(), &["rev-parse", "HEAD"]),
    )
    .unwrap();
    let hash = |p: &str| git(d.path(), &["hash-object", p]);
    std::fs::write(
        snap.path().join("dirty"),
        format!("b.py\t{}\nnotes.txt\t{}\n", hash("b.py"), hash("notes.txt")),
    )
    .unwrap();
    for p in ["b.py", "notes.txt"] {
        put(
            snap.path(),
            &format!("files/{p}"),
            &std::fs::read_to_string(d.path().join(p)).unwrap(),
        );
    }
    // the turn: edits b.py further, commits a change to a.py, adds c.py
    put(d.path(), "b.py", "b\nuser\nagent\n");
    put(d.path(), "a.py", "a\nagent\n");
    git(d.path(), &["commit", "-qam", "agent"]);
    put(d.path(), "c.py", "c\n");
    let c = ChangeSet::compute(d.path(), Mode::Turn(snap.path().to_path_buf())).unwrap();
    assert_eq!(
        c.changed,
        ["a.py", "b.py", "c.py"],
        "the user's notes.txt is not the turn's"
    );
    // b.py: only the line the turn added, not the user's
    assert_eq!(lines(&c.added["b.py"]), [3]);
    assert_eq!(lines(&c.added["a.py"]), [2]);
    assert_eq!(
        std::fs::read_to_string(c.base_copy("b.py").unwrap()).unwrap(),
        "b\nuser\n"
    );
}

#[test]
fn since_a_merge_base() {
    let d = repo(&[("a.py", "a\n")]);
    git(d.path(), &["checkout", "-qb", "feature"]);
    put(d.path(), "a.py", "a\nb\n");
    git(d.path(), &["commit", "-qam", "f"]);
    put(d.path(), "u.py", "u\n");
    let c = ChangeSet::compute(d.path(), Mode::Since("main".into())).unwrap();
    assert_eq!(c.changed, ["a.py", "u.py"]);
    assert_eq!(lines(&c.added["a.py"]), [2]);
    assert!(ChangeSet::compute(d.path(), Mode::Since("nope".into())).is_err());
}

#[test]
fn all_lists_the_project() {
    let d = repo(&[("a.py", "a\n"), ("b/c.py", "c\n")]);
    put(d.path(), "d.py", "d\n");
    let c = ChangeSet::compute(d.path(), Mode::All).unwrap();
    assert_eq!(c.all.as_deref().unwrap(), ["a.py", "b/c.py", "d.py"]);
    assert!(c.added.is_empty());
}

/// The edit-time run on one Python file with the python pack's ast-grep rules: Forge's own
/// overhead, median of warm runs (printed; asserted loosely, debug builds are slower).
#[test]
fn post_edit_overhead() {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).unwrap();
    let rules: Vec<ActiveRule> = set
        .packs
        .values()
        .filter(|p| p.id == "python" || p.id.starts_with("python/"))
        .flat_map(|p| p.rules.iter())
        .filter(|r| forge_check::astgrep::target_of(r).is_some())
        .cloned()
        .map(ActiveRule::new)
        .collect();
    assert!(rules.len() > 10);
    let body: String = (0..40)
        .map(|i| format!("def f{i}(x):\n    return x + {i}\n\n\n"))
        .collect();
    let d = repo(&[("app/a.py", &body)]);
    put(
        d.path(),
        "app/a.py",
        &format!("{body}def g():\n    eval('1')\n"),
    );
    let checker = Checker::new(d.path().to_path_buf(), rules);
    let opts = Options::post_edit(std::env::current_exe().unwrap());
    let mut times = Vec::new();
    for _ in 0..15 {
        let o = checker
            .check_files(&["app/a.py".to_string()], None, &opts)
            .unwrap();
        times.push(o.timings.forge);
    }
    times.sort();
    let median = times[times.len() / 2];
    eprintln!(
        "post-edit overhead, python ast-grep rules ({} rules): median {median:?}",
        checker.rules.len()
    );
    let _: BTreeSet<u8> = BTreeSet::new();
    assert!(median < Duration::from_millis(500), "{median:?}");
}
