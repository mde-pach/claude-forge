//! What the example and golden tests share: the pack examples of the rules that call these
//! builtins, a temporary git repository per example with the change set the checker would make,
//! the checker's gating (scope, `when_changed`), and the tools.
//!
//! Tools (`FORGE_TEST_TOOLS`):
//! - unset or `fake`: the fake binaries of tests/fakes (each reproduces its tool's output format,
//!   captured once from the real tool, for what the examples exercise);
//! - `local`: the real pinned tools already on this machine (the TypeScript Forge's cache
//!   `~/.cache/forge/bin`, uvx and bunx offline caches), fakes for the project tools not on PATH;
//! - `download`: forge-tools' provisioner, which may download the pinned tools.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use forge_builtins_infra::{Extra, Options, PathTools, names};
use forge_core::builtin::{BuiltinCtx, Scope};
use forge_core::{Note, NoteKind};
use forge_packs::model::{Example, Language, Rule, Target};

pub fn fakes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fakes")
}

/// One builtin call on one pack example.
#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub rule: Rule,
    pub builtin: String,
    pub args: Vec<String>,
    pub when_changed: Option<Vec<String>>,
    pub head: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
    /// true: something must be found; false: nothing.
    pub expect: bool,
}

/// The builtin and arguments of a check command (`forge_x a b`, `forge_advise forge_x a`,
/// `forge_once forge_x`), when it is one of this crate's.
pub fn builtin_call(run: &str) -> Option<(String, Vec<String>)> {
    let mut w = run.split_whitespace().peekable();
    while w
        .peek()
        .is_some_and(|x| *x == "forge_advise" || *x == "forge_once")
    {
        w.next();
    }
    let name = w.next()?.strip_prefix("forge_")?.to_string();
    if !names().contains(&name.as_str()) {
        return None;
    }
    let args: Vec<String> = w.map(str::to_string).collect();
    if args.iter().any(|a| a.contains(['|', ';', '&', '$', '`'])) {
        return None;
    }
    Some((name, args))
}

pub fn cases() -> Vec<Case> {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
    let mut out = vec![];
    for pack in set.packs.values() {
        for rule in &pack.rules {
            for t in &rule.enforce {
                let Target::Check(c) = t else { continue };
                let Some((builtin, args)) = builtin_call(&c.run) else {
                    continue;
                };
                let Some(ex) = &rule.examples else { continue };
                for (expect, list) in [(true, &ex.match_), (false, &ex.no_match)] {
                    for (i, e) in list.iter().flatten().enumerate() {
                        let Example::Fixture(f) = e else { continue };
                        out.push(Case {
                            name: format!(
                                "{} {}[{i}]",
                                rule.id,
                                if expect { "match" } else { "no_match" }
                            ),
                            rule: rule.clone(),
                            builtin: builtin.clone(),
                            args: args.clone(),
                            when_changed: c.when_changed.clone(),
                            head: f.head.clone().unwrap_or_default().into_iter().collect(),
                            files: f.files.clone().into_iter().collect(),
                            expect,
                        });
                    }
                }
            }
        }
    }
    out
}

pub fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git")
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn write(dir: &Path, files: &BTreeMap<String, String>) {
    for (p, text) in files {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// An example as a repository: `head` committed, `files` written over it; the base copies of
/// the changed files, as the checker keeps them.
pub struct Repo {
    pub dir: tempfile::TempDir,
    pub root: PathBuf,
    pub base: PathBuf,
    pub changed: Vec<String>,
}

pub fn repo(case: &Case) -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    let mut head = case.head.clone();
    head.insert(".gitignore".into(), "node_modules/\n.venv/\n".into());
    write(&root, &head);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "head"]);
    write(&root, &case.files);
    let changed: Vec<String> = case
        .files
        .iter()
        .filter(|(p, t)| head.get(*p) != Some(*t))
        .map(|(p, _)| p.clone())
        .collect();
    let base = dir.path().join("base");
    std::fs::create_dir_all(&base).unwrap();
    for p in &changed {
        if let Some(t) = head.get(p) {
            let b = base.join(p);
            std::fs::create_dir_all(b.parent().unwrap()).unwrap();
            std::fs::write(b, t).unwrap();
        }
    }
    Repo {
        dir,
        root,
        base,
        changed,
    }
}

/// The accepted changes of forge.yaml for the rule.
pub fn accepted(case: &Case) -> Vec<String> {
    let Some(text) = case.files.get("forge.yaml").or(case.head.get("forge.yaml")) else {
        return vec![];
    };
    let v = forge_packs::yaml::parse_strict(text).unwrap().value;
    v.get("accept")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter(|e| e.get("rule").and_then(|r| r.as_str()) == Some(case.rule.id.as_str()))
        .filter_map(|e| e.get("change").and_then(|c| c.as_str()).map(str::to_owned))
        .collect()
}

pub fn ctx(case: &Case, r: &Repo) -> BuiltinCtx {
    BuiltinCtx {
        root: r.root.clone(),
        base: Some(r.base.clone()),
        changed: r.changed.clone(),
        rule: case.rule.id.clone(),
        scope: Scope::Changed,
        accepted: accepted(case),
        notes_file: None,
    }
}

