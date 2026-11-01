//! §10 (the checker) on small rules in temporary git repositories: the order of the steps and
//! where they stop, the check stages, attribution of a tool's diagnostics to the change, what a
//! skipped or stopped check says, advice, and what the agent and the person read.
//!
//! The check commands are plain shell (no project tool, no builtin), so no Forge executable or
//! machine tool is needed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use forge_check::change::{ChangeSet, Mode};
use forge_check::{ActiveRule, Checker, Options, Status};
use forge_core::{NoteKind, Severity};
use forge_packs::model::Rule;
use serde_json::{Value, json};

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

/// A repository with `files` committed.
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

/// A rule as its file would say it (loading's own fields left at their defaults).
fn rule(v: Value) -> Rule {
    serde_json::from_value(v).expect("rule")
}

/// A `check` rule of `stage` running `run` (shell) on Python files.
fn check_rule(id: &str, stage: &str, run: &str) -> Rule {
    rule(json!({
        "id": id, "description": format!("{id}."),
        "scope": {"language": "python"},
        "enforce": [{"target": "check", "stage": stage, "run": run}],
    }))
}

fn checker(root: &Path, rules: Vec<Rule>) -> Checker {
    Checker::new(
        root.to_path_buf(),
        rules.into_iter().map(ActiveRule::new).collect(),
    )
}

fn exe() -> PathBuf {
    PathBuf::from("/bin/false")
}

fn stop() -> Options {
    Options::stop(exe())
}

fn ran(root: &Path, marker: &str) -> bool {
    root.join(marker).exists()
}

#[test]
fn exit_codes() {
    assert_eq!(Status::Pass.exit_code(), 0);
    assert_eq!(Status::Fail.exit_code(), 1);
    assert_eq!(Status::Unverified.exit_code(), 3);
    assert_eq!(Status::AdviceOnly.exit_code(), 4);
}

/// Step 1 (counts) failing stops the run before the forbid rules and the stages.
#[test]
fn a_failing_count_stops_the_later_steps() {
    let d = repo(&[("a.py", "x = 1  # noqa\n")]);
    put(d.path(), "a.py", "x = 1  # noqa\ny = 2  # noqa\n");
    let count = rule(json!({
        "id": "acme/no-more-noqa", "description": "No new noqa comments.",
        "scope": {"language": "python"},
        "enforce": [{"target": "grep", "mode": "no-increase", "pattern": "# noqa"}],
    }));
    let lint = check_rule("acme/lint", "lint", "touch lint-ran");
    let o = checker(d.path(), vec![count, lint])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail);
    assert_eq!(o.report.findings.len(), 1, "{:?}", o.report.findings);
    let f = &o.report.findings[0];
    assert_eq!(f.agent_text(), "a.py No new noqa comments. (1 -> 2)");
    assert!(!ran(d.path(), "lint-ran"), "the lint stage must not run");
}

/// A `no-decrease` count fails when the file has fewer matches than in the base.
#[test]
fn no_decrease_counts() {
    let d = repo(&[("t.py", "assert a\nassert b\n")]);
    put(d.path(), "t.py", "assert a\n");
    let asserts = rule(json!({
        "id": "acme/asserts", "description": "Keep the asserts.",
        "scope": {"language": "python"},
        "enforce": [{"target": "grep", "mode": "no-decrease", "pattern": "^assert "}],
    }));
    let o = checker(d.path(), vec![asserts])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail);
    assert_eq!(
        o.report.findings[0].agent_text(),
        "t.py Keep the asserts. (2 -> 1)"
    );
    // adding one back passes
    put(d.path(), "t.py", "assert a\nassert c\n");
    let o = checker(
        d.path(),
        vec![rule(json!({
            "id": "acme/asserts", "description": "Keep the asserts.",
            "scope": {"language": "python"},
            "enforce": [{"target": "grep", "mode": "no-decrease", "pattern": "^assert "}],
        }))],
    )
    .check(Mode::Changed, &stop())
    .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
}

