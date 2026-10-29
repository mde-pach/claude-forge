//! End to end: a generator sample in a fresh repository, `claude-forge apply --yes`, then a
//! violation `claude-forge check --changed` must report (findings for the agent, notes for the
//! person apart), then the fix it must pass. Tools are kept off PATH and downloads off, so
//! what runs is Forge's own: the rules `apply` selected, run by the checker.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

struct Sample {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    forge_home: PathBuf,
}

impl Sample {
    fn new(name: &str) -> Sample {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join(name);
        copy_dir(&repo_root().join("tests/generator").join(name), &repo);
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        std::fs::write(&claude, "#!/bin/sh\necho \"2.1.295 (Claude Code)\"\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let s = Sample {
            forge_home: tmp.path().join("forge-home"),
            _tmp: tmp,
            repo,
            bin,
            home,
        };
        s.git(&["init", "-q", "-b", "main"]);
        s.commit("sample");
        s
    }

    fn env(&self, c: &mut Command) {
        c.current_dir(&self.repo)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", self.bin.display()),
            )
            .env("HOME", &self.home)
            .env("FORGE_HOME", &self.forge_home)
            .env("FORGE_NO_DOWNLOAD", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("XDG_STATE_HOME")
            .env_remove("VIRTUAL_ENV")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("FORGE_LAUNCHED_VERSION")
            .env_remove("CLAUDECODE")
            .stdin(Stdio::null());
    }

    fn git(&self, args: &[&str]) {
        let mut c = Command::new("git");
        c.args([
            "-c",
            "user.name=forge",
            "-c",
            "user.email=forge@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "maintenance.auto=false",
        ])
        .args(args);
        self.env(&mut c);
        let out = c.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", msg, "--no-verify"]);
    }

    fn forge(&self, args: &[&str]) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_claude-forge"));
        c.args(args);
        self.env(&mut c);
        c.output().unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.repo.join(rel), text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.repo.join(rel)).unwrap()
    }

    /// `forge hook <event>` with `input` (session and cwd added) as Claude Code sends it; the
    /// JSON answer, None when it says nothing. Nothing it prints names Forge's internals.
    fn hook(&self, event: &str, input: serde_json::Value) -> Option<serde_json::Value> {
        use std::io::Write as _;
        let mut input = input;
        input["session_id"] = "sess-e2e".into();
        input["cwd"] = self.repo.to_string_lossy().into_owned().into();
        let mut c = Command::new(env!("CARGO_BIN_EXE_claude-forge"));
        c.args(["hook", event]);
        self.env(&mut c);
        c.env("FORGE_ENGINE", "off")
            .env_remove("CLAUDE_CODE_ENTRYPOINT")
            .env_remove("CLAUDE_ENV_FILE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{event}: {}", text(&out));
        let all = text(&out);
        assert!(
            forge_core::internals::find(&all).is_empty(),
            "{event} names internals: {all}"
        );
        (!out.stdout.is_empty()).then(|| serde_json::from_slice(&out.stdout).unwrap())
    }

    /// The agent writes `rel` (Claude Code's Write, then its PostToolUse hook): the context the
    /// hook adds, if any.
    fn agent_writes(&self, rel: &str, content: &str, id: &str) -> Option<String> {
        self.write(rel, content);
        let path = self.repo.join(rel).to_string_lossy().into_owned();
        let v = self.hook(
            "post-tool-use",
            serde_json::json!({
                "hook_event_name": "PostToolUse",
                "tool_name": "Write",
                "tool_input": {"file_path": path, "content": content},
                "tool_response": {"filePath": path, "success": true},
                "tool_use_id": id,
            }),
        )?;
        Some(
            v["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .to_string(),
        )
    }

    /// The agent stops: the block reason, None when it may stop.
    fn agent_stops(&self, continuing: bool) -> Option<String> {
        let v = self.hook(
            "stop",
            serde_json::json!({"hook_event_name": "Stop", "stop_hook_active": continuing}),
        )?;
        assert_eq!(v["decision"], "block", "{v}");
        Some(v["reason"].as_str().unwrap().to_string())
    }
}

/// A turn through the hooks, as Claude Code drives them: the prompt, a Write with a violation
/// (the edit's context names it, without the rule id), Stop blocked on it, the fix, Stop passing.
fn hook_turn(sample: &str, file: &str, bad: &str, rule: &str, agent_text: &str) {
    let s = Sample::new(sample);
    let out = s.forge(&["apply", "--yes"]);
    assert!(out.status.success(), "apply: {}", text(&out));
    s.commit("forge");
    assert_eq!(
        s.hook(
            "user-prompt-submit",
            serde_json::json!({"hook_event_name": "UserPromptSubmit", "prompt": "add a feature"}),
        ),
        None
    );
    let clean = s.read(file);
    let ctx = s
        .agent_writes(file, &format!("{clean}{bad}"), "w1")
        .unwrap_or_else(|| panic!("the edit's hook said nothing about {agent_text:?}"));
    assert!(ctx.contains(agent_text), "{ctx}");
    assert!(!ctx.contains(rule), "the agent's text names no rule: {ctx}");
    let reason = s.agent_stops(false).expect("Stop blocks on the violation");
    assert!(reason.contains(agent_text), "{reason}");
    assert!(!reason.contains(rule), "{reason}");
    assert_eq!(s.agent_writes(file, &clean, "w2"), None);
    assert_eq!(s.agent_stops(true), None, "Stop passes after the fix");
}

