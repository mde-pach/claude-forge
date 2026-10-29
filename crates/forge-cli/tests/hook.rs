//! `forge hook <event>` as Claude Code runs it: one process per call, on a generator sample after
//! `forge apply`. The budget tests (timing-sensitive) are ignored by default; run them on a quiet
//! machine with a release build:
//!
//! ```sh
//! FORGE_BUDGETS=1 cargo test --release -p forge-cli --test hook -- --ignored --nocapture
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

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

struct Sandbox {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    forge_home: PathBuf,
}

impl Sandbox {
    fn new(sample: &str) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let repo = base.join(sample);
        copy_dir(&repo_root().join("tests/generator").join(sample), &repo);
        let bin = base.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        std::fs::write(&claude, "#!/bin/sh\necho \"2.1.295 (Claude Code)\"\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let s = Sandbox {
            _tmp: tmp,
            repo,
            bin,
            home,
            forge_home: base.join("forge-home"),
        };
        s.git(&["init", "-q"]);
        s.git(&["add", "-A"]);
        s.git(&["commit", "-qm", "sample"]);
        let out = s.cmd(&["apply", "--yes"]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        s.git(&["add", "-A"]);
        s.git(&["commit", "-qm", "forge"]);
        s
    }

    fn git(&self, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(&self.repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_claude-forge"));
        c.args(args)
            .current_dir(&self.repo)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("HOME", &self.home)
            .env("FORGE_HOME", &self.forge_home)
            .env_remove("XDG_CACHE_HOME")
            .env_remove("XDG_STATE_HOME")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_CODE_ENTRYPOINT")
            .env_remove("CLAUDE_ENV_FILE")
            .env_remove("FORGE_LAUNCHED_VERSION")
            .env("FORGE_ENGINE", "off")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    }

    fn hook(&self, event: &str, input: &Value) -> Output {
        let mut input = input.clone();
        input["session_id"] = json!("sess-1");
        input["cwd"] = json!(self.repo);
        let mut child = self
            .cmd(&["hook", event])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        for text in [&out.stdout, &out.stderr] {
            let text = String::from_utf8_lossy(text);
            assert!(
                forge_core::internals::find(&text).is_empty(),
                "{event}: internals in {text}"
            );
        }
        out
    }

    fn decision(&self, input: &Value) -> Option<(String, String)> {
        let out = self.hook("pre-tool-use", input);
        assert!(out.status.success());
        if out.stdout.is_empty() {
            return None;
        }
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        let h = &v["hookSpecificOutput"];
        Some((
            h["permissionDecision"].as_str()?.to_string(),
            h["permissionDecisionReason"].as_str()?.to_string(),
        ))
    }
}

fn bash(cmd: &str, id: &str) -> Value {
    json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}, "tool_use_id": id})
}

#[test]
fn hooks_on_an_applied_sample() {
    let s = Sandbox::new("fastapi-app");
    let (d, reason) = s.decision(&bash("pip install requests", "t1")).unwrap();
    assert_eq!(d, "deny");
    assert!(reason.contains("uv add"), "{reason}");
    assert_eq!(
        s.decision(&bash("pip install --dry-run requests", "t2")),
        None
    );
    assert_eq!(
        s.decision(&bash("git commit -m 'remove chmod 777'", "t3")),
        None
    );
    let cache = s.forge_home.join("bin/forge-0.1.0");
    let (d, reason) = s
        .decision(&json!({"tool_name": "Read", "tool_input": {"file_path": cache}}))
        .unwrap();
    assert_eq!(d, "deny");
    assert_eq!(reason, forge_hooks::INTERNAL_REASON);
    // a prompt prints nothing; a session start gives the repository context
    let out = s.hook("user-prompt-submit", &json!({"prompt": "hi"}));
    assert!(out.status.success() && out.stdout.is_empty());
    let out = s.hook("session-start", &json!({"source": "startup"}));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("Last commits:"), "{ctx}");
    // an unknown event and garbage input are silent successes
    let out = s
        .cmd(&["hook", "pre-tool-use"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success() && out.stdout.is_empty());
}

/// Runs `event` `n` times; the median and p99 wall times of the whole process.
fn measure(s: &Sandbox, event: &str, input: &Value, n: usize) -> (Duration, Duration) {
    let mut times: Vec<Duration> = (0..n)
        .map(|_| {
            let t = Instant::now();
            let out = s.hook(event, input);
            assert!(out.status.success());
            t.elapsed()
        })
        .collect();
    times.sort();
    (times[n / 2], times[(n * 99 / 100).min(n - 1)])
}

#[test]
#[ignore = "timing: run with FORGE_BUDGETS=1 on a quiet machine, in release"]
fn budgets() {
    if std::env::var_os("FORGE_BUDGETS").is_none() {
        return;
    }
    let s = Sandbox::new("fastapi-app");
    // warm: the hook cache and the snapshot objects
    s.hook("session-start", &json!({"source": "startup"}));
    s.hook("pre-tool-use", &bash("ls", "warm"));
    let n = 120;
    let cases: &[(&str, &str, Value, u64)] = &[
        (
            "guard decision (deny)",
            "pre-tool-use",
            bash("pip install requests", "b1"),
            10,
        ),
        (
            "guard + snapshot",
            "pre-tool-use",
            bash("ls -la app", "b2"),
            50,
        ),
        (
            "internals refusal (Read)",
            "pre-tool-use",
            json!({"tool_name": "Read", "tool_input": {"file_path": s.repo.join("app/main.py")}}),
            10,
        ),
        (
            "user-prompt-submit",
            "user-prompt-submit",
            json!({"prompt": "go"}),
            50,
        ),
        (
            "session-start",
            "session-start",
            json!({"source": "startup"}),
            200,
        ),
    ];
    let mut over = Vec::new();
    for (name, event, input, budget) in cases {
        let (median, p99) = measure(&s, event, input, n);
        println!("{name}: median {median:?}, p99 {p99:?} (budget p99 {budget} ms)");
        if p99 > Duration::from_millis(*budget) {
            over.push(name.to_string());
        }
    }
    assert!(over.is_empty(), "over budget: {over:?}");
}