/// Counts are totals per rule over the changed files: a match moved to another file is not a
/// decrease; deleting the file that held it is.
#[test]
fn counts_are_totals_deleted_files_included() {
    let keep = || {
        rule(json!({
            "id": "acme/protect", "description": "Keep the protections.",
            "scope": {"language": "any"},
            "enforce": [{"target": "grep", "mode": "no-decrease", "pattern": "prevent_destroy"}],
        }))
    };
    let d = repo(&[("main.tf", "db prevent_destroy\nnet prevent_destroy\n")]);
    put(d.path(), "main.tf", "\n");
    put(d.path(), "db.tf", "db prevent_destroy\n");
    put(d.path(), "net.tf", "net prevent_destroy\n");
    let o = checker(d.path(), vec![keep()])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    let d = repo(&[("db.tf", "db prevent_destroy\n"), ("app.tf", "x\n")]);
    std::fs::remove_file(d.path().join("db.tf")).unwrap();
    let o = checker(d.path(), vec![keep()])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail, "{:?}", o.report);
    assert_eq!(
        o.report.findings[0].agent_text(),
        "db.tf Keep the protections. (1 -> 0)"
    );
}

/// A failing check that names no added line is run again on the base: what the base already
/// reported is not the change's (a note says so); what is new fails.
#[test]
fn a_failure_the_base_already_had_is_not_the_changes() {
    let lacks = || {
        rule(json!({
            "id": "acme/conf", "description": "conf.txt declares REQUIRED.",
            "scope": {"language": "any"},
            "enforce": [{"target": "check", "stage": "lint", "when_changed": ["conf.txt"],
                "run": "grep -q REQUIRED conf.txt || { echo 'conf.txt does not declare REQUIRED'; exit 1; }"}],
        }))
    };
    // already missing: an unrelated edit of the file passes
    let d = repo(&[("conf.txt", "a = 1\n")]);
    put(d.path(), "conf.txt", "a = 2\n");
    let o = checker(d.path(), vec![lacks()])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    assert!(
        o.report
            .notes
            .iter()
            .any(|n| n.text.contains("was already there")),
        "{:?}",
        o.report.notes
    );
    // removed by the change: it fails
    let d = repo(&[("conf.txt", "REQUIRED\n")]);
    put(d.path(), "conf.txt", "a = 2\n");
    let o = checker(d.path(), vec![lacks()])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail, "{:?}", o.report);
    assert!(
        o.report.findings[0]
            .agent_text()
            .contains("does not declare REQUIRED")
    );
}

/// The base run reads the base tree through `FORGE_ROOT` too (as builtins do): a failure the
/// change brought is not taken for one the base already had.
#[test]
fn the_base_run_names_the_base_tree_as_its_root() {
    let lacks = rule(json!({
        "id": "acme/conf", "description": "conf.txt declares REQUIRED.",
        "scope": {"language": "any"},
        "enforce": [{"target": "check", "stage": "lint", "when_changed": ["conf.txt"],
            "run": "grep -q REQUIRED \"$FORGE_ROOT/conf.txt\" || { echo 'conf.txt does not declare REQUIRED'; exit 1; }"}],
    }));
    let d = repo(&[("conf.txt", "REQUIRED\n")]);
    put(d.path(), "conf.txt", "a = 2\n");
    let o = checker(d.path(), vec![lacks])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail, "{:?}", o.report);
}

