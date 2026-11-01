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
        Sandbox::with(sample, |_| {})
    }

    /// The sample, changed by `setup` before `forge apply`.
    fn with(sample: &str, setup: impl FnOnce(&Path)) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let repo = base.join(sample);
        copy_dir(&repo_root().join("fixtures/samples").join(sample), &repo);
        setup(&repo);
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
        // the person's commands (apply, status) run outside Claude Code; hooks run either way
        if args.first() != Some(&"hook") {
            c.env_remove("CLAUDECODE");
        }
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

/// Packaging created during a session activates packs the rules files written at apply time
/// do not cover: the next PostToolUse gives the agent their context, once.
#[test]
fn context_of_packs_activated_mid_session() {
    let s = Sandbox::new("astro-app");
    let out = s.hook("user-prompt-submit", &json!({"prompt": "add a script"}));
    assert!(out.status.success());
    // what `uv init --bare` and `uv add` leave, then a script
    std::fs::write(
        s.repo.join("pyproject.toml"),
        "[project]\nname = \"tools\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\ndependencies = [\"httpx>=0.27\"]\n",
    )
    .unwrap();
    std::fs::write(s.repo.join("uv.lock"), "version = 1\n").unwrap();
    std::fs::write(s.repo.join("fetch.py"), "import httpx\n").unwrap();
    let edit = json!({"hook_event_name": "PostToolUse", "tool_name": "Write", "tool_input": {"file_path": s.repo.join("fetch.py")}});
    let out = s.hook("post-tool-use", &edit);
    assert!(out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("This project now uses uv:\n"), "{ctx}");
    assert!(ctx.contains("uv add"), "{ctx}");
    // given once in the session
    let out = s.hook("post-tool-use", &edit);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("This project now uses"), "{text}");
}

/// A project that moves from Poetry to uv during the session: the guard follows the package
/// manager the repository has now, as the checks do.
#[test]
fn the_guard_follows_a_package_manager_switched_mid_session() {
    let s = Sandbox::with("fastapi-app", |repo| {
        std::fs::remove_file(repo.join("uv.lock")).unwrap();
        std::fs::write(repo.join("poetry.lock"), "# poetry\n").unwrap();
    });
    let reason = |cmd: &str, id: &str| s.decision(&bash(cmd, id)).map(|(_, r)| r);
    let r = reason("uv add httpx", "p1").unwrap();
    assert!(r.contains("poetry add"), "{r}");
    assert_eq!(reason("poetry install", "p2"), None);
    // the migration: poetry.lock gone, uv.lock and the [project] table
    std::fs::remove_file(s.repo.join("poetry.lock")).unwrap();
    std::fs::write(s.repo.join("uv.lock"), "version = 1\n").unwrap();
    let r = reason("uv add httpx", "p3").unwrap_or_default();
    assert!(!r.contains("poetry add"), "{r}");
    let r = reason("poetry install", "p4").unwrap();
    assert!(r.contains("uv add"), "{r}");
}

/// A plain-scripts project (no packaging) that gains a requirements file: the rules file written
/// at apply says it has no packaging; the next PostToolUse says once that this no longer applies.
#[test]
fn context_that_no_longer_holds_is_retracted_once() {
    let s = Sandbox::with("astro-app", |repo| {
        std::fs::write(repo.join("fetch.py"), "print(1)\n").unwrap();
    });
    let rules = std::fs::read_to_string(s.repo.join(".claude/rules/forge-python.md")).unwrap();
    assert!(rules.contains("declares no dependencies yet"), "{rules}");
    s.hook("user-prompt-submit", &json!({"prompt": "add a dependency"}));
    std::fs::write(s.repo.join("requirements.txt"), "httpx==0.27.2\n").unwrap();
    let edit = json!({"hook_event_name": "PostToolUse", "tool_name": "Write", "tool_input": {"file_path": s.repo.join("requirements.txt")}});
    let out = s.hook("post-tool-use", &edit);
    assert!(out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        ctx.contains("This no longer applies: This project declares no dependencies yet:"),
        "{ctx}"
    );
    assert!(ctx.contains("This project now uses pip:\n"), "{ctx}");
    // once in the session; the repository's rules file is left as apply wrote it
    let out = s.hook("post-tool-use", &edit);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("no longer applies"), "{text}");
    assert_eq!(
        std::fs::read_to_string(s.repo.join(".claude/rules/forge-python.md")).unwrap(),
        rules
    );
}

/// The edit-time relock (§8.7): an edit of pyproject.toml that leaves uv.lock stale runs `uv
/// lock` and wakes Claude; without a uv.lock, uv is not run.
#[test]
fn a_pyproject_edit_relocks_a_stale_uv_lock() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new("fastapi-app");
    // Forge's own uv (installed by apply) would come before the sandbox's bin on the PATH
    let _ = std::fs::remove_file(s.forge_home.join("path/uv"));
    let log = s.bin.join("uv.log");
    let uv = s.bin.join("uv");
    std::fs::write(
        &uv,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$2\" = --check ] && exit 1\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&uv, std::fs::Permissions::from_mode(0o755)).unwrap();
    let typecheck = |id: &str| {
        let mut c = s.cmd(&["hook", "post-tool-use-typecheck"]);
        c.env("FORGE_EDIT_TYPECHECK", "always")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        let input = json!({"session_id": "sess-1", "cwd": s.repo, "hook_event_name": "PostToolUse",
            "tool_name": "Edit", "tool_use_id": id,
            "tool_input": {"file_path": s.repo.join("pyproject.toml")}});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let pyproject = s.repo.join("pyproject.toml");
    let text = std::fs::read_to_string(&pyproject).unwrap();
    // a new dependency: not resolved into the lock after the edit (Stop checks it first)
    std::fs::write(
        &pyproject,
        text.replace(
            "\"pydantic>=2.9\",",
            "\"pydantic>=2.9\",\n  \"httpx>=0.27\",",
        ),
    )
    .unwrap();
    let out = typecheck("r0");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!log.exists());
    // a change that names no new package (a narrower range): relocked
    std::fs::write(
        &pyproject,
        text.replace("\"pydantic>=2.9\",", "\"pydantic>=2.10\","),
    )
    .unwrap();
    let out = typecheck("r1");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "uv.lock was updated to match pyproject.toml.\n"
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "lock --check\nlock --quiet\n"
    );
    // no uv.lock: nothing runs
    std::fs::remove_file(&log).unwrap();
    std::fs::remove_file(s.repo.join("uv.lock")).unwrap();
    let out = typecheck("r2");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    assert!(!log.exists());
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
    // a session's turns have a snapshot
    s.hook("user-prompt-submit", &json!({"prompt": "go"}));
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
            "test-edit lookup (Edit of a test)",
            "pre-tool-use",
            json!({"tool_name": "Edit", "tool_use_id": "b3", "tool_input": {
                "file_path": s.repo.join("tests/test_main.py"),
                "old_string": "def ", "new_string": "def "}}),
            10,
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
        // the overhead's budget is a warm median; a p99 past 1.5 times it is a regression too
        // (the tools' processes make single runs noisy)
        if om > budget || op > budget * 1.5 || wm > total_budget {
            over.push(name.to_string());
        }
    }
    std::fs::write(&file, clean).unwrap();
    over
}