#[test]
fn fastapi_hook_turn() {
    hook_turn(
        "fastapi-app",
        "app/main.py",
        "app.add_middleware(CORSMiddleware, allow_origins=[\"*\"], allow_credentials=True)\n",
        "python/fastapi/cors-credentials-wildcard",
        "app/main.py:17 CORS never combines",
    );
}

#[test]
fn typescript_hook_turn() {
    hook_turn(
        "eslint-app",
        "src/index.ts",
        "export const run = (code: string): unknown => eval(code);\n",
        "typescript/no-eval",
        "src/index.ts:2 ",
    );
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// apply, a violation `check --changed` reports, its fix passing.
fn violation_then_fix(sample: &str, file: &str, bad: &str, rule: &str, agent_text: &str) {
    let s = Sample::new(sample);
    let out = s.forge(&["apply", "--yes"]);
    assert!(out.status.success(), "apply: {}", text(&out));
    assert!(s.repo.join("forge.lock").is_file());
    let lock = s.read("forge.lock");
    assert!(lock.contains("[tools]"), "{lock}");
    s.commit("forge");

    let clean = s.read(file);
    s.write(file, &format!("{clean}{bad}"));
    let out = s.forge(&["check", "--changed"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(1), "check: {}", text(&out));
    let finding = stdout
        .lines()
        .find(|l| l.contains(agent_text))
        .unwrap_or_else(|| panic!("no finding {agent_text:?} in:\n{stdout}"));
    // the person's text names the rule; notes come apart, after the findings
    assert!(finding.starts_with(rule), "{finding}");
    if let Some(at) = stdout.find("Notes:") {
        assert!(stdout.find(agent_text).unwrap() < at, "{stdout}");
    }
    // run by Claude Code (its Bash tool, or the pre-commit hook it triggers): only what the
    // agent can fix, without rule ids or notes
    let mut c = Command::new(env!("CARGO_BIN_EXE_claude-forge"));
    c.args(["check", "--changed"]);
    s.env(&mut c);
    let out = c.env("CLAUDECODE", "1").output().unwrap();
    let agent_out = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(1), "{agent_out}");
    assert!(agent_out.contains(agent_text), "{agent_out}");
    assert!(!agent_out.contains(rule), "{agent_out}");
    assert!(!agent_out.contains("Notes:"), "{agent_out}");
    assert!(forge_core::internals::find(&agent_out).is_empty(), "{agent_out}");
    let out = s.forge(&["check", "--changed", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let findings = v["findings"].as_array().unwrap();
    let f = findings
        .iter()
        .find(|f| f["rule"] == rule)
        .unwrap_or_else(|| panic!("{v:#}"));
    let agent = f["agent_text"].as_str().unwrap();
    assert!(agent.contains(agent_text), "{agent}");
    assert!(
        !agent.contains(rule),
        "the agent's text names no rule: {agent}"
    );
    // notes are the person's: never in what the agent reads
    for n in v["notes"].as_array().unwrap() {
        let n = n["text"].as_str().unwrap();
        assert!(
            findings
                .iter()
                .all(|f| !f["agent_text"].as_str().unwrap().contains(n)),
            "{n}"
        );
    }

    s.write(file, &clean);
    let out = s.forge(&["check", "--changed"]);
    assert_eq!(out.status.code(), Some(0), "after the fix: {}", text(&out));
}

#[test]
fn fastapi_violation_fails_and_its_fix_passes() {
    violation_then_fix(
        "fastapi-app",
        "app/main.py",
        "app.add_middleware(CORSMiddleware, allow_origins=[\"*\"], allow_credentials=True)\n",
        "python/fastapi/cors-credentials-wildcard",
        "app/main.py:17 CORS never combines",
    );
}

#[test]
fn typescript_violation_fails_and_its_fix_passes() {
    violation_then_fix(
        "eslint-app",
        "src/index.ts",
        "export const run = (code: string): unknown => eval(code);\n",
        "typescript/no-eval",
        "src/index.ts:2 ",
    );
}

#[test]
fn status_reports_tools_and_pin_differences() {
    let s = Sample::new("fastapi-app");
    let out = s.forge(&["apply", "--yes"]);
    assert!(out.status.success(), "apply: {}", text(&out));
    let applied = text(&out);
    assert!(applied.contains("missing tools"), "{applied}");
    // a lock written by another Forge pins other versions
    let lock = s.read("forge.lock");
    let line = lock
        .lines()
        .find(|l| l.starts_with("uv = "))
        .expect("uv pin")
        .to_string();
    s.write("forge.lock", &lock.replace(&line, "uv = \"0.0.1\""));
    let out = s.forge(&["status"]);
    let st = text(&out);
    assert!(st.contains("Tool pins (forge.lock"), "{st}");
    assert!(st.contains("uv: 0.0.1 -> "), "{st}");
    assert!(st.contains("Missing tools:\n  ruff"), "{st}");
    let out = s.forge(&["status", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["tool_pins"][0]["tool"], "uv");
}