/// A kube-linter-style finding at line 0 of a changed manifest (about the whole object) is
/// compared with the base: an edit elsewhere in a manifest that already had it passes; the
/// change that brings it fails.
#[test]
fn a_whole_file_finding_the_base_already_had_is_not_the_changes() {
    let lint = || {
        rule(json!({
            "id": "acme/kube", "description": "Workloads set readOnlyRootFilesystem.",
            "scope": {"language": "any", "files": ["deploy/*.yaml"]},
            "enforce": [{"target": "check", "stage": "lint",
                "run": "grep -q readOnlyRootFilesystem deploy/web.yaml || { echo 'deploy/web.yaml:0: (object: <no namespace>/web apps/v1, Kind=Deployment) container \"web\" does not have a read-only root file system (check: no-read-only-root-fs)'; exit 1; }"}],
        }))
    };
    let d = repo(&[("deploy/web.yaml", "kind: Deployment\nreplicas: 1\n")]);
    put(
        d.path(),
        "deploy/web.yaml",
        "kind: Deployment\nreplicas: 2\n",
    );
    let o = checker(d.path(), vec![lint()])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    let d = repo(&[(
        "deploy/web.yaml",
        "kind: Deployment\nreadOnlyRootFilesystem: true\n",
    )]);
    put(d.path(), "deploy/web.yaml", "kind: Deployment\n");
    let o = checker(d.path(), vec![lint()])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail, "{:?}", o.report);
    assert!(o.report.findings[0].agent_text().contains("read-only root"));
}

/// Step 2: a forbid grep fails on an added line only, names `path:line` and the description,
/// and stops the stages; documentation files are never grepped.
#[test]
fn forbid_on_added_lines_and_never_in_prose() {
    let d = repo(&[("a.py", "eval(x)\n"), ("README.md", "intro\n")]);
    put(d.path(), "a.py", "eval(x)\nok = 1\neval(y)\n");
    put(d.path(), "README.md", "intro\nNever call eval(x).\n");
    let forbid = rule(json!({
        "id": "acme/no-eval", "description": "No eval().",
        "scope": {"language": "any"},
        "enforce": [{"target": "grep", "mode": "forbid", "pattern": "eval\\("}],
    }));
    let lint = check_rule("acme/lint", "lint", "touch lint-ran");
    let o = checker(d.path(), vec![forbid, lint])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail);
    let texts: Vec<String> = o.report.findings.iter().map(|f| f.agent_text()).collect();
    assert_eq!(texts, ["a.py:3 No eval()."]);
    assert!(!ran(d.path(), "lint-ran"));
}

/// Step 3: the stages run in order and stop at the first failing one: a failing codegen check
/// stops the run before the typecheck; a passing one lets it run.
#[test]
fn a_failing_codegen_stage_stops_before_the_typecheck() {
    let d = repo(&[("a.py", "x = 1\n")]);
    put(d.path(), "a.py", "x = 2\n");
    let rules = |codegen: &str| {
        vec![
            check_rule("acme/codegen", "codegen", codegen),
            check_rule("acme/types", "typecheck", "touch types-ran"),
        ]
    };
    let o = checker(
        d.path(),
        // stale only after the change (a failure the base already had is not the change's)
        rules("grep -q 'x = 2' a.py || exit 0; echo 'the client is stale'; echo 'run make gen'; echo more; echo more; exit 1"),
    )
    .check(Mode::Changed, &stop())
    .unwrap();
    assert_eq!(o.status, Status::Fail);
    assert!(!ran(d.path(), "types-ran"));
    let m = &o.report.findings[0].message;
    assert!(
        m.starts_with(
            "The generated code is not up to date (expected: acme/codegen):\nthe client is stale"
        ),
        "{m}"
    );
    let o = checker(d.path(), rules("true"))
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass);
    assert!(ran(d.path(), "types-ran"));
}

/// A check whose tool is missing is skipped: a note for the person, never a failure.
#[test]
fn a_missing_tool_is_a_note_and_the_check_passes() {
    let d = repo(&[("a.py", "x = 1\n")]);
    put(d.path(), "a.py", "x = 2\n");
    let mut c = checker(
        d.path(),
        vec![check_rule("acme/tests", "test", "vitest run")],
    );
    c.tools = std::sync::Arc::new(forge_check::tools::LocalTools {
        path: Some("/nonexistent".into()),
        ..Default::default()
    });
    let o = c.check(Mode::Changed, &stop()).unwrap();
    assert_eq!(o.status, Status::Pass);
    assert!(o.report.findings.is_empty());
    let n = &o.report.notes[0];
    assert_eq!(n.kind, NoteKind::MissingTool);
    assert_eq!(n.to_string(), "acme/tests: skipped: vitest not installed");
}

