//! §3 `forge test`: a repository's local rules' examples, run through the machinery of the
//! built-in packs' example tests. Passing, failing and skipped examples, with and without
//! Claude Code (`CLAUDECODE=1`): the same verdicts, a skip told by its structure, and no note
//! in what Claude reads.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

fn put(root: &Path, path: &str, text: &str) {
    let f = root.join(path);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

/// A repository with three local rules: a grep rule whose examples hold, an ast-grep rule with a
/// wrong `no_match` example, and a check through a tool this machine lacks.
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let ok = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(d.path())
        .status()
        .unwrap()
        .success();
    assert!(ok);
    put(
        d.path(),
        ".forge/rules/no-todo.yaml",
        r#"id: acme/no-todo
description: "No TODO markers in the config."
scope: {language: any, files: ["config/*.ini"]}
enforce:
  - {target: grep, mode: forbid, pattern: "TODO"}
examples:
  match: ["key = 1 ; TODO\n"]
  no_match: ["key = 1\n"]
"#,
    );
    put(
        d.path(),
        ".forge/rules/no-print.yaml",
        r#"id: acme/no-print
description: "No print() calls: use the logger."
scope: {language: python}
enforce:
  - {target: ast-grep, mode: forbid, rule: {pattern: "print($$$A)"}}
examples:
  match: ["print('hi')\n"]
  no_match: ["log.info('hi')\n", "print('still a print')\n"]
"#,
    );
    // a local pack's rules are the repository's too
    put(
        d.path(),
        ".forge/packs/acme/pack.yaml",
        "id: acme\ndescription: Acme's conventions.\nlevel: project\ndetect: {always: true}\n",
    );
    put(
        d.path(),
        ".forge/packs/acme/rules/no-debug.yaml",
        r#"id: acme/no-debug
description: "No debug flag in the config."
scope: {language: any, files: ["config/*.ini"]}
enforce:
  - {target: grep, mode: forbid, pattern: "debug *= *true"}
examples:
  match: ["debug = true\n"]
  no_match: ["debug = false\n"]
"#,
    );
    put(
        d.path(),
        ".forge/rules/lint.yaml",
        r#"id: acme/lint
description: "The project passes ruff."
scope: {language: python}
enforce:
  - {target: check, stage: lint, run: "forge_do ruff lint {files}"}
examples:
  match: ["import os\n"]
"#,
    );
    d
}

fn forge_test(dir: &Path, home: &Path, args: &[&str], claude: bool) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_forge"));
    c.arg("test")
        .args(args)
        .current_dir(dir)
        .env("PATH", PATH)
        .env("FORGE_HOME", home)
        .env("FORGE_NO_DOWNLOAD", "1")
        .env_remove("VIRTUAL_ENV")
        .env_remove("FORGE_LAUNCHED_VERSION");
    if claude {
        c.env("CLAUDECODE", "1");
    } else {
        c.env_remove("CLAUDECODE");
    }
    c.output().unwrap()
}

#[test]
fn local_rules_examples_pass_fail_and_skip() {
    let d = repo();
    let home = tempfile::tempdir().unwrap();

    // structured results: the verdicts, and the skip's note
    let out = forge_test(d.path(), home.path(), &["--json"], false);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let lines: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let (summary, results) = lines.split_last().unwrap();
    let verdict = |rule: &str, kind: &str, n: u64| -> &Value {
        results
            .iter()
            .find(|r| r["rule"] == rule && r["kind"] == kind && r["index"] == n)
            .unwrap_or_else(|| panic!("{rule} {kind} {n} in {results:?}"))
    };
    assert_eq!(verdict("acme/no-todo", "match", 1)["verdict"], "pass");
    assert_eq!(verdict("acme/no-todo", "no_match", 1)["verdict"], "pass");
    assert_eq!(verdict("acme/no-print", "match", 1)["verdict"], "pass");
    assert_eq!(verdict("acme/no-print", "no_match", 1)["verdict"], "pass");
    let wrong = verdict("acme/no-print", "no_match", 2);
    assert_eq!(wrong["verdict"], "fail", "{wrong}");
    let skipped = verdict("acme/lint", "match", 1);
    assert_eq!(skipped["verdict"], "skip", "{skipped}");
    assert_eq!(skipped["note"]["rule"], "acme/lint", "{skipped}");
    assert!(skipped["note"]["kind"].is_string(), "{skipped}");
    assert_eq!(summary["failed"], 1, "{summary}");
    assert_eq!(summary["skipped"], 1, "{summary}");
    assert_eq!(summary["passed"], 6, "{summary}");
    assert_eq!(verdict("acme/no-debug", "match", 1)["verdict"], "pass");

    // for the person: one line per example, the skip with its reason
    let out = forge_test(d.path(), home.path(), &[], false);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("PASS acme/no-todo grep match example 1"),
        "{text}"
    );
    assert!(
        text.contains("FAIL acme/no-print ast-grep no_match example 2: matches"),
        "{text}"
    );
    assert!(
        text.contains("SKIP acme/lint check match example 1: "),
        "{text}"
    );
    assert!(
        text.contains("8 examples of 4 rules: 6 passed, 1 failed, 1 skipped"),
        "{text}"
    );

    // under Claude Code: the same verdicts and exit status, the skip without its note
    let out = forge_test(d.path(), home.path(), &[], true);
    assert_eq!(out.status.code(), Some(1));
    let agent = String::from_utf8_lossy(&out.stdout);
    assert!(
        agent.contains("FAIL acme/no-print ast-grep no_match example 2"),
        "{agent}"
    );
    assert!(
        agent
            .lines()
            .any(|l| l == "SKIP acme/lint check match example 1"),
        "{agent}"
    );
    assert!(agent.contains("6 passed, 1 failed, 1 skipped"), "{agent}");
    assert!(!agent.contains("ruff"), "a note reached Claude: {agent}");
    assert!(forge_core::internals::find(&agent).is_empty(), "{agent}");

    // fixed, every example passes (the skip is no failure)
    put(
        d.path(),
        ".forge/rules/no-print.yaml",
        &std::fs::read_to_string(d.path().join(".forge/rules/no-print.yaml"))
            .unwrap()
            .replace(", \"print('still a print')\\n\"", ""),
    );
    for claude in [false, true] {
        let out = forge_test(d.path(), home.path(), &[], claude);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
    }
}

#[test]
fn a_repository_without_local_rules_has_nothing_to_test() {
    let d = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(d.path())
        .status()
        .unwrap();
    let home = tempfile::tempdir().unwrap();
    let out = forge_test(d.path(), home.path(), &[], true);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("This repository has no local rules."),
        "{text}"
    );
}