/// The check's files as the checker computes them, or None when it would not run the check:
/// with `when_changed`, a changed file must match it; without, an in-scope file must have changed.
pub fn gate(case: &Case, r: &Repo, opts: &Options) -> Option<Vec<String>> {
    let ctx = ctx(case, r);
    let extra = |rule: Rule| Extra {
        rule: Some(rule),
        ..Extra::default()
    };
    let cx = forge_builtins_infra::Cx::new(&ctx, extra(case.rule.clone()), opts);
    let files: Vec<String> = r
        .changed
        .iter()
        .filter(|p| r.root.join(p).is_file() && cx.in_rule_scope(p))
        .cloned()
        .collect();
    match &case.when_changed {
        Some(w) => {
            let mut wr = case.rule.clone();
            wr.scope.files = Some(w.clone());
            wr.scope.language = Language::Any;
            let wcx = forge_builtins_infra::Cx::new(&ctx, extra(wr), opts);
            r.changed
                .iter()
                .any(|p| wcx.in_rule_scope(p))
                .then_some(files)
        }
        None => (!files.is_empty()).then_some(files),
    }
}

/// Pinned tools from a directory of executables named after the tools.
pub struct DirTools(pub PathBuf);

impl forge_tools::ToolResolver for DirTools {
    fn resolve(&self, tool: &str) -> Option<PathBuf> {
        let p = self.0.join(tool);
        p.is_file().then_some(p)
    }

    fn ensure(&self, tool: &str) -> Result<PathBuf, Note> {
        self.resolve(tool)
            .ok_or_else(|| Note::new(NoteKind::MissingTool, format!("{tool} is not installed")))
    }
}

fn install(dir: &Path, name: &str, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The fake tools, in a directory of their own.
pub fn fake_bin() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for e in std::fs::read_dir(fakes_dir()).unwrap() {
        let e = e.unwrap();
        let text = std::fs::read_to_string(e.path()).unwrap();
        install(d.path(), &e.file_name().to_string_lossy(), &text);
    }
    for p in ["aws", "google", "azurerm"] {
        install(
            d.path(),
            &format!("tflint-ruleset-{p}"),
            "#!/bin/sh\nexit 0\n",
        );
    }
    d
}

/// The tools of `FORGE_TEST_TOOLS`, and the directories they live in.
pub fn tools() -> (Options, Vec<tempfile::TempDir>, String) {
    let mode = std::env::var("FORGE_TEST_TOOLS").unwrap_or_else(|_| "fake".into());
    let fakes = fake_bin();
    let fake_path = fakes.path().to_string_lossy().into_owned();
    let sys = std::env::var("PATH").unwrap_or_default();
    match mode.as_str() {
        "local" => {
            let real = tempfile::tempdir().unwrap();
            let cache = std::env::var("HOME").unwrap() + "/.cache/forge/bin";
            let plat = format!(
                "{}-{}",
                if cfg!(target_os = "macos") {
                    "Darwin"
                } else {
                    "Linux"
                },
                if cfg!(target_arch = "aarch64") {
                    "arm64"
                } else {
                    "x86_64"
                }
            );
            let cat = forge_tools::Catalog::builtin();
            for t in [
                "actionlint",
                "kube-linter",
                "kubeconform",
                "tflint",
                "tflint-ruleset-aws",
                "tflint-ruleset-google",
                "tflint-ruleset-azurerm",
                "trivy",
                "oasdiff",
            ] {
                let v = cat.get(t).unwrap().version;
                let p = format!("{cache}/{t}-{v}-{plat}/{t}");
                if Path::new(&p).is_file() {
                    std::os::unix::fs::symlink(&p, real.path().join(t)).unwrap();
                }
            }
            let ver = |t: &str| cat.get(t).unwrap().version;
            install(
                real.path(),
                "hadolint",
                &format!(
                    "#!/bin/sh\nexec uvx -q --offline --from hadolint-bin=={} hadolint \"$@\"\n",
                    ver("hadolint")
                ),
            );
            install(
                real.path(),
                "zizmor",
                &format!(
                    "#!/bin/sh\nexec uvx -q --offline zizmor@{} \"$@\"\n",
                    ver("zizmor")
                ),
            );
            install(
                real.path(),
                "redocly",
                &format!(
                    "#!/bin/sh\nexec bunx @redocly/cli@{} \"$@\"\n",
                    ver("redocly")
                ),
            );
            install(
                real.path(),
                "spectral",
                &format!(
                    "#!/bin/sh\nexec bunx @stoplight/spectral-cli@{} \"$@\"\n",
                    ver("spectral")
                ),
            );
            let opts = Options {
                pinned: Arc::new(DirTools(real.path().to_path_buf())),
                project: Arc::new(PathTools {
                    path: Some(format!("{sys}:{fake_path}")),
                }),
                cache_dir: None,
            };
            (opts, vec![fakes, real], mode)
        }
        "download" => {
            let home = forge_home::Home::from_env().expect("home");
            let opts = Options {
                pinned: Arc::new(forge_tools::Provisioner::new(home)),
                project: Arc::new(PathTools {
                    path: Some(format!("{sys}:{fake_path}")),
                }),
                cache_dir: None,
            };
            (opts, vec![fakes], mode)
        }
        _ => {
            let opts = Options {
                pinned: Arc::new(DirTools(fakes.path().to_path_buf())),
                project: Arc::new(PathTools {
                    path: Some(fake_path),
                }),
                cache_dir: None,
            };
            (opts, vec![fakes], "fake".into())
        }
    }
}