/// `when_changed`: the check runs only when one of those files changed.
#[test]
fn when_changed_gates_a_check() {
    let d = repo(&[("a.py", "x = 1\n"), ("migrations/1.sql", "select 1;\n")]);
    put(d.path(), "a.py", "x = 2\n");
    let mut r = check_rule("acme/migrations", "migration", "touch migration-ran");
    if let forge_packs::model::Target::Check(c) = &mut r.enforce[0] {
        c.when_changed = Some(vec!["migrations/**".into()]);
    }
    let o = checker(d.path(), vec![r.clone()])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass);
    assert!(!ran(d.path(), "migration-ran"));
    put(d.path(), "migrations/1.sql", "select 2;\n");
    checker(d.path(), vec![r])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert!(ran(d.path(), "migration-ran"));
}

/// A check past its own `timeout` fails, saying so first, with its output until then.
#[test]
fn a_check_past_its_timeout_fails_saying_so() {
    let d = repo(&[("a.py", "x = 1\n")]);
    put(d.path(), "a.py", "x = 2\n");
    let mut r = check_rule("acme/tests", "test", "echo collecting; sleep 5");
    if let forge_packs::model::Target::Check(c) = &mut r.enforce[0] {
        c.timeout = Some(1);
    }
    let o = checker(d.path(), vec![r])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail);
    let m = &o.report.findings[0].message;
    assert!(m.starts_with("timed out after 1s and was stopped"), "{m}");
    assert!(m.ends_with("collecting"), "{m}");
}

/// A run whose time budget runs out stops the check: a note (Forge's), never a failure.
#[test]
fn a_spent_budget_is_a_note_not_a_failure() {
    let d = repo(&[("a.py", "x = 1\n")]);
    put(d.path(), "a.py", "x = 2\n");
    let mut opts = stop();
    opts.budget = Duration::from_millis(300);
    let o = checker(d.path(), vec![check_rule("acme/tests", "test", "sleep 5")])
        .check(Mode::Changed, &opts)
        .unwrap();
    assert_eq!(o.status, Status::Pass);
    assert!(o.report.findings.is_empty());
    let n = &o.report.notes[0];
    assert_eq!(n.kind, NoteKind::ForgeFailure);
    assert!(
        n.text
            .starts_with("not checked: the run's time budget ran out"),
        "{n}"
    );
}

/// Lint diagnostics count on the added lines only (outside `--all`); the rest is a note.
#[test]
fn lint_is_attributed_by_added_line() {
    let d = repo(&[("a.py", "a = 1\nb = 2\n")]);
    put(d.path(), "a.py", "a = 1\nb = 2\nc = 3\n");
    let lint = |line: u32| {
        check_rule(
            "acme/lint",
            "lint",
            &format!("echo 'a.py:{line}:1: error E1 bad'; exit 1"),
        )
    };
    let o = checker(d.path(), vec![lint(1)])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    assert_eq!(
        o.report.notes[0].text,
        "ok for the added lines (findings remain on lines this change did not add)"
    );
    let o = checker(d.path(), vec![lint(3)])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail);
    assert_eq!(o.report.findings[0].agent_text(), "a.py:3 error E1 bad");
    // --all: every diagnostic counts
    let o = checker(d.path(), vec![lint(1)])
        .check(Mode::All, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail);
}

/// A check that compared with the base itself (it created `$FORGE_ATTRIBUTED`) keeps its
/// findings wherever they are: a function the change left unused in a file it did not touch.
#[test]
fn self_attributed_findings_are_taken_as_they_are() {
    let d = repo(&[("a.py", "a = 1\n"), ("b.py", "def f(): pass\n")]);
    put(d.path(), "a.py", "a = 2\n");
    let rule = |touch: bool| {
        check_rule(
            "acme/dead",
            "lint",
            &format!(
                "{}echo 'b.py:1 function `f` is unused'; exit 1",
                if touch {
                    ": > \"$FORGE_ATTRIBUTED\"; "
                } else {
                    ""
                }
            ),
        )
    };
    let o = checker(d.path(), vec![rule(false)])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    let o = checker(d.path(), vec![rule(true)])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail);
    assert_eq!(
        o.report.findings[0].agent_text(),
        "b.py:1 function `f` is unused"
    );
}

