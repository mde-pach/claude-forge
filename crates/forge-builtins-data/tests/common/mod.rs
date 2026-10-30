//! Fixtures shared by the tests: a temporary git repository holding a pack example's `head`
//! (committed) and `files` (on top), the change set as the checker hands it to builtins, and
//! tools run as the checker's adapters would.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use forge_builtins_data::{Mode, Options, ToolOutput, ToolRun, glob_matches};
use forge_check::builtins::{BuiltinTools, CheckCtx, CheckRule};

/// The Squawk release Forge pins.
pub const SQUAWK_VERSION: &str = "2.70.0";
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
    expand(globs)
        .iter()
        .any(|g| !g.starts_with('!') && glob_matches(g, path))
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
fn uvx_pinned(name: &str) -> Result<Vec<String>, String> {
    {
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
/// the machine's alembic, and condor: a stand-in (`tests/fake_condor.py`), or the pinned cndr
/// through uvx when `FORGE_REAL_CONDOR` is set.
pub struct LocalProjectTools;

/// The pinned condor release (PyPI `cndr`).
pub const CONDOR_VERSION: &str = "0.2.0";

/// condor as the adapter runs it (`dead-code`: `--format json`), from `root`.
pub fn condor(root: &Path, args: &[String]) -> ToolRun {
    let mut c = if std::env::var_os("FORGE_REAL_CONDOR").is_some() {
        let mut c = Command::new("uvx");
        c.args(["-q", "--from", &format!("cndr=={CONDOR_VERSION}"), "condor"]);
        c
    } else {
        let mut c = Command::new("python3");
        c.arg("-I")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_condor.py"));
        c
    };
    output(c.args(["--format", "json"]).args(args).current_dir(root))
}

pub fn output(c: &mut Command) -> ToolRun {
    match c.stdin(Stdio::null()).output() {
        Ok(o) => ToolRun::Ran(ToolOutput {
            status: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }),
        Err(e) => ToolRun::Missing(e.to_string()),
    }
}

impl BuiltinTools for LocalProjectTools {
    fn pinned(&self, tool: &str) -> Result<Vec<String>, String> {
        uvx_pinned(tool)
    }

    fn run(&self, root: &Path, _cwd: &Path, tool: &str, op: &str, args: &[String]) -> ToolRun {
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
            ("condor", "dead-code") => condor(root, args),
            _ => ToolRun::Missing(format!("{tool} is not installed")),
        }
    }
}

pub fn options() -> Options {
    Options {
        tools: Arc::new(LocalProjectTools),
        ..Options::default()
    }
}

/// The checker's context of a check: `files` in `mode`, Forge's default roles.
pub fn facts(mode: Mode, files: Vec<String>) -> CheckCtx {
    CheckCtx {
        files,
        mode,
        ..CheckCtx::default()
    }
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

// ---- cases: a builtin call on a fixture

/// One builtin call on one fixture.
#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub rule: String,
    /// The check command (`changed_run` when it takes `{files}`, else `run`).
    pub cmd: String,
    /// The rule, for the check's files.
    pub scope: Option<Rule>,
    pub when_changed: Option<Vec<String>>,
    pub lint: bool,
    pub mode: Mode,
    pub head: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
    /// Some(true): must find something; Some(false): nothing.
    pub want: Option<bool>,
}

/// The builtin a check command calls, with its literal arguments (`{files}` expanded).
pub fn call(cmd: &str, files: &[String]) -> Option<(String, Vec<String>)> {
    let ours = forge_builtins_data::names();
    let cmd = cmd.replace("forge_advise ", "");
    let re = regex::Regex::new(r"forge_([a-z_]+)((?: [^|;&$()\n]+)?)").unwrap();
    for c in re.captures_iter(&cmd) {
        let name = c[1].to_owned();
        if !ours.contains(&name.as_str()) {
            continue;
        }
        let mut args = vec![];
        for w in c[2].split_whitespace() {
            if w == "{files}" {
                args.extend(files.iter().cloned());
            } else {
                args.push(w.to_owned());
            }
        }
        // the Prisma rule's command passes the check's files itself
        if cmd.contains("$_dm_files") {
            args = files.to_vec();
        }
        return Some((name, args));
    }
    None
}

/// Every fixture example of a rule whose check calls one of this crate's builtins.
pub fn pack_cases() -> Vec<Case> {
    use forge_packs::model::{Example, Stage, Target};
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
    let mut out = vec![];
    for pack in set.packs.values() {
        for rule in &pack.rules {
            for t in &rule.enforce {
                let Target::Check(c) = t else { continue };
                let cmd = match &c.changed_run {
                    Some(r) if r.contains("{files}") => r.clone(),
                    _ => c.run.clone(),
                };
                if call(&cmd, &[]).is_none() {
                    continue;
                }
                let Some(ex) = &rule.examples else { continue };
                for (want, list) in [(true, &ex.match_), (false, &ex.no_match)] {
                    for (i, e) in list.iter().flatten().enumerate() {
                        let Example::Fixture(fx) = e else { continue };
                        out.push(Case {
                            name: format!(
                                "{} {}[{i}]",
                                rule.id,
                                if want { "match" } else { "no_match" }
                            ),
                            rule: rule.id.clone(),
                            cmd: cmd.clone(),
                            scope: Some(rule.clone()),
                            when_changed: c.when_changed.clone(),
                            lint: c.stage == Stage::Lint,
                            mode: Mode::Changed,
                            head: fx.head.clone().unwrap_or_default().into_iter().collect(),
                            files: fx.files.clone().into_iter().collect(),
                            want: Some(want),
                        });
                    }
                }
            }
        }
    }
    out
}

/// What a run produced.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Every line of output for the agent.
    pub texts: Vec<String>,
    /// The output that counts: accepted changes dropped, a lint check's findings only when one
    /// names the change; empty when the check does not run.
    pub counted: Vec<String>,
    pub notes: Vec<String>,
}

