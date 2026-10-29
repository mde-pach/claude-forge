//! Every hook event on fixture inputs, in a temporary repository, with a fake checker.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use forge_core::{Finding, Note, NoteKind, Report};
use forge_home::{Home, RepoId};
use forge_hooks::{
    Checker, Env, INTERNAL_REASON, Output, Runtime, StopOutcome, StopStatus, TestBlock,
    TestEditRule,
};
use serde_json::{Value, json};

/// What the fake checker answers, and what it was asked.
#[derive(Default)]
struct Fake {
    post: Report,
    stop: Option<StopOutcome>,
    asked_post: RefCell<Vec<Vec<String>>>,
    asked_stop: RefCell<u32>,
}

impl Checker for Fake {
    fn post_edit(&self, _: &Path, files: &[String], _: Option<&Path>) -> Report {
        self.asked_post.borrow_mut().push(files.to_vec());
        self.post.clone()
    }

    fn stop(&self, _: &Path, _: Option<&Path>) -> StopOutcome {
        *self.asked_stop.borrow_mut() += 1;
        self.stop.clone().unwrap_or_else(StopOutcome::pass)
    }

    fn test_edit_rule(&self, _: &Path, path: &str) -> Option<TestEditRule> {
        path.starts_with("tests/").then(|| TestEditRule {
            id: "python/pytest/no-test-edits".into(),
            review: None,
        })
    }

    /// Python tests: each `def test_…` up to the next top-level line.
    fn test_blocks(&self, _: &str, source: &str) -> Vec<TestBlock> {
        let mut out: Vec<TestBlock> = Vec::new();
        for line in source.lines() {
            if let Some(rest) = line.strip_prefix("def ") {
                let name = rest.split('(').next().unwrap_or("").to_string();
                out.push(TestBlock {
                    key: name,
                    print: String::new(),
                });
            } else if line.starts_with(' ') {
                if let Some(b) = out.last_mut() {
                    b.print.push_str(line.trim());
                }
            }
        }
        out
    }
}

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: Home,
    user_home: PathBuf,
    env_file: PathBuf,
}

fn sh(dir: &Path, script: &str) {
    let out = Command::new("sh")
        .args(["-c", script])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{script}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn lock(rules: &[&str]) -> String {
    let mut s = String::from(
        "forge = \"0.6.0\"\nclaude_code_min = \"2.1.295\"\nrule_set = \"test\"\n\n[rules]\n",
    );
    for r in rules {
        s.push_str(&format!("\"{r}\" = \"h\"\n"));
    }
    s
}

impl Sandbox {
    fn new(rules: &[&str]) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("repo");
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("forge.lock"), lock(rules)).unwrap();
        std::fs::write(
            root.join("pyproject.toml"),
            "[project]\nname = \"x\"\nversion = \"0\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::write(root.join("uv.lock"), "version = 1\n").unwrap();
        std::fs::write(root.join("app/main.py"), "x = 1\n").unwrap();
        std::fs::write(
            root.join("tests/test_cart.py"),
            "def test_total():\n    assert total([40, 60]) == 100\n",
        )
        .unwrap();
        sh(
            &root,
            "git init -q && git add -A && git commit -qm 'first commit'",
        );
        let user_home = base.join("home");
        std::fs::create_dir_all(user_home.join(".claude")).unwrap();
        let home = Home::new(base.join("cache"), base.join("state"));
        Sandbox {
            env_file: base.join("env-file"),
            _tmp: tmp,
            root,
            home,
            user_home,
        }
    }

    fn env(&self) -> Env {
        Env {
            claude: PathBuf::from("/nonexistent/claude"),
            home_dir: Some(self.user_home.clone()),
            env_file: Some(self.env_file.clone()),
            snapshot_cap: Duration::from_secs(10),
            ..Env::default()
        }
    }

    fn run_env(&self, checker: &dyn Checker, env: Env, event: &str, input: Value) -> Output {
        let rt = Runtime {
            home: self.home.clone(),
            env,
            checker,
            exe: PathBuf::from("/nonexistent/forge"),
            launcher_notes: Vec::new(),
        };
        let mut input = input;
        input["session_id"] = json!("s1");
        if input.get("cwd").is_none() {
            input["cwd"] = json!(self.root);
        }
        let out = forge_hooks::run(&rt, event, input.to_string().as_bytes());
        assert_agent_clean(&out);
        out
    }

    fn run(&self, checker: &dyn Checker, event: &str, input: Value) -> Output {
        self.run_env(checker, self.env(), event, input)
    }

    fn state(&self) -> PathBuf {
        self.home
            .repo_state_dir(&RepoId::from_physical_path(&self.root))
    }

    fn notes(&self) -> Vec<Note> {
        forge_hooks::state::read_notes(&self.state())
    }
}

