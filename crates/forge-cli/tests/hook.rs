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
    over.extend(check_budgets(&s));
    assert!(over.is_empty(), "over budget: {over:?}");
}

/// The tools time the last hook check of `phase` recorded (ms).
fn tools_ms(s: &Sandbox, phase: &str) -> f64 {
    let state = s.forge_home.join("state");
    let mut stack = vec![state];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .file_name()
                .is_some_and(|n| *n == *format!("timings.{phase}.json"))
            {
                let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
                return v["tools_ms"].as_f64().unwrap_or(0.0);
            }
        }
    }
    0.0
}

fn stats(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    (v[n / 2], v[(n * 99 / 100).min(n - 1)])
}

/// PostToolUse after an Edit of a Python file, and Stop: Forge's overhead (the process's wall
/// time less the project's tools) against its budget, the tools' time reported apart.
fn check_budgets(s: &Sandbox) -> Vec<String> {
    let mut over = Vec::new();
    let file = s.repo.join("app/main.py");
    let clean = std::fs::read_to_string(&file).unwrap();
    s.hook("user-prompt-submit", &json!({"prompt": "go"}));
    let edit = |i: usize| {
        std::fs::write(&file, format!("{clean}\nVALUE_{i} = {i}\n")).unwrap();
        json!({"hook_event_name": "PostToolUse", "tool_name": "Edit",
               "tool_input": {"file_path": file, "old_string": "x", "new_string": "y"},
               "tool_response": {"filePath": file}, "tool_use_id": format!("e{i}")})
    };
    let cases: [(&str, &str, usize, f64, f64); 2] = [
        (
            "post-tool-use (Edit, Python)",
            "post-edit",
            60,
            30.0,
            1000.0,
        ),
        ("stop", "stop", 15, 300.0, f64::INFINITY),
    ];
    for (name, phase, n, budget, total_budget) in cases {
        let (mut overhead, mut tools, mut total) = (vec![], vec![], vec![]);
        for i in 0..=n {
            let input = edit(i + 1000 * usize::from(phase == "stop"));
            let t = Instant::now();
            if phase == "stop" {
                s.hook("post-tool-use", &input);
                let t = Instant::now();
                s.hook(
                    "stop",
                    &json!({"hook_event_name": "Stop", "stop_hook_active": false}),
                );
                let wall = t.elapsed().as_secs_f64() * 1000.0;
                let tm = tools_ms(s, phase);
                if i > 0 {
                    total.push(wall);
                    tools.push(tm);
                    overhead.push(wall - tm);
                }
                continue;
            }
            s.hook("post-tool-use", &input);
            let wall = t.elapsed().as_secs_f64() * 1000.0;
            let tm = tools_ms(s, phase);
            // the first edit of a session may exceed the budget
            if i > 0 {
                total.push(wall);
                tools.push(tm);
                overhead.push(wall - tm);
            }
        }
        let (om, op) = stats(overhead);
        let (tm, tp) = stats(tools);
        let (wm, wp) = stats(total);
        println!(
            "{name}: Forge overhead median {om:.1} ms, p99 {op:.1} ms (budget {budget} ms); tools median {tm:.1} ms, p99 {tp:.1} ms; total median {wm:.1} ms, p99 {wp:.1} ms"
        );
        if op > budget || wm > total_budget {
            over.push(name.to_string());
        }
    }
    std::fs::write(&file, clean).unwrap();
    over
}
