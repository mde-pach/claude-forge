//! Fixtures shared by the tests: a temporary git repository holding a pack example's `head`
//! (committed) and `files` (on top), the change set as the checker hands it to builtins, and
//! tools run as the checker's adapters would.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use forge_builtins_data::{
    Facts, Mode, Options, PinnedTools, ProjectTools, SQUAWK_VERSION, ToolOutput, ToolRun,
    glob_matches,
};
use forge_core::builtin::{BuiltinCtx, Scope};
use forge_packs::model::{Language, Rule};

pub fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .stdout(Stdio::null())
        .status()
        .expect("git")
        .success();
    assert!(ok, "git {args:?}");
}

pub fn write(dir: &Path, files: &BTreeMap<String, String>) {
    for (p, text) in files {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// A repository with a change, laid out as the checker lays it out.
pub struct Repo {
    pub dir: tempfile::TempDir,
    pub root: PathBuf,
    pub base: PathBuf,
    pub changed: Vec<String>,
    pub head: BTreeMap<String, String>,
}

pub fn repo(head: &BTreeMap<String, String>, files: &BTreeMap<String, String>) -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    write(&root, head);
    std::fs::write(root.join(".forge-test-keep"), "").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "head"]);
    write(&root, files);
    let changed: Vec<String> = files
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
        head: head.clone(),
    }
}

impl Repo {
    pub fn ctx(&self, rule: &str, accepted: Vec<String>) -> BuiltinCtx {
        BuiltinCtx {
            root: self.root.clone(),
            base: Some(self.base.clone()),
            changed: self.changed.clone(),
            rule: rule.into(),
            scope: Scope::Changed,
            accepted,
            notes_file: None,
        }
    }

    /// The lines the change added to a file (None: the whole file is new).
    pub fn added_lines(&self, path: &str) -> Option<Vec<u32>> {
        let old = self.base.join(path);
        if !old.is_file() {
            return None;
        }
        let out = Command::new("git")
            .args(["diff", "--no-index", "-U0", "--"])
            .arg(&old)
            .arg(self.root.join(path))
            .output()
            .expect("git diff");
        let hunk = regex::Regex::new(r"^@@ -\S+ \+(\d+)(?:,(\d+))? @@").unwrap();
        let mut v = vec![];
        for l in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(c) = hunk.captures(l) {
                let start: u32 = c[1].parse().unwrap();
                let n: u32 = c.get(2).map_or(1, |m| m.as_str().parse().unwrap());
                v.extend(start..start + n);
            }
        }
        Some(v)
    }

    /// Whether a finding names the change: a changed file as a whole, or an added line.
    pub fn names_change(&self, f: &forge_core::Finding) -> bool {
        let Some(p) = &f.path else { return false };
        if !self.changed.contains(p) {
            return false;
        }
        match (f.line, self.added_lines(p)) {
            (None | Some(0), _) | (_, None) => true,
            (Some(l), Some(added)) => added.contains(&l),
        }
    }

    pub fn existing(&self) -> Vec<String> {
        self.changed
            .iter()
            .filter(|p| self.root.join(p).is_file())
            .cloned()
            .collect()
    }
}

fn language_globs(l: Language) -> &'static [&'static str] {
    match l {
        Language::Python => &["*.py", "*.pyi"],
        Language::Typescript => &[
            "*.ts", "*.tsx", "*.mts", "*.cts", "*.js", "*.jsx", "*.mjs", "*.cjs",
        ],
        Language::Tsx => &["*.tsx", "*.jsx"],
        Language::Yaml => &["*.yaml", "*.yml"],
        Language::Any => &[],
    }
}

fn expand(globs: &[String]) -> Vec<String> {
    let mut out = vec![];
    for g in globs {
        match forge_packs::roles::role_ref(g) {
            Some((neg, name)) => {
                for p in forge_packs::roles::default_patterns(&name) {
                    let (pn, pg) = match p.strip_prefix('!') {
                        Some(r) => (true, r.to_owned()),
                        None => (false, p.clone()),
                    };
                    out.push(if neg != pn { format!("!{pg}") } else { pg });
                }
            }
            None => out.push(g.clone()),
        }
    }
    out
}