/// What the agent reads names no internals.
fn assert_agent_clean(out: &Output) {
    for text in [&out.stdout, &out.stderr] {
        let found = forge_core::internals::find(text);
        assert!(found.is_empty(), "internals {found:?} in {text}");
    }
}

fn json_out(out: &Output) -> Value {
    serde_json::from_str(&out.stdout).unwrap_or_else(|e| panic!("{e}: {:?}", out.stdout))
}

fn bash(cmd: &str, id: &str) -> Value {
    json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}, "tool_use_id": id})
}

#[test]
fn pip_install_in_a_uv_project_is_denied_with_uv_add() {
    let s = Sandbox::new(&["python/uv/no-pip"]);
    let f = Fake::default();
    let out = s.run(&f, "pre-tool-use", bash("pip install requests", "t1"));
    let v = json_out(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("uv add"), "{reason}");
    assert!(!reason.contains("python/uv"), "no rule id: {reason}");
    // read-only pip commands pass, and their snapshot is taken
    for cmd in ["pip install --dry-run requests", "pip list", "pip show x"] {
        let out = s.run(&f, "pre-tool-use", bash(cmd, "t2"));
        assert_eq!(out.stdout, "", "{cmd}");
    }
}

#[test]
fn user_guard_reads_commands_not_messages() {
    let s = Sandbox::new(&[]);
    std::fs::write(
        s.user_home.join(".claude/forge-user.lock"),
        lock(&["safety/chmod-777", "safety/git-force-push"]),
    )
    .unwrap();
    let f = Fake::default();
    for cmd in [
        "git commit -m \"remove chmod 777\"",
        "gh pr create --body \"never git push --force\"",
        "echo chmod 777 > notes.txt",
    ] {
        let out = s.run(&f, "user-pre-tool-use", bash(cmd, "u1"));
        assert_eq!(out.stdout, "", "{cmd}");
    }
    let out = s.run(&f, "user-pre-tool-use", bash("chmod 777 app", "u2"));
    assert_eq!(
        json_out(&out)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
}

#[test]
fn forge_files_are_refused_neutrally() {
    let s = Sandbox::new(&[]);
    let f = Fake::default();
    let cache_file = s.home.cache_root().join("bin/forge-0.6.0");
    let state_dir = s.state();
    let cases = [
        json!({"tool_name": "Read", "tool_input": {"file_path": cache_file}}),
        json!({"tool_name": "Grep", "tool_input": {"pattern": "x", "path": state_dir}}),
        json!({"tool_name": "Glob", "tool_input": {"pattern": format!("{}/**", s.home.cache_root().display())}}),
        json!({"tool_name": "Edit", "tool_input": {"file_path": state_dir.join("sessions/s1/blocks"), "old_string": "1", "new_string": "0"}}),
        bash(&format!("cat {}", cache_file.display()), "b1"),
        bash(&format!("ls {} | head", state_dir.display()), "b2"),
    ];
    for c in cases {
        let out = s.run(&f, "pre-tool-use", c.clone());
        let v = json_out(&out);
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny", "{c}");
        assert_eq!(
            v["hookSpecificOutput"]["permissionDecisionReason"],
            INTERNAL_REASON
        );
    }
    let ok = s.run(
        &f,
        "pre-tool-use",
        json!({"tool_name": "Read", "tool_input": {"file_path": s.root.join("app/main.py")}}),
    );
    assert_eq!(ok.stdout, "");
}

fn failing(rule: &str, msg: &str) -> Report {
    Report {
        findings: vec![Finding::error(rule, msg).at("app/main.py", Some(1))],
        notes: vec![Note::new(NoteKind::MissingTool, "ruff is not installed")],
    }
}

#[test]
fn stop_blocks_on_findings_not_on_notes() {
    let s = Sandbox::new(&[]);
    s.run(
        &Fake::default(),
        "user-prompt-submit",
        json!({"prompt": "fix it"}),
    );
    // nothing changed during the turn: no check
    let f = Fake {
        stop: Some(StopOutcome {
            report: failing("python/no-eval", "No eval()."),
            status: StopStatus::Fail,
        }),
        ..Fake::default()
    };
    assert_eq!(s.run(&f, "stop", json!({})).stdout, "");
    assert_eq!(*f.asked_stop.borrow(), 0);

    std::fs::write(s.root.join("app/main.py"), "x = eval('1')\n").unwrap();
    let out = s.run(&f, "stop", json!({}));
    let v = json_out(&out);
    assert_eq!(v["decision"], "block");
    assert_eq!(v["reason"], "app/main.py:1 No eval().");
    assert!(!out.stdout.contains("ruff"), "notes stay out");
    assert!(
        s.notes()
            .iter()
            .any(|n| n.text.contains("ruff is not installed"))
    );
    // the same tree: the reason again, the checks not re-run
    let out = s.run(&f, "stop", json!({"stop_hook_active": true}));
    assert!(
        json_out(&out)["reason"]
            .as_str()
            .unwrap()
            .contains("No eval().")
    );
    assert_eq!(*f.asked_stop.borrow(), 1);
    // after three blocks in a turn the stop goes through, with a note for the person
    s.run(&f, "stop", json!({"stop_hook_active": true}));
    let out = s.run(&f, "stop", json!({"stop_hook_active": true}));
    assert_eq!(out.stdout, "");
    assert!(s.notes().iter().any(|n| n.text.contains("3 times")));

    // a failure with notes only (Forge's own) never blocks
    std::fs::write(s.root.join("app/main.py"), "x = 2\n").unwrap();
    let notes_only = Fake {
        stop: Some(StopOutcome {
            report: Report {
                findings: vec![],
                notes: vec![Note::new(
                    NoteKind::ForgeFailure,
                    "the engine did not start",
                )],
            },
            status: StopStatus::Fail,
        }),
        ..Fake::default()
    };
    assert_eq!(s.run(&notes_only, "stop", json!({})).stdout, "");
}

#[test]
fn advice_blocks_once() {
    let s = Sandbox::new(&[]);
    s.run(
        &Fake::default(),
        "user-prompt-submit",
        json!({"prompt": "go"}),
    );
    std::fs::write(s.root.join("app/new.py"), "y = 1\n").unwrap();
    let f = Fake {
        stop: Some(StopOutcome {
            report: Report {
                findings: vec![Finding::advice("r", "Add a test.")],
                notes: vec![],
            },
            status: StopStatus::AdviceOnly,
        }),
        ..Fake::default()
    };
    let v = json_out(&s.run(&f, "stop", json!({})));
    assert_eq!(v["reason"], "Advice: Add a test.");
    assert_eq!(
        s.run(&f, "stop", json!({"stop_hook_active": true})).stdout,
        ""
    );
}

#[test]
fn post_tool_use_reports_findings_on_the_changed_files() {
    let s = Sandbox::new(&[]);
    let quiet = Fake::default();
    let edit = json!({"hook_event_name": "PostToolUse", "tool_name": "Edit", "tool_input": {"file_path": s.root.join("app/main.py")}});
    assert_eq!(s.run(&quiet, "post-tool-use", edit.clone()).stdout, "");
    assert_eq!(quiet.asked_post.borrow()[0], ["app/main.py"]);

    let f = Fake {
        post: failing("python/no-eval", "No eval()."),
        ..Fake::default()
    };
    let v = json_out(&s.run(&f, "post-tool-use", edit));
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    assert_eq!(
        v["hookSpecificOutput"]["additionalContext"],
        "app/main.py:1 No eval()."
    );

    // Bash: the files the command changed, from the snapshots around it
    let pre = s.run(
        &quiet,
        "pre-tool-use",
        bash("echo hi > app/made.py", "bash1"),
    );
    assert_eq!(pre.stdout, "");
    std::fs::write(s.root.join("app/made.py"), "hi\n").unwrap();
    std::fs::write(s.root.join("app/main.py"), "x = 3\n").unwrap();
    let mut post = bash("echo hi > app/made.py", "bash1");
    post["hook_event_name"] = json!("PostToolUseFailure");
    let out = s.run(&f, "post-tool-use-failure", post);
    assert_eq!(
        json_out(&out)["hookSpecificOutput"]["hookEventName"],
        "PostToolUseFailure"
    );
    assert_eq!(
        f.asked_post.borrow().last().unwrap(),
        &["app/made.py", "app/main.py"]
    );
}

#[test]
fn prompt_snapshots_the_turn_silently() {
    let s = Sandbox::new(&[]);
    std::fs::write(s.root.join("app/main.py"), "x = 9\n").unwrap();
    let out = s.run(
        &Fake::default(),
        "user-prompt-submit",
        json!({"prompt": "do it"}),
    );
    assert_eq!(out, Output::silent());
    let turn = s.state().join("sessions/s1/turn");
    assert!(
        std::fs::read_to_string(turn.join("dirty"))
            .unwrap()
            .starts_with("app/main.py\t")
    );
    assert_eq!(
        std::fs::read_to_string(turn.join("files/app/main.py")).unwrap(),
        "x = 9\n"
    );
}

#[test]
fn session_start_gives_repository_context_only() {
    let s = Sandbox::new(&["python/uv/no-pip"]);
    std::fs::write(s.root.join("app/main.py"), "x = 2\n").unwrap();
    let out = s.run(
        &Fake::default(),
        "session-start",
        json!({"source": "startup"}),
    );
    let v = json_out(&out);
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("1 changed file:"), "{ctx}");
    assert!(ctx.contains("app/main.py"));
    assert!(ctx.contains("first commit"));
    assert!(ctx.lines().count() <= 20);
    assert!(
        !ctx.to_lowercase().contains("forge") && !ctx.contains("pack"),
        "{ctx}"
    );
    assert!(v.get("systemMessage").is_none());
    // uv project: Forge's uv on the agent's PATH
    let env = std::fs::read_to_string(&s.env_file).unwrap();
    assert!(env.contains(&s.home.path_dir().display().to_string()));
}

