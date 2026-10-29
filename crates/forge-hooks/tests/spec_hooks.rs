//! §8 and §9: hook behaviours on fixture inputs, in a temporary repository, with a fake checker
//! (the hooks' own logic; the checker's is §10's). Complements tests/events.rs.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use forge_core::{Finding, Note, NoteKind, Report};
use forge_home::{Home, RepoId};
use forge_hooks::{
    Checker, EVENTS, Env, Output, Runtime, StopOutcome, StopStatus, TestBlock, TestEditRule,
};
use forge_render::hooks::{PROJECT_HOOKS, USER_HOOKS};
use serde_json::{Value, json};

#[derive(Default)]
struct Fake {
    post: Report,
    stop: Option<StopOutcome>,
    panic: bool,
    asked_post: RefCell<Vec<Vec<String>>>,
    asked_stop: RefCell<Vec<Option<PathBuf>>>,
}

impl Checker for Fake {
    fn post_edit(&self, _: &Path, files: &[String], _: Option<&Path>) -> Report {
        assert!(!self.panic, "checker crashed");
        self.asked_post.borrow_mut().push(files.to_vec());
        self.post.clone()
    }

    fn stop(&self, _: &Path, snapshot: Option<&Path>) -> StopOutcome {
        self.asked_stop
            .borrow_mut()
            .push(snapshot.map(Path::to_path_buf));
        self.stop.clone().unwrap_or_else(StopOutcome::pass)
    }

    fn test_edit_rule(&self, _: &Path, path: &str) -> Option<TestEditRule> {
        path.starts_with("tests/").then(|| TestEditRule {
            id: "python/pytest/no-test-edits".into(),
            review: Some("Does the user's request require changing this test?".into()),
        })
    }

    fn test_blocks(&self, _: &str, source: &str) -> Vec<TestBlock> {
        let mut out: Vec<TestBlock> = Vec::new();
        for line in source.lines() {
            if let Some(rest) = line.strip_prefix("def ") {
                out.push(TestBlock {
                    key: rest.split('(').next().unwrap_or("").to_string(),
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

struct Sandbox {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    root: PathBuf,
    home: Home,
    user_home: PathBuf,
    env_file: PathBuf,
}

impl Sandbox {
    fn new(rules: &[&str]) -> Sandbox {
        Sandbox::with_files(
            rules,
            &[
                (
                    "pyproject.toml",
                    "[project]\nname = \"x\"\nversion = \"0\"\ndependencies = []\n",
                ),
                ("uv.lock", "version = 1\n"),
            ],
        )
    }

    fn with_files(rules: &[&str], files: &[(&str, &str)]) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("repo");
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("forge.lock"), lock(rules)).unwrap();
        for (p, c) in files {
            std::fs::write(root.join(p), c).unwrap();
        }
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
            base,
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

    fn run_rt(
        &self,
        checker: &dyn Checker,
        env: Env,
        notes: Vec<forge_launcher::Note>,
        event: &str,
        input: Value,
    ) -> Output {
        let rt = Runtime {
            home: self.home.clone(),
            env,
            checker,
            exe: PathBuf::from("/nonexistent/forge"),
            launcher_notes: notes,
        };
        let mut input = input;
        if input.get("session_id").is_none() {
            input["session_id"] = json!("s1");
        }
        if input.get("cwd").is_none() {
            input["cwd"] = json!(self.root);
        }
        let out = forge_hooks::run(&rt, event, input.to_string().as_bytes());
        // what the agent reads: everything but the systemMessage, which is the person's
        let agent_stdout = match serde_json::from_str::<Value>(&out.stdout) {
            Ok(mut v) => {
                if let Some(o) = v.as_object_mut() {
                    o.remove("systemMessage");
                }
                v.to_string()
            }
            Err(_) => out.stdout.clone(),
        };
        for text in [&agent_stdout, &out.stderr] {
            let found = forge_core::internals::find(text);
            assert!(found.is_empty(), "internals {found:?} in {text}");
        }
        out
    }

    fn run_env(&self, checker: &dyn Checker, env: Env, event: &str, input: Value) -> Output {
        self.run_rt(checker, env, Vec::new(), event, input)
    }

    fn run(&self, checker: &dyn Checker, event: &str, input: Value) -> Output {
        self.run_env(checker, self.env(), event, input)
    }

    fn state(&self) -> PathBuf {
        self.home
            .repo_state_dir(&RepoId::from_physical_path(&self.root))
    }

    fn session(&self) -> PathBuf {
        self.state().join("sessions/s1")
    }

    fn notes(&self) -> Vec<Note> {
        forge_hooks::state::read_notes(&self.state())
    }

    /// A fake `claude` that records its arguments and environment and gives `decision`.
    fn reviewer(&self, decision: &str) -> (PathBuf, PathBuf) {
        let claude = self.base.join("claude");
        let log = self.base.join("claude.args");
        std::fs::write(
            &claude,
            format!(
                "#!/bin/sh\n{{ echo \"FORGE_REVIEWER=$FORGE_REVIEWER\"; for a in \"$@\"; do printf '<%s>\\n' \"$a\"; done; echo ----; }} >> {}\necho '{{\"type\":\"result\",\"structured_output\":{{\"decision\":\"{decision}\",\"reason\":\"It is routine.\"}}}}'\n",
                log.display()
            ),
        )
        .unwrap();
        sh(&self.base, "chmod +x claude");
        (claude, log)
    }
}

fn json_out(out: &Output) -> Value {
    serde_json::from_str(&out.stdout).unwrap_or_else(|e| panic!("{e}: {:?}", out.stdout))
}

fn decision(out: &Output) -> (String, String) {
    let v = json_out(out);
    let h = &v["hookSpecificOutput"];
    (
        h["permissionDecision"].as_str().unwrap_or("").to_string(),
        h["permissionDecisionReason"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    )
}

fn bash(cmd: &str, id: &str) -> Value {
    json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}, "tool_use_id": id})
}

fn post_bash(cmd: &str, id: &str) -> Value {
    json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}, "tool_use_id": id})
}

