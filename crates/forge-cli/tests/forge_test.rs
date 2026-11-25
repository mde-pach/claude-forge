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

/// A repository with one local capability and its examples.
fn capability_repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(d.path())
            .status()
            .unwrap()
            .success()
    );
    put(
        d.path(),
        ".forge/capabilities/web-browser.yaml",
        r#"id: acme/web-browser
description: A persistent web browser, helpful to simulate a real user or do testing.
relevant:
  - The task may involve something a person sees or does in a web page.
deliver: inject
text: Check pages in the browser.
examples:
  selected: ["The login page shows a blank screen after I submit the form; fix it."]
  not_selected: ["Bump the version in pyproject.toml to 1.2.0."]
"#,
    );
    d
}

/// This machine's consent and key, as `forge apply --user` records them.
fn consent(home: &Path, key: &str) {
    let dir = home.join("state/user/capabilities");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("consent"), "1\n").unwrap();
    std::fs::write(dir.join("key"), format!("{key}\n")).unwrap();
}

/// A fake capability engine: a statement holds (0.9) when the request names a page, else 0.1.
fn fake_engine() -> String {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            let mut req = Vec::new();
            let mut buf = [0u8; 16384];
            let text = loop {
                let Ok(n) = s.read(&mut buf) else {
                    break String::new();
                };
                if n == 0 {
                    break String::new();
                }
                req.extend_from_slice(&buf[..n]);
                let t = String::from_utf8_lossy(&req).into_owned();
                if let Some(i) = t.find("\r\n\r\n") {
                    let len = t[..i]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if req.len() >= i + 4 + len {
                        break t[i + 4..].to_string();
                    }
                }
            };
            let v: Value = serde_json::from_str(&text).unwrap_or_default();
            let page = v["state"]["user_message"]
                .as_str()
                .unwrap_or("")
                .contains("page");
            let answers: serde_json::Map<String, Value> = v["questions"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, _)| {
                    (
                        k.clone(),
                        serde_json::json!({"type": "noul", "noul": if page { 0.9 } else { 0.1 }}),
                    )
                })
                .collect();
            let body = serde_json::json!({"model": "jev-1.14.0", "answers": answers, "usage": {}})
                .to_string();
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

// §20.8: a capability's examples run on this machine's capability engine: skipped while it is off
// here, passing when the engine agrees with each list.
#[test]
fn capability_examples_run_on_this_machines_engine() {
    let d = capability_repo();
    let home = tempfile::tempdir().unwrap();
    let out = forge_test(d.path(), home.path(), &[], false);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("SKIP acme/web-browser capability match example 1: the capability engine is off on this machine"), "{text}");
    assert!(
        text.contains("2 examples of 0 rules and 1 capabilities: 0 passed, 0 failed, 2 skipped"),
        "{text}"
    );
    consent(home.path(), "sk-test");
    let url = fake_engine();
    let mut c = Command::new(env!("CARGO_BIN_EXE_forge"));
    let out = c
        .args(["test", "--json"])
        .current_dir(d.path())
        .env("PATH", PATH)
        .env("FORGE_HOME", home.path())
        .env("FORGE_NO_DOWNLOAD", "1")
        .env("FORGE_CAPABILITIES_URL", &url)
        .env_remove("CLAUDECODE")
        .env_remove("FORGE_LAUNCHED_VERSION")
        .output()
        .unwrap();
    let lines: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let (summary, results) = lines.split_last().unwrap();
    assert_eq!(summary["passed"], 2, "{lines:?}");
    assert_eq!(summary["capabilities"], 1, "{summary}");
    assert!(
        results.iter().all(|r| r["target"] == "capability"),
        "{results:?}"
    );
}

// The same examples against TypeSafe's real engine: opt-in (FORGE_TEST_JEV=1, with
// TYPESAFE_API_KEY), and in the weekly upstream job.
#[test]
fn capability_examples_against_the_real_engine() {
    if std::env::var_os("FORGE_TEST_JEV").is_none() {
        eprintln!(
            "skipped: set FORGE_TEST_JEV=1 and TYPESAFE_API_KEY to run against the real engine"
        );
        return;
    }
    let Some(key) = std::env::var("TYPESAFE_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    else {
        eprintln!(
            "skipped: FORGE_TEST_JEV is set but TYPESAFE_API_KEY is not (the repository's secret)"
        );
        return;
    };
    let d = capability_repo();
    let home = tempfile::tempdir().unwrap();
    consent(home.path(), &key);
    let out = forge_test(d.path(), home.path(), &[], false);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("2 passed, 0 failed, 0 skipped"), "{text}");
}

// §6.5: for a check, advice catches a `match` example; a `no_match` example fails only on a
// finding that fails the check, so one that gets advice (advice, not a failure) passes.
#[test]
fn advice_catches_a_match_and_does_not_fail_a_no_match() {
    let d = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(d.path())
            .status()
            .unwrap()
            .success()
    );
    let rule = |id: &str, run: &str| {
        format!(
            r#"id: acme/{id}
description: "Prefer the new settings to the legacy ones."
scope: {{language: any, files: ["config/*.ini"]}}
enforce:
  - {{target: check, stage: lint, run: "{run}"}}
examples:
  match:
    - files: {{config/app.ini: "legacy = 1\n"}}
  no_match:
    - files: {{config/app.ini: "legacy = 1\nmodern = 1\n"}}
"#
        )
    };
    let check = "! grep -Hn legacy config/app.ini";
    put(
        d.path(),
        ".forge/rules/advised.yaml",
        &rule("advised", &format!("forge_advise sh -c '{check}'")),
    );
    put(
        d.path(),
        ".forge/rules/blocking.yaml",
        &rule("blocking", check),
    );
    let home = tempfile::tempdir().unwrap();
    let out = forge_test(d.path(), home.path(), &["--json"], false);
    let lines: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let verdict = |rule: &str, kind: &str| {
        lines
            .iter()
            .find(|r| r["rule"] == rule && r["kind"] == kind)
            .unwrap_or_else(|| panic!("{rule} {kind} in {lines:?}"))["verdict"]
            .clone()
    };
    assert_eq!(verdict("acme/advised", "match"), "pass", "{lines:?}");
    assert_eq!(verdict("acme/advised", "no_match"), "pass", "{lines:?}");
    assert_eq!(verdict("acme/blocking", "match"), "pass", "{lines:?}");
    assert_eq!(verdict("acme/blocking", "no_match"), "fail", "{lines:?}");
}
