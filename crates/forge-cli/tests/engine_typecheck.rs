//! §8.4, §8.7, §15.3: a project whose only typed rule is a typecheck gets a types engine at
//! SessionStart, and the edit-time typecheck reports the error an edit added. Runs the real
//! pinned ty (through uvx, linked into the sandbox's Forge cache); skipped with a message when
//! uvx cannot provide it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const TY: &str = "0.0.86";

/// The pinned ty's executable, from uvx's environment for it.
fn ty() -> Option<PathBuf> {
    let out = Command::new("uvx")
        .args([
            "-q",
            "--from",
            &format!("ty=={TY}"),
            "python",
            "-c",
            "import os, sys; print(os.path.dirname(sys.executable))",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let ty = Path::new(String::from_utf8_lossy(&out.stdout).trim()).join("ty");
    ty.is_file().then_some(ty)
}

struct Sandbox {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    forge_home: PathBuf,
}

impl Sandbox {
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_claude-forge"));
        c.args(args)
            .current_dir(&self.repo)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", self.bin.display()),
            )
            .env("HOME", &self.home)
            .env("FORGE_HOME", &self.forge_home)
            .env("FORGE_NO_DOWNLOAD", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("FORGE_ENGINE")
            .env_remove("FORGE_EDIT_TYPECHECK")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("XDG_STATE_HOME")
            .env_remove("VIRTUAL_ENV")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_CODE_ENTRYPOINT")
            .env_remove("CLAUDE_ENV_FILE")
            .env_remove("FORGE_LAUNCHED_VERSION")
            .env_remove("CLAUDECODE");
        c
    }

    fn git(&self, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&self.repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn hook(&self, event: &str, input: Value) -> Output {
        let mut input = input;
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
        child.wait_with_output().unwrap()
    }
}

#[test]
fn a_typecheck_rule_alone_gets_an_engine_that_reports_an_added_error() {
    let Some(ty) = ty() else {
        eprintln!("skipped: uvx cannot provide ty {TY}");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let s = Sandbox {
        repo: base.join("repo"),
        bin: base.join("bin"),
        home: base.join("home"),
        forge_home: base.join("forge-home"),
        _tmp: tmp,
    };
    for d in [&s.repo, &s.bin, &s.home] {
        std::fs::create_dir_all(d).unwrap();
    }
    let claude = s.bin.join("claude");
    std::fs::write(&claude, "#!/bin/sh\necho \"2.1.295 (Claude Code)\"\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // the pinned ty, as `forge apply` would have installed it
    let dir = s.forge_home.join(format!("tools/ty/{TY}"));
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::os::unix::fs::symlink(&ty, dir.join("bin/ty")).unwrap();
    std::fs::write(dir.join(".forge-installed"), "").unwrap();

    std::fs::write(
        s.repo.join("pyproject.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\n",
    )
    .unwrap();
    std::fs::write(s.repo.join("main.py"), "total: int = 1\n").unwrap();
    s.git(&["init", "-q", "-b", "main"]);
    let out = s.cmd(&["apply", "--yes"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // only the typecheck asks for the engine: the rules with `where` are disabled
    let lock: toml::Table = std::fs::read_to_string(s.repo.join("forge.lock"))
        .unwrap()
        .parse()
        .unwrap();
    let rules = lock["rules"].as_table().unwrap();
    assert!(rules.contains_key("python/typecheck"), "{rules:?}");
    let packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
    let mut config = String::from("forge: \"0.6\"\ndisable:\n");
    for id in rules.keys() {
        let (pack, name) = id.rsplit_once('/').unwrap();
        let file = packs.join(pack).join("rules").join(format!("{name}.yaml"));
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if text.contains("where:") {
            config.push_str(&format!("  - {{id: {id}, reason: test}}\n"));
        }
    }
    assert!(
        config.contains("reason"),
        "no typed rule to disable: {rules:?}"
    );
    std::fs::write(s.repo.join("forge.yaml"), config).unwrap();
    let out = s.cmd(&["apply", "--yes"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    s.git(&["add", "-A"]);
    s.git(&["commit", "-qm", "app"]);

    let out = s.hook("session-start", json!({"source": "startup"}));
    assert!(out.status.success());
    std::fs::write(s.repo.join("main.py"), "total: int = \"one\"\n").unwrap();
    let edit = json!({"hook_event_name": "PostToolUse", "tool_name": "Edit",
        "tool_input": {"file_path": s.repo.join("main.py")}, "tool_use_id": "e1"});
    // the engine starts in the background: the typecheck is silent until it answers
    let until = Instant::now() + Duration::from_secs(60);
    let out = loop {
        let out = s.hook("post-tool-use-typecheck", edit.clone());
        if out.status.code() == Some(2) || Instant::now() > until {
            break out;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let _ = s.hook("session-end", json!({"reason": "exit"}));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.starts_with("This edit added type errors:"),
        "{stderr}"
    );
    assert!(stderr.contains("main.py:1:"), "{stderr}");
    assert!(stderr.contains("invalid-assignment"), "{stderr}");
}