fn invocations(log: &Path) -> usize {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .matches("----")
        .count()
}

// ---- §8.1 the hook entries

#[test]
fn hook_entries_wire_every_event() {
    let row = |groups: &[forge_render::hooks::HookGroup]| -> Vec<String> {
        groups
            .iter()
            .flat_map(|g| {
                g.commands.iter().map(move |c| {
                    format!(
                        "{} {} {} {}{}{}",
                        g.event,
                        g.matcher.unwrap_or("-"),
                        c.event,
                        c.timeout,
                        if c.block_on_failure { " block" } else { "" },
                        if c.async_rewake { " rewake" } else { "" }
                    )
                })
            })
            .collect()
    };
    assert_eq!(
        row(PROJECT_HOOKS),
        [
            "SessionStart startup|resume|clear|compact session-start 10",
            "UserPromptSubmit - user-prompt-submit 10",
            "PreToolUse Bash pre-tool-use 180 block",
            "PreToolUse Read|Edit|Write|MultiEdit|NotebookEdit|Grep|Glob pre-tool-use 180",
            "PostToolUse Edit|Write|MultiEdit|NotebookEdit|Bash post-tool-use 60",
            "PostToolUse Edit|Write|MultiEdit|NotebookEdit|Bash post-tool-use-typecheck 300 rewake",
            "PostToolUseFailure Bash post-tool-use-failure 60",
            "PostToolUseFailure Bash post-tool-use-typecheck 300 rewake",
            "Stop - stop 600",
            "PermissionRequest - permission-request 180",
            "SessionEnd - session-end 10",
        ]
    );
    assert_eq!(
        row(USER_HOOKS),
        [
            "PreToolUse Bash user-pre-tool-use 180 block",
            "PreToolUse Read|Edit|Write|MultiEdit|NotebookEdit|Grep|Glob user-pre-tool-use 180",
            "PermissionRequest - user-permission-request 180",
            "Notification permission_prompt|idle_prompt user-notification 5",
        ]
    );
    // every rendered event is one forge hook answers, and the other way round
    let mut rendered: Vec<&str> = PROJECT_HOOKS
        .iter()
        .chain(USER_HOOKS)
        .flat_map(|g| g.commands.iter().map(|c| c.event))
        .collect();
    rendered.sort();
    rendered.dedup();
    let mut events = EVENTS.to_vec();
    events.sort();
    assert_eq!(rendered, events);
}

// ---- §8.2 SessionStart

