//! §3 Forge CLI, §4 ownership, §7 generated output, §12 lock: the setup commands on small
//! repositories. Hermetic: a fake `claude`, private HOME and FORGE_HOME, no downloads.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "maintenance.auto=false"])
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

#[cfg(unix)]
fn exe(p: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A small Python project in a fresh git repository.
const PY: &[(&str, &str)] = &[
    (
        "pyproject.toml",
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\ndependencies = []\n",
    ),
    ("app/main.py", "def main() -> None:\n    print(1)\n"),
];

struct Sandbox {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    forge_home: PathBuf,
    /// PATH without the fake claude (only git).
    no_claude: Option<PathBuf>,
}

impl Sandbox {
    fn new(files: &[(&str, &str)]) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for (p, t) in files {
            let f = repo.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, t).unwrap();
        }
        git(&repo, &["init", "-q"]);
        let bin = tmp.path().join("bin");
        exe(
            &bin.join("claude"),
            "#!/bin/sh\necho \"2.1.300 (Claude Code)\"\n",
        );
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let forge_home = tmp.path().join("forge-home");
        Sandbox {
            _tmp: tmp,
            repo,
            bin,
            home,
            forge_home,
            no_claude: None,
        }
    }

    fn path(&self) -> String {
        match &self.no_claude {
            Some(p) => p.display().to_string(),
            None => format!(
                "{}:{}",
                self.bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        }
    }

    fn forge_in(&self, dir: &Path, args: &[&str]) -> Output {
        self.forge_env(dir, args, None)
    }

    /// `config_dir`: Claude Code's `CLAUDE_CONFIG_DIR`.
    fn forge_env(&self, dir: &Path, args: &[&str], config_dir: Option<&Path>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_forge"));
        match config_dir {
            Some(d) => cmd.env("CLAUDE_CONFIG_DIR", d),
            None => cmd.env_remove("CLAUDE_CONFIG_DIR"),
        };
        cmd.env_remove("CLAUDECODE");
        cmd.args(args)
            .current_dir(dir)
            .env("PATH", self.path())
            .env("HOME", &self.home)
            .env("FORGE_HOME", &self.forge_home)
            .env("FORGE_NO_DOWNLOAD", "1")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("XDG_STATE_HOME")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("FORGE_LAUNCHED_VERSION")
            .env_remove("VIRTUAL_ENV")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn forge(&self, args: &[&str]) -> Output {
        self.forge_in(&self.repo, args)
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.forge(args);
        assert!(
            out.status.success(),
            "forge {args:?} failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.repo.join(rel)).ok()
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.repo.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn exists(&self, rel: &str) -> bool {
        self.repo.join(rel).exists()
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn usage_errors_exit_2() {
    let s = Sandbox::new(PY);
    for args in [
        &["frobnicate"][..],
        &["apply", "--nope"],
        &["apply", "--user", "--local"],
        &["explain"],
    ] {
        assert_eq!(s.forge(args).status.code(), Some(2), "forge {args:?}");
    }
    let out = s.forge(&["init", "--user"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("init is project-level only"));
}

#[test]
fn apply_needs_a_git_repository_and_claude_code() {
    let s = Sandbox::new(PY);
    let outside = s.home.clone();
    let out = s.forge_in(&outside, &["apply", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out)
            .contains("forge: not inside a git repository (use --user for the user-level setup)"),
        "{}",
        stderr(&out)
    );

    // Claude Code absent: refused, nothing written
    let mut s = Sandbox::new(PY);
    let only_git = s.home.join("only-git");
    std::fs::create_dir_all(&only_git).unwrap();
    let real_git = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    std::os::unix::fs::symlink(real_git.trim(), only_git.join("git")).unwrap();
    s.no_claude = Some(only_git);
    let out = s.forge(&["apply", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("forge: Claude Code not found; forge needs claude >= 2.1.295"),
        "{}",
        stderr(&out)
    );
    assert!(!s.exists("forge.lock") && !s.exists(".claude"));
}

#[test]
fn shared_settings_need_yes_without_a_terminal_and_local_does_not() {
    let s = Sandbox::new(PY);
    let out = s.forge(&["apply"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("forge: apply these changes to .claude/settings.json? Re-run with --yes to confirm non-interactively."),
        "{}",
        stderr(&out)
    );
    assert!(!stderr(&out).contains("forge: forge:"), "{}", stderr(&out));
    assert!(!s.exists("forge.lock") && !s.exists(".claude/settings.json"));

    // --local: the entries go to settings.local.json, with no confirmation
    let out = s.ok(&["apply", "--local"]);
    assert!(out.contains("forge: applied"), "{out}");
    assert!(!s.exists(".claude/settings.json"));
    let local = s.read(".claude/settings.local.json").unwrap();
    assert!(local.contains("Edit(forge.yaml)"), "{local}");
    assert!(local.contains("claude-forge hook pre-tool-use"), "{local}");
    let lock = s.read("forge.lock").unwrap();
    assert!(
        lock.contains("file = \".claude/settings.local.json\""),
        "{lock}"
    );
}

#[test]
fn diff_drift_and_re_adding_entries() {
    let s = Sandbox::new(PY);
    // before any apply, diff shows what apply would write and exits 1
    let out = s.forge(&["diff"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("+++ forge.lock"), "{}", stdout(&out));
    s.ok(&["apply", "--yes"]);
    assert_eq!(s.forge(&["diff"]).status.code(), Some(0));

    // a settings entry removed by hand: diff and status report it, apply re-adds it
    let settings = s.read(".claude/settings.json").unwrap();
    s.write(
        ".claude/settings.json",
        &settings.replace("      \"Edit(forge.yaml)\",\n", ""),
    );
    let out = s.forge(&["diff"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("forge: .claude/settings.json: entry missing, apply re-adds it: permissions.ask \"Edit(forge.yaml)\""),
        "{}",
        stdout(&out)
    );
    let st = s.ok(&["status"]);
    assert!(
        st.contains("  .claude/settings.json: missing permissions.ask \"Edit(forge.yaml)\""),
        "{st}"
    );
    s.ok(&["apply", "--yes"]);
    assert!(
        s.read(".claude/settings.json")
            .unwrap()
            .contains("\"Edit(forge.yaml)\"")
    );
    assert!(s.ok(&["apply", "--yes"]).contains("forge: up to date"));

    // a deleted rules file is written again without --force
    let rules: Vec<String> = std::fs::read_dir(s.repo.join(".claude/rules"))
        .unwrap()
        .flatten()
        .map(|e| format!(".claude/rules/{}", e.file_name().to_string_lossy()))
        .collect();
    let victim = &rules[0];
    let text = s.read(victim).unwrap();
    std::fs::remove_file(s.repo.join(victim)).unwrap();
    assert_eq!(s.forge(&["diff"]).status.code(), Some(1));
    s.ok(&["apply", "--yes"]);
    assert_eq!(s.read(victim).unwrap(), text);
}

#[test]
fn files_the_current_plan_no_longer_writes_are_deleted() {
    let s = Sandbox::new(PY);
    s.ok(&["apply", "--yes"]);
    assert!(s.exists(".claude/rules/forge-python.md"));
    s.write("forge.yaml", "forge: \"0.6\"\npacks: {remove: [python]}\n");
    let out = stdout(&s.forge(&["diff"]));
    assert!(
        out.contains("--- .claude/rules/forge-python.md (deleted)"),
        "{out}"
    );
    s.ok(&["apply", "--yes"]);
    assert!(!s.exists(".claude/rules/forge-python.md"));
    let st = s.ok(&["status"]);
    assert!(
        st.contains("  python: not active (removed in forge.yaml)"),
        "{st}"
    );
    let lock = s.read("forge.lock").unwrap();
    assert!(!lock.contains("forge-python.md"), "{lock}");
}

#[test]
fn entries_already_in_settings_are_kept_unrecorded_and_survive_remove() {
    let s = Sandbox::new(PY);
    let mine =
        "{\n  // ours\n  \"permissions\": {\n    \"ask\": [\"Edit(forge.yaml)\"] // keep\n  }\n}\n";
    s.write(".claude/settings.json", mine);
    s.ok(&["apply", "--yes"]);
    let lock = s.read("forge.lock").unwrap();
    assert!(!lock.contains("value = \"Edit(forge.yaml)\""), "{lock}");
    assert!(lock.contains("value = \"Edit(forge.lock)\""), "{lock}");
    let settings = s.read(".claude/settings.json").unwrap();
    assert!(
        settings.contains("// ours") && settings.contains("// keep"),
        "{settings}"
    );
    assert_eq!(
        settings.matches("\"Edit(forge.yaml)\"").count(),
        1,
        "{settings}"
    );

    // remove --dry-run lists what it would delete and deletes nothing
    let out = s.ok(&["remove", "--dry-run"]);
    assert!(out.contains("--- forge.lock (deleted)"), "{out}");
    assert!(
        out.contains(".claude/rules/forge-python.md (deleted)"),
        "{out}"
    );
    assert!(s.exists("forge.lock") && s.exists(".claude/rules/forge-python.md"));

    let out = s.ok(&["remove", "--yes"]);
    assert!(out.contains("forge: removed "), "{out}");
    assert_eq!(s.read(".claude/settings.json").unwrap(), mine);
    assert!(!s.exists("forge.lock"));
    let out = s.forge(&["remove", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("forge: no forge.lock; nothing to remove"));
}

#[test]
fn a_settings_file_forge_created_is_deleted_by_remove() {
    let s = Sandbox::new(PY);
    s.ok(&["apply", "--yes"]);
    s.ok(&["remove", "--yes"]);
    assert!(!s.exists(".claude"), "remove leaves no empty .claude");
    assert!(!s.exists("forge.lock"));
}

#[test]
fn invalid_locks_and_settings_are_refused() {
    let s = Sandbox::new(PY);
    s.ok(&["apply", "--yes"]);
    let lock = s.read("forge.lock").unwrap();
    for (what, text) in [
        ("not TOML", "forge = \n".to_string()),
        (
            "an unknown key",
            format!("{lock}\nextra = 1\n").replace("[rules]", "extra_top = 1\n[rules]"),
        ),
        (
            "a path forge does not write",
            lock.replacen("[files]\n", "[files]\n\"src/app.py\" = \"00\"\n", 1),
        ),
    ] {
        s.write("forge.lock", &text);
        for cmd in [&["apply", "--yes"][..], &["status"], &["remove", "--yes"]] {
            let out = s.forge(cmd);
            assert_eq!(
                out.status.code(),
                Some(1),
                "{what}: forge {cmd:?}: {}",
                stdout(&out)
            );
            assert!(
                stderr(&out).contains("forge.lock"),
                "{what}: {}",
                stderr(&out)
            );
        }
        assert!(s.exists("app/main.py"));
    }
    s.write("forge.lock", &lock);
    s.write(".claude/settings.json", "{ not json");
    let out = s.forge(&["apply", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains(".claude/settings.json"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn forge_yaml_is_validated() {
    for (what, yaml, want) in [
        ("an unknown key", "forge: \"0.6\"\nextra: 1\n", "extra"),
        (
            "a disable without a reason",
            "forge: \"0.6\"\ndisable:\n  - id: python/no-eval\n",
            "reason",
        ),
        (
            "duplicate keys",
            "forge: \"0.6\"\nforge: \"0.6\"\n",
            "forge.yaml",
        ),
    ] {
        let s = Sandbox::new(PY);
        s.write("forge.yaml", yaml);
        let out = s.forge(&["apply", "--yes"]);
        assert_eq!(out.status.code(), Some(1), "{what}");
        assert!(stderr(&out).contains(want), "{what}: {}", stderr(&out));
        assert!(!s.exists("forge.lock"));
    }
}

#[test]
fn status_reports_packs_disabled_local_and_changed_rules() {
    let s = Sandbox::new(PY);
    s.write(
        "forge.yaml",
        "forge: \"0.6\"\ndisable:\n  - id: python/no-eval\n    reason: we sandbox eval ourselves\n",
    );
    let local = "id: python/no-print\ndescription: Our print rule.\nscope: {language: python}\nenforce:\n  - {target: grep, mode: forbid, pattern: \"print\\\\(\"}\nexamples:\n  match: [\"print(1)\"]\n  no_match: [\"log(1)\"]\n";
    s.write(".forge/rules/no-print.yaml", local);
    s.ok(&["apply", "--yes"]);
    let st = s.ok(&["status"]);
    assert!(st.contains("Packs:\n"), "{st}");
    assert!(st.contains("  python (detected)"), "{st}");
    assert!(
        st.contains("\nDisabled:\n  python/no-eval: we sandbox eval ourselves"),
        "{st}"
    );
    assert!(st.contains("  python/no-print (local)"), "{st}");
    assert!(st.contains("Drift: none"), "{st}");
    assert!(
        st.contains("CI step, the default branch: claude-forge check --all"),
        "{st}"
    );
    let out = s.ok(&["explain", "python/no-eval"]);
    assert!(
        out.contains("Disabled here: we sandbox eval ourselves"),
        "{out}"
    );
    let out = s.ok(&["explain", "python/no-print"]);
    assert!(out.starts_with("# .forge/rules/no-print.yaml\n"), "{out}");
    let out = s.forge(&["explain", "nope/nothing"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("forge: unknown rule nope/nothing"));
    let out = s.ok(&["explain", "docker/copy-not-add"]);
    assert!(
        out.contains("Not active here: pack docker is not active at project level."),
        "{out}"
    );

    // a rule whose source changed since the last apply
    s.write(
        ".forge/rules/no-print.yaml",
        &local.replace("Our print rule.", "Our print rule, v2."),
    );
    let out = s.forge(&["diff"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("forge: rule python/no-print changed since the last apply"),
        "{}",
        stdout(&out)
    );
    let st = s.ok(&["status"]);
    assert!(
        st.contains("  rule python/no-print changed since the last apply"),
        "{st}"
    );
    // forge.yaml and .forge/rules are inputs: apply never rewrites them
    s.ok(&["apply", "--yes"]);
    assert!(s.read(".forge/rules/no-print.yaml").unwrap().contains("v2"));
    assert!(
        s.read("forge.yaml")
            .unwrap()
            .contains("we sandbox eval ourselves")
    );
}

#[test]
fn status_json_keys_and_tool_versions() {
    let s = Sandbox::new(PY);
    // the project's ruff: one too old for any adapter range
    exe(
        &s.repo.join(".venv/bin/ruff"),
        "#!/bin/sh\necho 'ruff 0.0.200'\n",
    );
    s.write("ruff.toml", "line-length = 100\n");
    s.ok(&["apply", "--yes"]);
    let v: serde_json::Value = serde_json::from_str(&s.ok(&["status", "--json"])).unwrap();
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "forge_version",
            "level",
            "base",
            "applied",
            "packs",
            "inactive_packs",
            "counts",
            "rules",
            "disabled",
            "gated",
            "switched_off",
            "deprecated",
            "audits",
            "missing_tools",
            "tool_versions",
            "tool_pins",
            "pre_commit",
            "drift",
            "files",
            "notes",
            "warnings",
        ]
    );
    let versions: Vec<&str> = v["tool_versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert!(
        versions.iter().any(|l| l.starts_with(
            "ruff 0.0.200: no Forge adapter covers it (Forge drives ruff 0.1.4 and later): "
        ) && l.ends_with(" not enforced")),
        "{versions:?}\n{}",
        s.ok(&["status"])
    );
    let rules = v["rules"].as_array().unwrap();
    assert!(
        rules
            .iter()
            .all(|r| r.get("id").is_some() && r.get("enforced").is_some())
    );
    let st = s.ok(&["status"]);
    assert!(
        st.contains("Tool versions:\n  ruff 0.0.200: no Forge adapter covers it"),
        "{st}"
    );

    // a ruff newer than the adapter's proven releases is driven as the newest proven one
    exe(
        &s.repo.join(".venv/bin/ruff"),
        "#!/bin/sh\necho 'ruff 99.0.0'\n",
    );
    let st = s.ok(&["status"]);
    assert!(
        st.contains(
            "  ruff 99.0.0 is newer than the releases Forge's adapter was proven on (up to "
        ),
        "{st}"
    );
    let out = s.ok(&["apply", "--yes"]);
    assert!(
        out.contains("forge: tool versions (what they cannot do is not enforced):\n  ruff 99.0.0"),
        "{out}"
    );
}

#[test]
fn init_creates_missing_tool_configs_then_applies_once() {
    let s = Sandbox::new(PY);
    s.write("ruff.toml", "line-length = 120\n");
    let out = s.ok(&["init", "--yes"]);
    assert!(out.contains("forge: ruff.toml exists, kept"), "{out}");
    assert!(out.contains("forge: created "), "{out}");
    assert!(out.contains("forge: applied"), "{out}");
    assert_eq!(s.read("ruff.toml").unwrap(), "line-length = 120\n");
    assert!(s.exists("forge.lock"));
    let out = s.forge(&["init", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains(
            "forge: init is for new projects; this one already has forge (use forge apply)"
        ),
        "{}",
        stderr(&out)
    );
}

#[test]
fn user_level_setup() {
    let s = Sandbox::new(PY);
    let out = s.ok(&["apply", "--user", "--yes"]);
    assert!(out.contains("forge: applied"), "{out}");
    let settings = std::fs::read_to_string(s.home.join(".claude/settings.json")).unwrap();
    assert!(
        settings.contains("claude-forge hook user-pre-tool-use"),
        "{settings}"
    );
    let lock = std::fs::read_to_string(s.home.join(".claude/forge-user.lock")).unwrap();
    assert!(lock.starts_with("# Generated file: do not edit."), "{lock}");
    assert!(!s.exists("forge.lock"), "the repository is untouched");
    assert!(
        s.ok(&["apply", "--user", "--yes"])
            .contains("forge: up to date")
    );
    s.ok(&["remove", "--user", "--yes"]);
    assert!(!s.home.join(".claude/forge-user.lock").exists());
    assert!(!s.home.join(".claude/settings.json").exists());
}

/// Help and `--version` name the program as it was invoked.
#[test]
fn the_command_is_named_as_invoked() {
    for (exe, name) in [
        (env!("CARGO_BIN_EXE_forge"), "forge"),
        (env!("CARGO_BIN_EXE_claude-forge"), "claude-forge"),
    ] {
        let out = Command::new(exe)
            .arg("--version")
            .env_remove("CLAUDECODE")
            .output()
            .unwrap();
        let v = String::from_utf8_lossy(&out.stdout);
        assert!(v.starts_with(&format!("{name} ")), "{v}");
    }
}

/// The user level's sandbox network list is appended to the person's own (§7.2): their domains
/// stay, the registries' and GitHub's download hosts are added once, and remove takes back only
/// what Forge added.
#[test]
fn user_level_sandbox_domains_are_appended() {
    let s = Sandbox::new(PY);
    let settings = s.home.join(".claude/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(
        &settings,
        "{\"sandbox\": {\"network\": {\"allowedDomains\": [\"intranet.example.com\", \"github.com\"]}}}\n",
    )
    .unwrap();
    s.ok(&["apply", "--user", "--yes"]);
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    let domains: Vec<&str> = v["sandbox"]["network"]["allowedDomains"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d.as_str())
        .collect();
    for d in [
        "intranet.example.com",
        "registry.npmjs.org",
        "objects.githubusercontent.com",
        "release-assets.githubusercontent.com",
        "index.crates.io",
        "static.crates.io",
        "proxy.golang.org",
        "sum.golang.org",
    ] {
        assert!(domains.contains(&d), "{d} in {domains:?}");
    }
    assert_eq!(
        domains.iter().filter(|d| **d == "github.com").count(),
        1,
        "{domains:?}"
    );
    s.ok(&["remove", "--user", "--yes"]);
    let after = std::fs::read_to_string(&settings).unwrap();
    assert!(after.contains("intranet.example.com"), "{after}");
    assert!(
        after.contains("github.com"),
        "the person's own entry stays: {after}"
    );
    assert!(!after.contains("crates.io"), "{after}");
}

/// apply offers the uv setup a uv project lacks: the cooldown's missing keys in pyproject.toml,
/// and a uv.lock that matches it (`uv lock`).
#[test]
fn apply_adds_the_uv_cooldown() {
    // uv's mark: a [tool.uv] table, no lock yet
    let s = Sandbox::new(&[(
        "pyproject.toml",
        "[project]\nname = \"x\"\nversion = \"0\"\nrequires-python = \">=3.12\"\n\n[tool.uv]\npackage = false\n",
    )]);
    // a uv whose lock records the pyproject.toml it read
    let uv = s.bin.join("uv");
    std::fs::write(
        &uv,
        "#!/bin/sh\ncase \"$1\" in --version) echo 'uv 0.13.1'; exit 0 ;; lock) cp pyproject.toml uv.lock ;; esac\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&uv, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = s.ok(&["apply", "--yes"]);
    assert!(
        out.contains("forge: pyproject.toml: added [tool.uv] exclude-newer and required-version, uv.lock created; commit them with Forge's files"),
        "{out}"
    );
    assert!(
        s.read("uv.lock").unwrap().contains("exclude-newer"),
        "locked after the edit"
    );
    let py = s.read("pyproject.toml").unwrap();
    assert!(
        py.ends_with("\n[tool.uv]\nexclude-newer = \"7 days\"\nrequired-version = \">=0.9.17\"\npackage = false\n"),
        "{py}"
    );
    // once there, nothing more is offered
    assert!(!s.ok(&["apply", "--yes"]).contains("pyproject.toml: added"));
}

/// apply offers a ruff config where the Python checks run and the project has none (Forge's own
/// ruff only lints it): without --yes and without a terminal it says so; with --yes it creates
/// the python pack's template, and the next apply offers nothing.
#[test]
fn apply_offers_a_ruff_config() {
    let s = Sandbox::new(PY);
    let out = s.ok(&["apply", "--yes"]);
    assert!(
        out.contains("forge: created ruff.toml (from python); commit it with Forge's files"),
        "{out}"
    );
    let ruff = s.read("ruff.toml").unwrap();
    assert!(ruff.starts_with("line-length = 100\n"), "{ruff}");
    assert!(!s.ok(&["apply", "--yes"]).contains("ruff.toml"));
    // removed, and apply run without a terminal and without --yes: said, not written
    std::fs::remove_file(s.repo.join("ruff.toml")).unwrap();
    let out = s.ok(&["apply"]);
    assert!(
        out.contains("forge: the project does not configure ruff, so Forge's ruff lints its Python files but does not format them or apply safe fixes; re-run with --yes to create ruff.toml"),
        "{out}"
    );
    assert!(!s.exists("ruff.toml"));
    // a project that configures ruff itself is not asked
    let s = Sandbox::new(&[
        (
            "pyproject.toml",
            "[project]\nname = \"app\"\n\n[tool.ruff]\nline-length = 88\n",
        ),
        ("app/main.py", "X = 1\n"),
    ]);
    let out = s.ok(&["apply", "--yes"]);
    assert!(!out.contains("configure ruff"), "{out}");
    assert!(!s.exists("ruff.toml"));
}

/// apply offers the setup of the project's JavaScript package manager where it lacks it, as for
/// uv: npm's cooldown (.npmrc min-release-age, package.json requiring npm >= 11.10), and the lock
/// the project does not have, created with the adapter's `lock`. Without a terminal and without
/// `--yes`, a note and nothing written.
#[test]
fn apply_adds_the_npm_cooldown() {
    let s = Sandbox::new(&[
        (
            "package.json",
            "{\n  \"name\": \"shop\",\n  \"devDependencies\": {\"typescript\": \"5.9.3\"}\n}\n",
        ),
        ("tsconfig.json", "{}\n"),
    ]);
    let npm = s.bin.join("npm");
    std::fs::write(
        &npm,
        "#!/bin/sh\ncase \"$1 $2\" in --version*) echo '11.12.0'; exit 0 ;; 'install --package-lock-only') echo '{\"lockfileVersion\": 3}' > package-lock.json ;; esac\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&npm, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = s.ok(&["apply", "--yes"]);
    assert!(
        out.contains("forge: .npmrc: added min-release-age=7, package.json: added devEngines.packageManager npm >=11.10.0, package-lock.json created; commit them with Forge's files"),
        "{out}"
    );
    assert_eq!(s.read(".npmrc").unwrap(), "min-release-age=7\n");
    assert!(
        s.read("package.json")
            .unwrap()
            .starts_with("{\n  \"devEngines\": {\"packageManager\": {\"name\": \"npm\", \"version\": \">=11.10.0\"}},\n  \"name\": \"shop\","),
        "{:?}",
        s.read("package.json")
    );
    assert!(s.read("package-lock.json").is_some());
    // once there, nothing more is offered
    assert!(!s.ok(&["apply", "--yes"]).contains(".npmrc: added"));
    // without a terminal and without --yes: a note, nothing written
    std::fs::remove_file(s.repo.join(".npmrc")).unwrap();
    let out = s.ok(&["apply"]);
    assert!(
        out.contains("forge: the project's npm setup is incomplete; re-run with --yes to add npm's cooldown to .npmrc"),
        "{out}"
    );
    assert!(s.read(".npmrc").is_none());
}

/// Inside a Claude Code agent's command (`CLAUDECODE` set), Forge's command line refuses to run:
/// the agent's harness is not a tool it operates. The hook entries still run, and `forge test`
/// (`forge_test.rs` runs it with `CLAUDECODE`).
#[test]
fn the_agent_cannot_run_forge() {
    let s = Sandbox::new(PY);
    s.ok(&["apply", "--yes"]);
    for args in [
        &["status"][..],
        &["check", "--all"],
        &["--help"],
        &["--version"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .current_dir(&s.repo)
            .env("HOME", &s.home)
            .env("FORGE_HOME", &s.forge_home)
            .env("FORGE_NO_DOWNLOAD", "1")
            .env("CLAUDECODE", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert_eq!(
            stderr(&out).trim(),
            "This project's checks run by themselves after each edit and when you stop; they are not run by hand.",
            "{args:?}"
        );
    }
    let mut hook = Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["hook", "user-prompt-submit"])
        .current_dir(&s.repo)
        .env("HOME", &s.home)
        .env("FORGE_HOME", &s.forge_home)
        .env("FORGE_NO_DOWNLOAD", "1")
        .env("CLAUDECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let input = format!(
            "{{\"session_id\":\"s\",\"cwd\":{:?},\"hook_event_name\":\"UserPromptSubmit\",\"prompt\":\"x\"}}",
            s.repo.display().to_string()
        );
        hook.stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let out = hook.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!stderr(&out).contains("not run by hand"));
}

/// The user level is Claude Code's config directory: `$CLAUDE_CONFIG_DIR` when set.
#[test]
fn user_level_follows_claude_config_dir() {
    let s = Sandbox::new(PY);
    let cfg = s.home.join("elsewhere/claude");
    let run = |args: &[&str]| {
        let out = s.forge_env(&s.repo, args, Some(&cfg));
        assert!(out.status.success(), "{}", stderr(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let out = run(&["apply", "--user", "--yes"]);
    assert!(
        out.contains(&format!("+++ {}", cfg.join("settings.json").display()))
            || out.contains("+++ ~/elsewhere/claude/settings.json"),
        "{out}"
    );
    let settings = std::fs::read_to_string(cfg.join("settings.json")).unwrap();
    assert!(
        settings.contains("claude-forge hook user-pre-tool-use"),
        "{settings}"
    );
    assert!(
        settings.contains("Edit(~/elsewhere/claude/forge-user.lock)"),
        "{settings}"
    );
    assert!(cfg.join("forge-user.lock").is_file());
    assert!(!s.home.join(".claude").exists(), "~/.claude is untouched");
    assert!(run(&["apply", "--user", "--yes"]).contains("forge: up to date"));
    run(&["remove", "--user", "--yes"]);
    assert!(!cfg.join("forge-user.lock").exists());
}

/// A user lock that records its settings file relative to the home directory
/// (`.claude/settings.json`, before the config directory was the user level's base) is read.
#[test]
fn user_lock_with_home_relative_paths_is_read() {
    let s = Sandbox::new(PY);
    s.ok(&["apply", "--user", "--yes"]);
    let lock_path = s.home.join(".claude/forge-user.lock");
    let lock = std::fs::read_to_string(&lock_path).unwrap();
    assert!(lock.contains("\"settings.json\""), "{lock}");
    std::fs::write(
        &lock_path,
        lock.replace("\"settings.json\"", "\".claude/settings.json\""),
    )
    .unwrap();
    s.ok(&["apply", "--user", "--yes"]);
    let lock = std::fs::read_to_string(&lock_path).unwrap();
    assert!(!lock.contains(".claude/settings.json"), "{lock}");
    s.ok(&["remove", "--user", "--yes"]);
    assert!(!s.home.join(".claude/settings.json").exists());
}

/// A pack's `tool_off`: init writes the diagnostic off in Biome's config, and status lists it
/// with the pack and the reason.
#[test]
fn tool_off_is_written_by_init_and_listed_by_status() {
    let s = Sandbox::new(&[(
        "package.json",
        "{\"name\": \"api\", \"private\": true, \"dependencies\": {\"@nestjs/core\": \"11.1.0\", \"@nestjs/common\": \"11.1.0\"}, \"devDependencies\": {\"typescript\": \"5.9.3\"}}\n",
    )]);
    let out = s.ok(&["init", "--yes"]);
    assert!(
        out.contains("forge: created biome.json (from typescript)"),
        "{out}"
    );
    let biome: serde_json::Value = serde_json::from_str(&s.read("biome.json").unwrap()).unwrap();
    assert_eq!(
        biome["linter"]["rules"]["style"]["useImportType"], "off",
        "{biome}"
    );
    let st = s.ok(&["status"]);
    assert!(
        st.contains("\nSwitched off by packs:\n  biome lint/style/useImportType (typescript/nestjs): a DTO imported with import type"),
        "{st}"
    );
    let v: serde_json::Value = serde_json::from_str(&s.ok(&["status", "--json"])).unwrap();
    assert_eq!(v["switched_off"][0]["pack"], "typescript/nestjs");
}

/// A local pack (.forge/packs/<id>/) is detected like a built-in one; its context items are
/// rendered into its rules file only where their `when` holds, and its rules are active.
#[test]
fn local_packs_render_their_context_where_it_holds() {
    let pack = "id: acme\ndescription: Acme's conventions.\nlevel: project\ndetect: {files_any: [\"app/**\"]}\ncontext:\n  - text: Acme services log with structlog.\n  - text: Acme's uv workspace text.\n    when: {facts: {python.pm: uv}}\n  - text: Acme's pyproject text.\n    when: {facts: {python.pm: pyproject}}\n  - text: Acme's poetry text.\n    when: {facts: {python.pm: poetry}}\n";
    let rule = "id: acme/no-print\ndescription: No print in services.\nscope: {language: python}\nenforce:\n  - {target: grep, mode: forbid, pattern: \"print\\\\(\"}\nexamples:\n  match: [\"print(1)\"]\n  no_match: [\"log(1)\"]\n";
    let s = Sandbox::new(PY);
    s.write(".forge/packs/acme/pack.yaml", pack);
    s.write(".forge/packs/acme/rules/no-print.yaml", rule);
    s.ok(&["apply", "--yes"]);
    let text = s.read(".claude/rules/forge-acme.md").unwrap();
    assert!(text.contains("Acme services log with structlog."), "{text}");
    // pyproject.toml with a [project] table, no lock and no uv settings: python.pm is
    // pyproject (uv has left no mark)
    assert!(text.contains("Acme's pyproject text."), "{text}");
    assert!(!text.contains("uv workspace"), "{text}");
    assert!(!text.contains("poetry"), "{text}");
    let st = s.ok(&["status"]);
    assert!(st.contains("  acme (detected)"), "{st}");
    assert!(st.contains("  acme/no-print\n"), "{st}");
    // the pack's files are inputs: apply leaves them as they are
    assert_eq!(s.read(".forge/packs/acme/pack.yaml").unwrap(), pack);
    // a local pack whose id is a built-in pack's is refused
    s.write(
        ".forge/packs/git/pack.yaml",
        "id: git\ndescription: Ours.\nlevel: project\n",
    );
    let out = s.forge(&["apply", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("is a built-in pack's"),
        "{}",
        stderr(&out)
    );
}