fn list_match(globs: &[String], path: &str) -> bool {
    let inc: Vec<&String> = globs.iter().filter(|g| !g.starts_with('!')).collect();
    if globs
        .iter()
        .filter_map(|g| g.strip_prefix('!'))
        .any(|g| glob_matches(g, path))
    {
        return false;
    }
    inc.is_empty() || inc.iter().any(|g| glob_matches(g, path))
}

/// Whether a path is in the rule's scope (its files and its language).
pub fn in_scope(rule: &Rule, path: &str) -> bool {
    let files = expand(rule.scope.files.as_deref().unwrap_or(&[]));
    let lang: Vec<String> = language_globs(rule.scope.language)
        .iter()
        .map(|s| s.to_string())
        .collect();
    list_match(&files, path) && (lang.is_empty() || lang.iter().any(|g| glob_matches(g, path)))
}

pub fn when_changed(globs: &[String], path: &str) -> bool {
    expand(globs).iter().any(|g| !g.starts_with('!') && glob_matches(g, path))
}

pub fn have(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Squawk through uvx, as the checker's resolver offers it.
pub struct UvxPinned;

impl PinnedTools for UvxPinned {
    fn command(&self, name: &str) -> Result<Vec<String>, String> {
        if name != "squawk" || !have("uvx", &["--version"]) {
            return Err(format!("{name} is not installed"));
        }
        Ok(vec![
            "uvx".into(),
            "-q".into(),
            "--from".into(),
            format!("squawk-cli=={SQUAWK_VERSION}"),
            "squawk".into(),
        ])
    }
}

/// The project tools the examples need, run as their adapters run them: sqlfluff through uvx,
/// the machine's alembic.
pub struct LocalProjectTools;

pub fn output(c: &mut Command) -> ToolRun {
    match c.stdin(Stdio::null()).output() {
        Ok(o) => ToolRun::Ran(ToolOutput {
            status: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }),
        Err(_) => ToolRun::Missing,
    }
}

impl ProjectTools for LocalProjectTools {
    fn run(&self, root: &Path, tool: &str, op: &str, args: &[String]) -> ToolRun {
        match (tool, op) {
            ("sqlfluff", "lint") if have("uvx", &["--version"]) => output(
                Command::new("uvx")
                    .args(["-q", "--from", "sqlfluff==3.4.2", "sqlfluff", "lint"])
                    .args(["--format", "github-annotation-native", "--nocolor"])
                    .args(args)
                    .current_dir(root),
            ),
            ("alembic", "upgrade-sql") if have("alembic", &["--version"]) => output(
                Command::new("alembic")
                    .arg("upgrade")
                    .args(args)
                    .arg("--sql")
                    .current_dir(root),
            ),
            _ => ToolRun::Missing,
        }
    }
}

pub fn options(facts: Facts) -> Options {
    Options {
        project_tools: Arc::new(LocalProjectTools),
        pinned: Arc::new(UvxPinned),
        facts: Some(facts),
        ..Options::default()
    }
}

pub fn facts(mode: Mode, files: Vec<String>) -> Facts {
    Facts::new(mode, files)
}

/// The accepted changes of forge.yaml for a rule.
pub fn accepted(files: &BTreeMap<String, String>, rule: &str) -> Vec<String> {
    let Some(text) = files.get("forge.yaml") else {
        return vec![];
    };
    let v = forge_packs::yaml::parse_strict(text).unwrap().value;
    v.get("accept")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter(|e| e.get("rule").and_then(|r| r.as_str()) == Some(rule))
        .filter_map(|e| e.get("change").and_then(|c| c.as_str()).map(str::to_owned))
        .collect()
}