#[test]
fn session_start_shows_a_missing_pinned_version_to_the_person_only() {
    let s = Sandbox::new(&[]);
    let note = forge_launcher::Note::VersionUnavailable {
        locked: "0.7.0".into(),
        running: "0.6.0".into(),
        fetching: true,
        fetch_error: None,
    };
    let out = s.run_rt(
        &Fake::default(),
        s.env(),
        vec![note.clone()],
        "session-start",
        json!({"source": "startup"}),
    );
    let v = json_out(&out);
    assert!(
        v["systemMessage"]
            .as_str()
            .unwrap()
            .contains("This project pins Forge 0.7.0")
    );
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        !ctx.contains("0.7.0"),
        "the agent's context stays repository facts"
    );
    // outside a project too
    let elsewhere = s.base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let out = s.run_rt(
        &Fake::default(),
        s.env(),
        vec![note],
        "session-start",
        json!({"cwd": elsewhere}),
    );
    assert!(json_out(&out).get("hookSpecificOutput").is_none());
    assert!(json_out(&out)["systemMessage"].is_string());
}

#[test]
fn session_start_puts_uv_on_path_only_for_uv_or_unpackaged_projects() {
    let poetry = Sandbox::with_files(
        &[],
        &[
            (
                "pyproject.toml",
                "[tool.poetry]\nname = \"x\"\n[tool.poetry.dependencies]\npython = \"^3.12\"\nrequests = \"*\"\n",
            ),
            ("poetry.lock", ""),
        ],
    );
    poetry.run(
        &Fake::default(),
        "session-start",
        json!({"source": "startup"}),
    );
    assert!(
        !poetry.env_file.exists(),
        "a Poetry project keeps its own tools"
    );

    let scripts = Sandbox::with_files(&[], &[]);
    scripts.run(
        &Fake::default(),
        "session-start",
        json!({"source": "startup"}),
    );
    let env = std::fs::read_to_string(&scripts.env_file).unwrap();
    assert!(
        env.contains(&scripts.home.path_dir().display().to_string()),
        "{env}"
    );
}

// ---- §8.3 state

#[test]
fn state_lives_outside_the_repository_readable_by_the_user_only() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new(&[]);
    s.run(
        &Fake::default(),
        "user-prompt-submit",
        json!({"prompt": "go"}),
    );
    assert!(!s.session().starts_with(&s.root));
    for d in [s.state(), s.session(), s.session().join("turn")] {
        let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{}", d.display());
    }
    assert_eq!(
        std::fs::read_to_string(s.session().join("prompt")).unwrap(),
        "go"
    );
    // nothing was written into the repository
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&s.root)
        .output()
        .unwrap();
    assert!(out.stdout.is_empty());
}

#[test]
fn notes_keep_the_fifty_newest() {
    let t = tempfile::tempdir().unwrap();
    let notes: Vec<Note> = (0..55)
        .map(|i| Note::new(NoteKind::Environment, format!("note {i}")))
        .collect();
    forge_hooks::state::record_notes(t.path(), &notes[..30]);
    forge_hooks::state::record_notes(t.path(), &notes[30..]);
    let kept = forge_hooks::state::read_notes(t.path());
    assert_eq!(kept.len(), 50);
    assert_eq!(kept[0].text, "note 5");
    assert_eq!(kept[49].text, "note 54");
}

// ---- §8.4 Bash snapshots

#[test]
fn a_snapshot_past_its_time_is_skipped_and_left_to_stop() {
    let s = Sandbox::new(&[]);
    let f = Fake::default();
    let slow = Env {
        snapshot_cap: Duration::ZERO,
        ..s.env()
    };
    let out = s.run_env(
        &f,
        slow.clone(),
        "pre-tool-use",
        bash("echo hi > app/new.py", "k1"),
    );
    assert_eq!(out, Output::silent());
    let tree = s.session().join("bash/k1.tree");
    assert_eq!(std::fs::read_to_string(&tree).unwrap(), "skipped\n");
    std::fs::write(s.root.join("app/new.py"), "hi\n").unwrap();
    let out = s.run_env(
        &f,
        slow.clone(),
        "post-tool-use",
        post_bash("echo hi > app/new.py", "k1"),
    );
    assert_eq!(out, Output::silent());
    assert!(f.asked_post.borrow().is_empty(), "no edit-time check");
    assert!(!tree.exists());
    // the person is told once, however many commands
    s.run_env(&f, slow, "pre-tool-use", bash("ls", "k2"));
    let notes: Vec<Note> = s
        .notes()
        .into_iter()
        .filter(|n| n.text.contains("could not be snapshotted within 0s"))
        .collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].kind, NoteKind::Environment);
}