/// A run failing with warnings only passes (a note); with `fail_on_warnings` a warning on an
/// added line fails.
#[test]
fn warnings_count_only_with_fail_on_warnings() {
    let d = repo(&[("a.py", "a = 1\n")]);
    put(d.path(), "a.py", "a = 1\nb = 2\n");
    let mut r = check_rule(
        "acme/lint",
        "lint",
        "echo 'a.py:2:1: warning W1 meh'; exit 1",
    );
    let o = checker(d.path(), vec![r.clone()])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    assert_eq!(o.report.notes[0].text, "ok: only warnings");
    if let forge_packs::model::Target::Check(c) = &mut r.enforce[0] {
        c.fail_on_warnings = Some(true);
    }
    let o = checker(d.path(), vec![r])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail);
}

/// A typecheck counts errors in the changed files only; errors elsewhere are a note.
#[test]
fn typecheck_errors_in_unchanged_files_are_a_note() {
    let d = repo(&[("a.py", "a = 1\n"), ("old.py", "o = 1\n")]);
    put(d.path(), "a.py", "a = 2\n");
    let o = checker(
        d.path(),
        vec![check_rule(
            "acme/types",
            "typecheck",
            "echo 'old.py:1:1: error: bad type'; exit 1",
        )],
    )
    .check(Mode::Changed, &stop())
    .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    assert_eq!(
        o.report.notes[0].text,
        "ok for the changed files (errors remain in unchanged files)"
    );
}

/// An unresolved import of a dependency the project declares is the environment's state (the
/// dependency is not installed where the type check looks): a note, not a finding. One of a
/// package the project does not declare stays a finding.
#[test]
fn an_uninstalled_declared_dependency_is_a_note() {
    let d = repo(&[
        ("app.py", "x = 1\n"),
        ("requirements.txt", "beautifulsoup4==4.12.3\n"),
    ]);
    put(
        d.path(),
        "app.py",
        "from bs4 import BeautifulSoup\nimport numpy\n",
    );
    let ty = |modules: &str| {
        format!(
            "st=0; for m in {modules}; do grep -q \"$m\" app.py || continue; st=1; echo \"app.py:1:6: error[unresolved-import] Cannot resolve imported module \\`$m\\`\"; done; echo 'Found 1 diagnostic'; exit $st"
        )
    };
    let o = checker(
        d.path(),
        vec![check_rule("acme/types", "typecheck", &ty("bs4"))],
    )
    .check(Mode::Changed, &stop())
    .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    assert_eq!(o.report.agent_text(20), "");
    assert!(
        o.report.notes.iter().any(|n| n.text
            == "dependencies are not installed in the environment Forge type-checks with: beautifulsoup4 (bs4); their imports are not checked until they are"),
        "{:?}",
        o.report.notes
    );
    let o = checker(
        d.path(),
        vec![check_rule("acme/types", "typecheck", &ty("bs4 numpy"))],
    )
    .check(Mode::Changed, &stop())
    .unwrap();
    assert_eq!(o.status, Status::Fail, "{:?}", o.report);
    let text = o.report.agent_text(20);
    assert!(
        text.contains("`numpy`") && !text.contains("`bs4`"),
        "{text}"
    );
}