#[test]
fn test_edits_ask() {
    let s = Sandbox::new(&[]);
    let f = Fake::default();
    s.run(&f, "user-prompt-submit", json!({"prompt": "fix total"}));
    let file = s.root.join("tests/test_cart.py");
    let change = json!({"tool_name": "Edit", "tool_use_id": "e1", "tool_input": {
        "file_path": file, "old_string": "== 100", "new_string": "== 90"}});
    let out = s.run(&f, "pre-tool-use", change.clone());
    let v = json_out(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "ask");
    let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("\"test_total\" (changed)"), "{reason}");
    assert!(reason.contains("change the code instead"));
    // adding a test is fine
    let add = json!({"tool_name": "Edit", "tool_input": {"file_path": file,
        "old_string": "== 100\n", "new_string": "== 100\n\n\ndef test_empty():\n    assert total([]) == 0\n"}});
    assert_eq!(s.run(&f, "pre-tool-use", add).stdout, "");
    // removing it by Write asks too
    let write = json!({"tool_name": "Write", "tool_input": {"file_path": file, "content": "\n"}});
    assert!(
        s.run(&f, "pre-tool-use", write)
            .stdout
            .contains("(removed)")
    );
    // the edit ran: approved for the Stop check
    std::fs::write(
        &file,
        "def test_total():\n    assert total([40, 60]) == 90\n",
    )
    .unwrap();
    let mut post = change;
    post["hook_event_name"] = json!("PostToolUse");
    s.run(&f, "post-tool-use", post);
    let approved =
        std::fs::read_to_string(s.state().join("sessions/s1/turn/tests-approved")).unwrap();
    assert_eq!(approved, "tests/test_cart.py\ttest_total\n");
}

