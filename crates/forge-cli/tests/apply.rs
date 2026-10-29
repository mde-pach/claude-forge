//! `forge apply` on the generator samples (tests/generator/): golden snapshots of everything
//! it writes, idempotence, drift, dry runs, remove, and the migration from forge 0.5's layout.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SAMPLES: &[&str] = &[
    "fastapi-app",
    "nextjs-prisma-app",
    "astro-app",
    "eslint-app",
    "docker-app",
    "pnpm-workspace-app",
];

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

/// A sample in a fresh git repository, with a fake `claude` and private Forge and home dirs.
struct Sandbox {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    forge_home: PathBuf,
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "maintenance.auto=false"])
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

impl Sandbox {
    fn new(sample: &str) -> Sandbox {
        Self::with_claude(sample, "2.1.300")
    }

    fn with_claude(sample: &str, version: &str) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join(sample);
        copy_dir(&repo_root().join("tests/generator").join(sample), &repo);
        git(&repo, &["init", "-q"]);
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        std::fs::write(
            &claude,
            format!("#!/bin/sh\necho \"{version} (Claude Code)\"\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let home = tmp.path().join("home");
        let forge_home = tmp.path().join("forge-home");
        std::fs::create_dir_all(&home).unwrap();
        Sandbox {
            _tmp: tmp,
            repo,
            bin,
            home,
            forge_home,
        }
    }

    fn forge(&self, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_forge"))
            .args(args)
            .current_dir(&self.repo)
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("FORGE_HOME", &self.forge_home)
            .env("FORGE_NO_DOWNLOAD", "1")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("XDG_STATE_HOME")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("FORGE_LAUNCHED_VERSION")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
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

    /// Everything apply writes, as one text: the rules files, the settings, the lock, the git hook.
    fn snapshot(&self) -> String {
        let mut paths: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.repo.join(".claude/rules")) {
            for e in rd.flatten() {
                paths.push(format!(".claude/rules/{}", e.file_name().to_string_lossy()));
            }
        }
        paths.sort();
        for p in [
            ".claude/settings.json",
            ".claude/settings.local.json",
            "forge.lock",
            ".git/hooks/pre-commit",
        ] {
            if self.repo.join(p).exists() {
                paths.push(p.into());
            }
        }
        let mut out = String::new();
        for p in paths {
            out.push_str(&format!("=== {p}\n{}", self.read(&p).unwrap()));
        }
        out.replace(env!("CARGO_PKG_VERSION"), "<forge version>")
    }
}

#[test]
fn samples_match_their_snapshots() {
    for name in SAMPLES {
        let s = Sandbox::new(name);
        let out = s.forge(&["apply", "--yes"]);
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stderr), "", "{name}: stderr");
        insta::assert_snapshot!(*name, s.snapshot());
    }
}

#[test]
fn a_second_apply_changes_nothing() {
    for name in SAMPLES {
        let s = Sandbox::new(name);
        s.ok(&["apply", "--yes"]);
        let before = s.snapshot();
        let out = s.ok(&["apply", "--yes"]);
        assert!(out.contains("forge: up to date"), "{name}: {out}");
        assert_eq!(s.snapshot(), before, "{name}");
        let diff = s.forge(&["diff"]);
        assert_eq!(
            diff.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&diff.stdout)
        );
    }
}

/// Pack contexts that still name an internal (the accept list), until the pack content pass
/// rewrites them; any other occurrence fails.
const KNOWN_INTERNALS: &[(&str, &str)] = &[
    ("forge-typescript-prisma.md", "forge.yaml"),
    ("forge-terraform.md", "forge.yaml"),
    ("forge-python-library.md", "forge.yaml"),
    ("forge-typescript-library.md", "forge.yaml"),
];

/// The rules files are what the agent reads: no Forge internals in them, for any sample.
#[test]
fn rules_files_name_no_internals() {
    for name in SAMPLES {
        let s = Sandbox::new(name);
        s.ok(&["apply", "--yes"]);
        for e in std::fs::read_dir(s.repo.join(".claude/rules"))
            .unwrap()
            .flatten()
        {
            let text = std::fs::read_to_string(e.path()).unwrap();
            let file = e.file_name().to_string_lossy().into_owned();
            let found: Vec<_> = forge_core::internals::find(&text)
                .into_iter()
                .filter(|(_, term)| !KNOWN_INTERNALS.contains(&(file.as_str(), *term)))
                .collect();
            assert!(found.is_empty(), "{name}: {file} names {found:?}");
        }
    }
}