#[test]
fn bash_changes_leave_the_users_index_alone_and_skip_tool_directories() {
    let s = Sandbox::new(&[]);
    let f = Fake::default();
    let index = std::fs::read(s.root.join(".git/index")).unwrap();
    s.run(&f, "pre-tool-use", bash("make", "m1"));
    for (p, c) in [
        ("app/a.py", "a = 1\n"),
        ("node_modules/x/index.js", "x\n"),
        (".venv/lib/x.py", "x\n"),
        (".claude/notes.md", "x\n"),
        ("pkg/node_modules/y.js", "y\n"),
    ] {
        let f = s.root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    std::fs::remove_file(s.root.join("tests/test_cart.py")).unwrap();
    s.run(&f, "post-tool-use", post_bash("make", "m1"));
    assert_eq!(
        f.asked_post.borrow().as_slice(),
        [vec!["app/a.py".to_string()]]
    );
    assert_eq!(std::fs::read(s.root.join(".git/index")).unwrap(), index);
    // the list is kept for the typecheck hook
    assert_eq!(
        std::fs::read_to_string(s.session().join("bash/m1.files")).unwrap(),
        "app/a.py\n"
    );
    // a command that changes nothing: no check, nothing printed
    let quiet = Fake::default();
    s.run(&quiet, "pre-tool-use", bash("ls", "m2"));
    assert_eq!(
        s.run(&quiet, "post-tool-use", post_bash("ls", "m2")),
        Output::silent()
    );
    assert!(quiet.asked_post.borrow().is_empty());
}

#[test]
fn bash_snapshots_beyond_the_fifty_newest_are_dropped() {
    let s = Sandbox::new(&[]);
    let dir = s.session().join("bash");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..60 {
        std::fs::write(dir.join(format!("old{i}.tree")), "skipped\n").unwrap();
    }
    s.run(&Fake::default(), "pre-tool-use", bash("ls", "n1"));
    let left = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tree"))
        .count();
    assert!(left <= 51, "{left}");
    assert!(dir.join("n1.tree").is_file());
}

#[test]
fn a_checker_crash_is_silent_and_noted() {
    let s = Sandbox::new(&[]);
    let f = Fake {
        panic: true,
        ..Fake::default()
    };
    let edit = json!({"hook_event_name": "PostToolUse", "tool_name": "Edit", "tool_input": {"file_path": s.root.join("app/main.py")}});
    assert_eq!(s.run(&f, "post-tool-use", edit), Output::silent());
    assert!(
        s.notes()
            .iter()
            .any(|n| n.kind == NoteKind::ForgeFailure && n.text.contains("stopped unexpectedly"))
    );
}

// ---- §8.5 typecheck

#[test]
fn edit_typecheck_never_is_silent() {
    let s = Sandbox::new(&["python/typecheck"]);
    let env = Env {
        edit_typecheck: Some("never".into()),
        ..s.env()
    };
    let edit =
        json!({"tool_name": "Edit", "tool_input": {"file_path": s.root.join("app/main.py")}});
    assert_eq!(
        s.run_env(&Fake::default(), env, "post-tool-use-typecheck", edit),
        Output::silent()
    );
}

// ---- §8.6 Stop

fn failing(n: usize) -> Report {
    Report {
        findings: (0..n)
            .map(|i| {
                Finding::error("python/no-eval", format!("No eval() {i}."))
                    .at("app/main.py", Some(1))
            })
            .collect(),
        notes: vec![],
    }
}

#[test]
fn stop_remembers_a_passing_tree() {
    let s = Sandbox::new(&[]);
    let f = Fake::default();
    s.run(&f, "user-prompt-submit", json!({"prompt": "go"}));
    std::fs::write(s.root.join("app/main.py"), "x = 2\n").unwrap();
    assert_eq!(s.run(&f, "stop", json!({})), Output::silent());
    assert_eq!(s.run(&f, "stop", json!({})), Output::silent());
    assert_eq!(
        f.asked_stop.borrow().len(),
        1,
        "the passing tree is not checked again"
    );
    std::fs::write(s.root.join("app/main.py"), "x = 3\n").unwrap();
    s.run(&f, "stop", json!({}));
    assert_eq!(f.asked_stop.borrow().len(), 2);
}

#[test]
fn stop_without_a_turn_snapshot_checks_every_change() {
    let s = Sandbox::new(&[]);
    let f = Fake::default();
    std::fs::write(s.root.join("app/main.py"), "x = 2\n").unwrap();
    s.run(&f, "stop", json!({}));
    assert_eq!(f.asked_stop.borrow().as_slice(), [None]);
}

#[test]
fn a_commit_during_the_turn_is_part_of_it() {
    let s = Sandbox::new(&[]);
    let f = Fake::default();
    s.run(&f, "user-prompt-submit", json!({"prompt": "go"}));
    std::fs::write(s.root.join("app/main.py"), "x = 2\n").unwrap();
    sh(&s.root, "git commit -qam change");
    s.run(&f, "stop", json!({}));
    let asked = f.asked_stop.borrow();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].as_deref(),
        Some(s.session().join("turn").as_path())
    );
}