#[test]
fn typecheck_reports_new_errors_through_the_engine() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    let s = Sandbox::new(&["python/typecheck"]);
    let socket = s.home.engine_socket(&RepoId::from_physical_path(&s.root));
    let addr = forge_engine::socket_addr(&socket);
    std::fs::create_dir_all(addr.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&addr).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(req["op"], "diagnostics");
        assert_eq!(req["files"][0]["path"], "app/main.py");
        assert!(req["files"][0]["base"].is_string());
        let resp = json!({"diagnostics": [{"path": "app/main.py", "line": 1, "column": 5,
            "severity": "error", "code": "invalid-assignment", "message": "bad"}]});
        (&stream).write_all(format!("{resp}\n").as_bytes()).unwrap();
    });
    std::fs::write(s.root.join("app/main.py"), "x: int = 'a'\n").unwrap();
    let edit =
        json!({"tool_name": "Edit", "tool_input": {"file_path": s.root.join("app/main.py")}});
    let out = s.run(&Fake::default(), "post-tool-use-typecheck", edit.clone());
    server.join().unwrap();
    assert_eq!(out.code, 2);
    assert!(
        out.stderr
            .contains("app/main.py:1:5: error[invalid-assignment] bad"),
        "{}",
        out.stderr
    );
    // under claude -p (no streaming input) it is the Stop check's
    let env = Env {
        entrypoint: Some("sdk-cli".into()),
        ..s.env()
    };
    let out = s.run_env(&Fake::default(), env, "post-tool-use-typecheck", edit);
    assert_eq!(out, Output::silent());
}