impl Case {
    pub fn needs_uvx(&self) -> bool {
        self.cmd.contains("squawk")
            || self.cmd.contains("sqlfluff")
            || (self.cmd.contains("forge_condor")
                && std::env::var_os("FORGE_REAL_CONDOR").is_some())
    }

    /// The check's files: the in-scope changed files that exist (none with `--all`).
    pub fn check_files(&self, r: &Repo) -> Vec<String> {
        if self.mode == Mode::All {
            return vec![];
        }
        r.existing()
            .into_iter()
            .filter(|p| self.scope.as_ref().is_none_or(|s| in_scope(s, p)))
            .collect()
    }

    /// Whether the checker runs the check at all.
    pub fn triggered(&self, r: &Repo) -> bool {
        if self.mode == Mode::All || self.scope.is_none() {
            return true;
        }
        match &self.when_changed {
            Some(w) => r.changed.iter().any(|p| when_changed(w, p)),
            None => !self.check_files(r).is_empty(),
        }
    }

    pub fn accepted(&self) -> Vec<String> {
        let mut a = accepted(&self.files, &self.rule);
        a.extend(accepted(&self.head, &self.rule));
        a
    }

    /// The output that counts, from the lines a run printed and whether one names the change.
    pub fn count(
        &self,
        r: &Repo,
        lines: Vec<String>,
        names_file: bool,
        names_change: bool,
    ) -> Vec<String> {
        let accepted = self.accepted();
        let mut counted: Vec<String> = lines
            .into_iter()
            .filter(|t| !accepted.contains(t))
            .collect();
        if self.cmd.contains("$_dm_files") {
            // the pack command drops the lines forge.yaml accepts
            let yaml = std::fs::read_to_string(r.root.join("forge.yaml")).unwrap_or_default();
            counted.retain(|l| !yaml.contains(l.as_str()));
        }
        // a lint check fails on what names the change: an added line, or a changed file as a
        // whole (the checker's attribution); output naming no file counts as a whole. A
        // builtin that compared with the base itself (`$FORGE_ATTRIBUTED`) is taken as it is.
        let attributed = r.dir.path().join("attributed").exists();
        if self.lint && self.mode != Mode::All && names_file && !names_change && !attributed {
            counted.clear();
        }
        counted
    }
}

/// The Rust builtin on a case.
pub fn port(case: &Case, r: &Repo) -> Outcome {
    let mut out = Outcome::default();
    if !case.triggered(r) {
        return out;
    }
    let files = case.check_files(r);
    let (name, args) = call(&case.cmd, &files).expect("a builtin of this crate");
    let mut check = facts(case.mode, files);
    check.check = case
        .scope
        .clone()
        .map(|rule| CheckRule { rule, dirs: None });
    check.attributed = Some(r.dir.path().join("attributed"));
    let all = forge_builtins_data::builtins(options());
    let b = all.iter().find(|b| b.name() == name).unwrap();
    let mut ctx = r.ctx(&case.rule, case.accepted());
    if case.mode == Mode::All {
        ctx.scope = Scope::All;
        ctx.changed.clear();
        ctx.base = None;
    }
    let report = b.run(&ctx, &check, &args);
    out.notes = report.notes.iter().map(|n| n.to_string()).collect();
    out.texts = report.findings.iter().map(|f| f.agent_text()).collect();
    // what the checker attributes: a finding at a line of a file
    let named: Vec<&forge_core::Finding> = report
        .findings
        .iter()
        .filter(|f| f.path.is_some() && f.line.is_some())
        .collect();
    let names_file = !named.is_empty();
    let names_change = named.iter().any(|f| r.names_change(f));
    out.counted = case.count(r, out.texts.clone(), names_file, names_change);
    out
}