#[test]
fn the_block_reason_is_capped_and_a_new_prompt_resets_the_count() {
    let s = Sandbox::new(&[]);
    let f = Fake {
        stop: Some(StopOutcome {
            report: failing(60),
            status: StopStatus::Fail,
        }),
        ..Fake::default()
    };
    s.run(&f, "user-prompt-submit", json!({"prompt": "go"}));
    std::fs::write(s.root.join("app/main.py"), "x = eval('2')\n").unwrap();
    let v = json_out(&s.run(&f, "stop", json!({})));
    assert_eq!(v["decision"], "block");
    let reason = v["reason"].as_str().unwrap();
    assert!(reason.lines().count() <= 40 && reason.len() <= 6000);
    assert!(reason.starts_with("app/main.py:1 No eval() 0."));
    assert_eq!(
        std::fs::read_to_string(s.session().join("blocks")).unwrap(),
        "1"
    );
    // the same tree again: Claude saw the findings and left them; the stop goes through, the
    // checks are not re-run, and the person is told
    let again = s.run(&f, "stop", json!({"stop_hook_active": true}));
    assert_eq!(again.stdout, "");
    assert_eq!(f.asked_stop.borrow().len(), 1);
    // a new block in the same turn, then a new prompt resets the count
    std::fs::write(s.root.join("app/main.py"), "x = eval('3')\n").unwrap();
    assert_eq!(
        json_out(&s.run(&f, "stop", json!({"stop_hook_active": true})))["decision"],
        "block"
    );
    s.run(&f, "user-prompt-submit", json!({"prompt": "again"}));
    assert!(!s.session().join("blocks").exists());
}

#[test]
fn tests_failing_before_the_turn_too_block_once() {
    let s = Sandbox::new(&[]);
    let f = Fake {
        stop: Some(StopOutcome {
            report: failing(1),
            status: StopStatus::Unverified,
        }),
        ..Fake::default()
    };
    s.run(&f, "user-prompt-submit", json!({"prompt": "go"}));
    std::fs::write(s.root.join("app/main.py"), "x = 2\n").unwrap();
    assert_eq!(json_out(&s.run(&f, "stop", json!({})))["decision"], "block");
    assert_eq!(
        s.run(&f, "stop", json!({"stop_hook_active": true})),
        Output::silent()
    );
    assert!(
        s.notes()
            .iter()
            .any(|n| n.text.contains("could not be verified"))
    );
}

// ---- §9 guard

#[test]
fn the_user_guard_applies_in_any_directory() {
    let s = Sandbox::new(&[]);
    std::fs::write(
        s.user_home.join(".claude/forge-user.lock"),
        lock(&["safety/git-force-push"]),
    )
    .unwrap();
    let anywhere = s.base.join("anywhere");
    std::fs::create_dir_all(&anywhere).unwrap();
    let mut input = bash("git push --force", "u1");
    input["cwd"] = json!(anywhere);
    let out = s.run(&Fake::default(), "user-pre-tool-use", input);
    assert_eq!(decision(&out).0, "deny");
    // the project's own guard stays silent outside a project
    let mut input = bash("git push --force", "u2");
    input["cwd"] = json!(anywhere);
    assert_eq!(
        s.run(&Fake::default(), "pre-tool-use", input),
        Output::silent()
    );
}

// ---- §9.5 reviewer