/// A failure that names no location is shown under the stage's header with an excerpt; more
/// than 15 attributed findings are capped with a count.
#[test]
fn failure_text_and_caps() {
    let d = repo(&[("a.py", "")]);
    let body: String = (1..=20).map(|i| format!("v{i} = {i}\n")).collect();
    put(d.path(), "a.py", &body);
    let o = checker(
        d.path(),
        vec![check_rule(
            "acme/lint",
            "lint",
            "i=1; while [ $i -le 20 ]; do echo \"a.py:$i:1: error E$i\"; i=$((i+1)); done; exit 1",
        )],
    )
    .check(Mode::Changed, &stop())
    .unwrap();
    assert_eq!(o.report.findings.len(), 16);
    assert_eq!(o.report.findings[15].message, "... and 5 more");
    let o = checker(
        d.path(),
        vec![check_rule(
            "acme/tests",
            "test",
            "grep -q v1 a.py || exit 0; echo one; echo two; echo three; echo four; exit 1",
        )],
    )
    .check(Mode::Changed, &stop())
    .unwrap();
    assert_eq!(
        o.report.findings[0].message,
        "Tests failed (expected: acme/tests):\none\ntwo\nthree\nfour"
    );
}

/// A rule with `where` needs the types engine: without one it is a note, never a finding.
#[test]
fn a_typed_rule_without_the_types_engine_is_a_note() {
    let d = repo(&[("a.py", "")]);
    put(d.path(), "a.py", "x.query()\n");
    let typed = rule(json!({
        "id": "acme/typed", "description": "No query().",
        "scope": {"language": "python"},
        "enforce": [{"target": "ast-grep", "mode": "forbid",
            "rule": {"pattern": "$S.query()"},
            "where": {"S": {"type": {"module": "sqlalchemy.orm", "name": "Session"}}}}],
    }));
    let o = checker(d.path(), vec![typed])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass);
    let n = &o.report.notes[0];
    assert_eq!(n.kind, NoteKind::NotEnforced);
    assert_eq!(
        n.text,
        "not enforced: its matches need the types engine, which is not available"
    );
}

/// After an edit: `advise` rules report on added lines as advice (never a failure), and the
/// `check` stages other than format and lint do not run.
#[test]
fn after_an_edit_advice_and_no_later_stages() {
    let d = repo(&[("a.py", "a = 1\n")]);
    put(d.path(), "a.py", "a = 1\nprint(a)\n");
    let advise = rule(json!({
        "id": "acme/no-print", "description": "Prefer logging to print().",
        "scope": {"language": "python"},
        "enforce": [{"target": "grep", "mode": "advise", "pattern": "print\\("}],
    }));
    let tests = check_rule("acme/tests", "test", "touch tests-ran");
    let c = checker(d.path(), vec![advise, tests]);
    let o = c
        .check_files(&["a.py".to_string()], None, &Options::post_edit(exe()))
        .unwrap();
    assert_eq!(o.status, Status::AdviceOnly);
    assert_eq!(o.report.findings[0].severity, Severity::Advice);
    assert_eq!(
        o.report.agent_text(20),
        "a.py:2 Advice: Prefer logging to print()."
    );
    assert!(!ran(d.path(), "tests-ran"));
    // at Stop and in CI, advise-mode patterns are not run
    let o = c.check(Mode::Changed, &stop()).unwrap();
    assert!(o.report.findings.is_empty(), "{:?}", o.report);
}

/// What the person reads (rule ids, then the notes) and the JSON for scripts; the agent's
/// text has no rule id and no note.
#[test]
fn person_text_json_and_agent_text() {
    let d = repo(&[("a.py", "a = 1\n")]);
    put(d.path(), "a.py", "a = 1\neval(a)\n");
    let forbid = rule(json!({
        "id": "acme/no-eval", "description": "No eval().",
        "scope": {"language": "python"},
        "enforce": [{"target": "grep", "mode": "forbid", "pattern": "eval\\("}],
    }));
    let mut c = checker(
        d.path(),
        vec![forbid, check_rule("acme/tests", "test", "vitest")],
    );
    c.tools = std::sync::Arc::new(forge_check::tools::LocalTools {
        path: Some("/nonexistent".into()),
        ..Default::default()
    });
    let o = c.check(Mode::Changed, &stop()).unwrap();
    assert_eq!(o.report.agent_text(20), "a.py:2 No eval().");
    assert_eq!(
        forge_check::output::person_text(&o),
        "acme/no-eval a.py:2 No eval().\n"
    );
    let v = forge_check::output::json(&o);
    assert_eq!(v["status"], "fail");
    assert_eq!(v["exit"], 1);
    assert_eq!(v["findings"][0]["rule"], "acme/no-eval");
    assert_eq!(v["findings"][0]["agent_text"], "a.py:2 No eval().");
    // the stage step was not reached: no note; a run that reaches it has one
    put(d.path(), "a.py", "a = 2\n");
    let o = c.check(Mode::Changed, &stop()).unwrap();
    assert_eq!(
        forge_check::output::person_text(&o),
        "Notes:\n  acme/tests: skipped: vitest not installed\n"
    );
    assert_eq!(o.report.agent_text(20), "");
}