#[test]
fn drift_and_conflicts_are_refused_without_force() {
    let s = Sandbox::new("fastapi-app");
    s.ok(&["apply", "--yes"]);
    let f = ".claude/rules/forge-python.md";
    let mine = format!("{}\nmy own line\n", s.read(f).unwrap());
    s.write(f, &mine);
    let out = s.forge(&["apply", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!("forge: {f} was modified since the last apply")),
        "{err}"
    );
    assert_eq!(s.read(f).unwrap(), mine);
    assert_eq!(s.forge(&["diff"]).status.code(), Some(1));
    s.ok(&["apply", "--yes", "--force"]);
    assert_ne!(s.read(f).unwrap(), mine);

    let fresh = Sandbox::new("fastapi-app");
    fresh.write(f, "not forge's\n");
    let out = fresh.forge(&["apply", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("exists and is not a forge file"));
    assert!(fresh.read("forge.lock").is_none());
}

#[test]
fn dry_run_writes_nothing() {
    let s = Sandbox::new("nextjs-prisma-app");
    let out = s.ok(&["apply", "--dry-run"]);
    assert!(out.contains("+++ forge.lock"), "{out}");
    assert!(out.contains(".claude/rules/"), "{out}");
    assert!(s.read("forge.lock").is_none());
    assert!(!s.repo.join(".claude").exists());
}

#[test]
fn remove_restores_the_repository() {
    let s = Sandbox::new("fastapi-app");
    let mine = "{\n  // the team's\n  \"model\": \"opus\",\n  \"permissions\": {\n    \"allow\": [\"Bash(ls)\"]\n  }\n}\n";
    s.write(".claude/settings.json", mine);
    s.ok(&["apply", "--yes"]);
    let settings = s.read(".claude/settings.json").unwrap();
    assert!(settings.contains("// the team's"));
    assert!(settings.contains("claude-forge hook pre-tool-use"));
    let out = s.ok(&["remove", "--yes"]);
    assert!(out.contains("forge: removed"), "{out}");
    assert_eq!(s.read(".claude/settings.json").unwrap(), mine);
    assert!(s.read("forge.lock").is_none());
    assert!(!s.repo.join(".claude/rules").exists());
    assert!(s.read(".git/hooks/pre-commit").is_none());
}

#[test]
fn an_old_claude_code_is_refused() {
    let s = Sandbox::with_claude("fastapi-app", "2.1.284");
    let out = s.forge(&["apply", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("Claude Code 2.1.284 is too old; forge needs claude >= 2.1.295")
    );
}

#[test]
fn status_and_explain_report_the_plan() {
    let s = Sandbox::new("fastapi-app");
    s.ok(&["apply", "--yes"]);
    let out = s.ok(&["status"]);
    assert!(out.contains("Packs:\n"), "{out}");
    assert!(out.contains("Drift: none"), "{out}");
    let json: serde_json::Value = serde_json::from_str(&s.ok(&["status", "--json"])).unwrap();
    assert!(json["counts"]["active"].as_u64().unwrap() > 10);
    let out = s.ok(&["explain", "python/no-eval"]);
    assert!(out.starts_with("# python/rules/no-eval.yaml"), "{out}");
    assert!(out.contains("Enforced here:"), "{out}");
}

/// A repository forge 0.5 applied (tests/fixtures/old-layout, see make-old-layout.mjs).
fn old_layout() -> Sandbox {
    let s = Sandbox::new("fastapi-app");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/old-layout");
    copy_dir(&fixture.join("dot-claude"), &s.repo.join(".claude"));
    copy_dir(&fixture.join("dot-forge"), &s.repo.join(".forge"));
    let hook = s.repo.join(".git/hooks/pre-commit");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::copy(fixture.join("git-pre-commit"), &hook).unwrap();
    std::fs::create_dir_all(s.repo.join(".forge/state/x")).unwrap();
    s.write(
        ".forge/rules/team-secrets.yaml",
        "id: team/no-secrets-file\ndescription: Claude asks you before editing secrets.txt.\nscope: {language: any}\nenforce:\n  - {target: permission, decision: ask, rule: \"Edit(secrets.txt)\"}\nexamples:\n  match: [\"Edit(secrets.txt)\"]\n  no_match: [\"Edit(README.md)\"]\n",
    );
    s
}

#[test]
fn migration_from_the_old_layout() {
    let s = old_layout();
    // without a terminal or --yes, nothing happens
    let out = s.forge(&["apply"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert!(s.read(".forge/check.sh").is_some());

    let out = s.ok(&["apply", "--yes"]);
    assert!(out.contains("layout of forge 0.5"), "{out}");
    assert!(out.contains("  .forge/check.sh\n"), "{out}");
    for gone in [
        ".forge/check.sh",
        ".forge/helpers/ratchet.py",
        ".forge/ast-grep/sgconfig.yml",
        ".forge/manifest.sha256",
        ".forge/forge.lock.json",
        ".forge/.gitignore",
        ".claude/skills/forge-project/hooks/hooks.json",
    ] {
        assert!(s.read(gone).is_none(), "{gone} is still there");
    }
    assert!(!s.repo.join(".forge/state").exists());
    assert!(!s.repo.join(".claude/skills").exists());
    assert!(s.read(".forge/rules/team-secrets.yaml").is_some());
    let settings = s.read(".claude/settings.json").unwrap();
    assert!(!settings.contains("forge-project"), "{settings}");
    assert!(settings.contains("Edit(secrets.txt)"), "{settings}");
    assert!(settings.contains("claude-forge hook stop"), "{settings}");
    assert!(
        s.read(".git/hooks/pre-commit")
            .unwrap()
            .contains("claude-forge check --changed")
    );
    let lock = s.read("forge.lock").unwrap();
    assert!(lock.contains(".git/hooks/pre-commit"), "{lock}");
    assert!(s.ok(&["apply", "--yes"]).contains("forge: up to date"));
}

#[test]
fn migration_keeps_a_modified_old_file_without_force() {
    let s = old_layout();
    s.write(".forge/check.sh", "#!/bin/sh\n# mine now\n");
    let out = s.forge(&["apply", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains(".forge/check.sh (generated by forge 0.5) was modified")
    );
    assert!(s.read(".forge/check.sh").is_some());
    s.ok(&["apply", "--yes", "--force"]);
    assert!(s.read(".forge/check.sh").is_none());
}