#[test]
fn a_guard_ask_nobody_can_answer_is_reviewed() {
    let s = Sandbox::new(&["safety/git-force-with-lease"]);
    let (claude, log) = s.reviewer("allow");
    let f = Fake::default();
    let cmd = "git push --force-with-lease";
    // a person is there: the ask stands
    assert_eq!(
        decision(&s.run(&f, "pre-tool-use", bash(cmd, "r0"))).0,
        "ask"
    );
    for entrypoint in ["sdk-cli", "sdk-ts", "sdk-py", "claude-code-github-action"] {
        let env = Env {
            entrypoint: Some(entrypoint.into()),
            claude: claude.clone(),
            ..s.env()
        };
        let (d, reason) = decision(&s.run_env(&f, env, "pre-tool-use", bash(cmd, "r1")));
        assert_eq!(d, "allow", "{entrypoint}");
        assert_eq!(
            reason,
            "Reviewed by an independent session, nobody being there to answer: It is routine."
        );
    }
    // the same rule and input (whitespace aside) reuse the verdict within the session
    assert_eq!(invocations(&log), 1);
    let env = Env {
        entrypoint: Some("sdk-cli".into()),
        claude: claude.clone(),
        ..s.env()
    };
    s.run_env(
        &f,
        env.clone(),
        "pre-tool-use",
        bash("git push  --force-with-lease", "r2"),
    );
    assert_eq!(invocations(&log), 1);
    // another session asks again
    let mut other = bash(cmd, "r3");
    other["session_id"] = json!("s2");
    s.run_env(&f, env.clone(), "pre-tool-use", other);
    assert_eq!(invocations(&log), 2);
    // never, dontAsk mode and a review in progress: the ask stands
    let never = Env {
        ask_review: Some("never".into()),
        ..env.clone()
    };
    assert_eq!(
        decision(&s.run_env(&f, never, "pre-tool-use", bash(cmd, "r4"))).0,
        "ask"
    );
    let mut dont = bash(cmd, "r5");
    dont["permission_mode"] = json!("dontAsk");
    assert_eq!(
        decision(&s.run_env(&f, env.clone(), "pre-tool-use", dont)).0,
        "ask"
    );
    let inside = Env {
        in_review: true,
        ..env.clone()
    };
    assert_eq!(
        decision(&s.run_env(&f, inside, "pre-tool-use", bash(cmd, "r6"))).0,
        "ask"
    );
    // always: reviewed with a person there
    let always = Env {
        ask_review: Some("always".into()),
        claude: claude.clone(),
        ..s.env()
    };
    let mut fresh = bash(cmd, "r7");
    fresh["session_id"] = json!("s3");
    assert_eq!(
        decision(&s.run_env(&f, always, "pre-tool-use", fresh)).0,
        "allow"
    );
    // no claude to run: the ask stands
    let missing = Env {
        entrypoint: Some("sdk-cli".into()),
        ..s.env()
    };
    let mut fresh = bash(cmd, "r8");
    fresh["session_id"] = json!("s4");
    assert_eq!(
        decision(&s.run_env(&f, missing, "pre-tool-use", fresh)).0,
        "ask"
    );
    // the verdicts are logged in the state directory
    let logged = std::fs::read_to_string(s.state().join("reviews.log")).unwrap();
    assert!(
        logged.lines().all(|l| l.split('\t').count() == 6),
        "{logged}"
    );
    assert!(logged.contains("\ts1\tallow\tBash\t"));
}

#[test]
fn the_reviewer_runs_a_fresh_read_only_claude() {
    let s = Sandbox::new(&["safety/git-force-with-lease"]);
    let (claude, log) = s.reviewer("deny");
    let env = Env {
        entrypoint: Some("sdk-cli".into()),
        claude,
        review_model: Some("claude-haiku-x".into()),
        ..s.env()
    };
    let (d, reason) = decision(&s.run_env(
        &Fake::default(),
        env,
        "pre-tool-use",
        bash("git push --force-with-lease", "a1"),
    ));
    assert_eq!(d, "deny");
    assert!(reason.ends_with("It is routine."));
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(args.starts_with("FORGE_REVIEWER=1\n"), "{args}");
    let lines: Vec<&str> = args.lines().collect();
    let after = |flag: &str| -> &str {
        let i = lines
            .iter()
            .position(|l| *l == format!("<{flag}>"))
            .unwrap_or_else(|| panic!("{flag} missing: {args}"));
        lines[i + 1]
    };
    assert_eq!(after("--output-format"), "<json>");
    assert!(after("--json-schema").contains("\"decision\""));
    assert_eq!(after("--tools"), "<Read,Grep,Glob>");
    assert_eq!(after("--allowedTools"), "<Read,Grep,Glob>");
    assert_eq!(after("--permission-mode"), "<dontAsk>");
    assert_eq!(after("--max-turns"), "<12>");
    assert_eq!(after("--setting-sources"), "<>");
    assert_eq!(after("--settings"), "<{\"disableAllHooks\":true}>");
    assert_eq!(after("--model"), "<claude-haiku-x>");
    for flag in ["-p", "--no-session-persistence", "--strict-mcp-config"] {
        assert!(lines.contains(&format!("<{flag}>").as_str()), "{flag}");
    }
    // the prompt carries the rule's concern, not its id
    assert!(
        args.contains("Its concern: git push --force-with-lease still rewrites remote history")
    );
    assert!(!args.contains("safety/git-force-with-lease"));
}