/// A typecheck error the base already has in a changed file does not count (the command runs
/// again in a copy of the base tree); one more of a new text fails, naming only that one, and
/// the errors already there are a note for the person.
#[test]
fn typecheck_errors_the_base_already_has() {
    let run = "n=0; while read -r l; do n=$((n+1)); case $l in *bad*) echo \"a.py:$n:1: error: $l is bad\"; st=1 ;; esac; done < a.py; exit ${st:-0}";
    let d = repo(&[("a.py", "bad1\nok\n")]);
    put(d.path(), "a.py", "bad1\nok\nfine\n");
    let o = checker(d.path(), vec![check_rule("acme/types", "typecheck", run)])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Pass, "{:?}", o.report);
    put(d.path(), "a.py", "bad1\nok\nbad2\n");
    let o = checker(d.path(), vec![check_rule("acme/types", "typecheck", run)])
        .check(Mode::Changed, &stop())
        .unwrap();
    assert_eq!(o.status, Status::Fail, "{:?}", o.report);
    // the agent reads only its new error; the person also reads that one was already there
    assert_eq!(o.report.agent_text(20), "a.py:3 error: bad2 is bad");
    assert!(
        forge_check::output::person_text(&o).contains(
            "acme/types: 1 error(s) in the changed files were already there before this change"
        ),
        "{}",
        forge_check::output::person_text(&o)
    );
}

/// The builtin protocol: a finding the user accepted (forge.yaml `accept:`, by its exact text)
/// is dropped; notes go to the notes file as JSON lines; the status is 1 only for an error.
#[test]
fn builtin_protocol_accepts_and_notes() {
    use forge_core::builtin::{BuiltinCtx, Scope};
    use forge_core::{Finding, Note, Report};
    let d = tempfile::tempdir().unwrap();
    let notes = d.path().join("notes");
    let ctx = BuiltinCtx {
        root: d.path().to_path_buf(),
        base: None,
        changed: vec![],
        rule: "acme/ratchet".into(),
        scope: Scope::Changed,
        accepted: vec!["pyproject.toml:3 ruff ignore E501 added".into()],
        notes_file: Some(notes.clone()),
    };
    let report = |msg: &str| Report {
        findings: vec![Finding::error("acme/ratchet", msg).at("pyproject.toml", Some(3))],
        notes: vec![Note::new(NoteKind::Environment, "ruff 0.6 read the config")],
    };
    assert_eq!(ctx.finish(report("ruff ignore E501 added")), 0, "accepted");
    assert_eq!(ctx.finish(report("ruff ignore E722 added")), 1);
    let text = std::fs::read_to_string(&notes).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    let n: Note = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(n.kind, NoteKind::Environment);
}

/// `--since` a ref with no merge base fails with what to fetch.
#[test]
fn since_without_a_merge_base_says_what_to_fetch() {
    let d = repo(&[("a.py", "a\n")]);
    let e = ChangeSet::compute(d.path(), Mode::Since("origin/nope".into())).unwrap_err();
    assert_eq!(
        e.to_string(),
        "no merge base for origin/nope and HEAD (fetch it: git fetch origin <branch>, or check out with fetch-depth: 0)"
    );
}