#[test]
fn reviewer_answers_asks_nobody_can() {
    let s = Sandbox::new(&[]);
    let claude = s.root.parent().unwrap().join("claude");
    std::fs::write(
        &claude,
        "#!/bin/sh\necho '{\"type\":\"result\",\"structured_output\":{\"decision\":\"deny\",\"reason\":\"It deletes data.\"}}'\n",
    )
    .unwrap();
    sh(s.root.parent().unwrap(), "chmod +x claude");
    let env = Env {
        entrypoint: Some("sdk-cli".into()),
        claude: claude.clone(),
        ..s.env()
    };
    let input = json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "rm -rf data"}});
    let out = s.run_env(
        &Fake::default(),
        env.clone(),
        "permission-request",
        input.clone(),
    );
    let v = json_out(&out);
    assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "deny");
    assert!(
        v["hookSpecificOutput"]["decision"]["message"]
            .as_str()
            .unwrap()
            .contains("It deletes data.")
    );
    assert!(s.state().join("reviews.log").is_file());
    // a person is there: nothing
    assert_eq!(
        s.run(&Fake::default(), "permission-request", input.clone())
            .stdout,
        ""
    );
    // the user-level setup reviews it once
    std::fs::write(s.user_home.join(".claude/forge-user.lock"), lock(&[])).unwrap();
    assert_eq!(
        s.run_env(
            &Fake::default(),
            env.clone(),
            "permission-request",
            input.clone()
        )
        .stdout,
        ""
    );
    assert!(
        s.run_env(&Fake::default(), env, "user-permission-request", input)
            .stdout
            .contains("\"deny\"")
    );
}

#[test]
fn notification_and_session_end() {
    let s = Sandbox::new(&[]);
    let out = s.run(
        &Fake::default(),
        "user-notification",
        json!({"message": "Claude needs\u{7} permission"}),
    );
    let v = json_out(&out);
    assert_eq!(
        v["terminalSequence"],
        "\u{1b}]777;notify;Claude Code;Claude needs permission\u{7}"
    );
    assert_eq!(
        s.run(&Fake::default(), "session-end", json!({})),
        Output::silent()
    );
    assert_eq!(
        s.run(&Fake::default(), "no-such-event", json!({})),
        Output::silent()
    );
}

#[test]
fn session_start_starts_the_engine_for_typed_rules() {
    let s = Sandbox::new(&["python/typecheck"]);
    let base = s.root.parent().unwrap().to_path_buf();
    let exe = base.join("fake-forge");
    let log = base.join("started");
    std::fs::write(
        &exe,
        format!("#!/bin/sh\necho \"$@\" > {}\n", log.display()),
    )
    .unwrap();
    sh(&base, "chmod +x fake-forge");
    let fake = Fake::default();
    let rt = Runtime {
        home: s.home.clone(),
        env: Env {
            engine: true,
            ..s.env()
        },
        checker: &fake,
        exe,
        launcher_notes: Vec::new(),
    };
    let input = json!({"session_id": "s1", "cwd": s.root, "source": "startup"});
    let out = forge_hooks::run(&rt, "session-start", input.to_string().as_bytes());
    assert_agent_clean(&out);
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while !log.is_file() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(50));
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(args.starts_with("engine start --root"), "{args}");
    assert!(args.contains("--session s1 --langs python"), "{args}");
}

#[test]
fn outside_a_project_hooks_are_silent() {
    let s = Sandbox::new(&["python/uv/no-pip"]);
    let elsewhere = s.root.parent().unwrap().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    for ev in [
        "pre-tool-use",
        "stop",
        "post-tool-use",
        "user-prompt-submit",
        "session-start",
    ] {
        let mut i = bash("pip install x", "o1");
        i["cwd"] = json!(elsewhere);
        assert_eq!(s.run(&Fake::default(), ev, i), Output::silent(), "{ev}");
    }
}