#[test]
fn reviews_outside_a_project_are_logged_in_the_user_state() {
    let s = Sandbox::new(&[]);
    let (claude, _) = s.reviewer("allow");
    std::fs::write(s.user_home.join(".claude/forge-user.lock"), lock(&[])).unwrap();
    let env = Env {
        entrypoint: Some("sdk-cli".into()),
        claude,
        ..s.env()
    };
    let anywhere = s.base.join("anywhere");
    std::fs::create_dir_all(&anywhere).unwrap();
    let input = json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
        "tool_input": {"command": "ls"}, "cwd": anywhere});
    let out = s.run_env(&Fake::default(), env, "user-permission-request", input);
    assert_eq!(
        json_out(&out)["hookSpecificOutput"]["decision"]["behavior"],
        "allow"
    );
    let log = s.home.state_root().join("user/reviews.log");
    assert!(
        std::fs::read_to_string(log)
            .unwrap()
            .contains("\tallow\tBash\t")
    );
}

#[test]
fn a_test_edit_verdict_is_not_reused_once_the_request_changed() {
    let s = Sandbox::new(&[]);
    let (claude, log) = s.reviewer("allow");
    let env = Env {
        entrypoint: Some("sdk-cli".into()),
        claude,
        ..s.env()
    };
    let f = Fake::default();
    let edit = json!({"tool_name": "Edit", "tool_use_id": "e1", "tool_input": {
        "file_path": s.root.join("tests/test_cart.py"), "old_string": "== 100", "new_string": "== 90"}});
    s.run_env(
        &f,
        env.clone(),
        "user-prompt-submit",
        json!({"prompt": "the total is now 90"}),
    );
    let (d, _) = decision(&s.run_env(&f, env.clone(), "pre-tool-use", edit.clone()));
    assert_eq!(d, "allow");
    s.run_env(&f, env.clone(), "pre-tool-use", edit.clone());
    assert_eq!(invocations(&log), 1);
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(
        args.contains("<request>\nthe total is now 90\n</request>"),
        "{args}"
    );
    assert!(args.contains("Does the user's request require changing this test?"));
    s.run_env(
        &f,
        env.clone(),
        "user-prompt-submit",
        json!({"prompt": "refactor"}),
    );
    s.run_env(&f, env, "pre-tool-use", edit);
    assert_eq!(invocations(&log), 2);
}

// ---- §9.6 notifications

#[test]
fn notifications_use_osc_9_where_the_terminal_reads_it() {
    let s = Sandbox::new(&[]);
    for (term, want) in [
        (Some("iTerm.app"), "\u{1b}]9;Needs you\u{7}"),
        (Some("WezTerm"), "\u{1b}]9;Needs you\u{7}"),
        (
            Some("Apple_Terminal"),
            "\u{1b}]777;notify;Claude Code;Needs you\u{7}",
        ),
        (None, "\u{1b}]777;notify;Claude Code;Needs you\u{7}"),
    ] {
        let env = Env {
            term_program: term.map(str::to_owned),
            ..s.env()
        };
        let out = s.run_env(
            &Fake::default(),
            env,
            "user-notification",
            json!({"message": "Needs you"}),
        );
        assert_eq!(json_out(&out)["terminalSequence"], want, "{term:?}");
    }
    let out = s.run(
        &Fake::default(),
        "user-notification",
        json!({"message": " "}),
    );
    assert_eq!(
        json_out(&out)["terminalSequence"],
        "\u{1b}]777;notify;Claude Code;Claude Code needs your attention\u{7}"
    );
}
