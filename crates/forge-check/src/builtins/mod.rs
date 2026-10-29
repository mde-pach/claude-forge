//! The builtins pack checks call from their `run` commands (`forge_do ruff lint {files}`,
//! `forge_helper python ratchet.py`, `forge_hadolint DL3020`, …), dispatched by
//! `forge internal <name> [args…]`.
//!
//! The checker implements the machinery builtins (adapters, advice, helpers and ratchets,
//! roles, test integrity…). Domain builtins (docker, sql, kubernetes, …) and the Rust ports of
//! the engine helpers plug in through [`Registry::register`] and [`Registry::register_helper`]
//! as [`forge_core::builtin::Builtin`]s. A builtin nobody registered writes a note ("not
//! available yet") and exits 0: Forge's own gaps never fail a check.
//!
//! Context, beyond [`BuiltinCtx`] (forge-core's protocol), comes from the variables below,
//! which the checker sets for each check command.

mod machinery;
pub use machinery::{ACCEPT_HINT, RATCHET_HINT, is_local_db};
mod repo;
mod tests;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_core::builtin::{Builtin, BuiltinCtx, Scope};
use forge_core::{Note, NoteKind, Report, Severity};
use forge_packs::model::Rule;

use crate::change::Added;
use crate::scope::{Roles, RuleScope};
use crate::tools::ToolResolver;

/// The check's files: its in-scope changed files, newline-separated (also `$FORGE_FILES` in
/// the command).
pub const ENV_FILES: &str = "FORGE_FILES";
/// The turn snapshot directory (`--turn`).
pub const ENV_SNAPSHOT: &str = "FORGE_SNAPSHOT";
/// A file a builtin creates when its failure is advice (§10.7), not a failing check.
pub const ENV_ADVISED: &str = "FORGE_ADVISED";
/// A JSON file: every file role's effective patterns.
pub const ENV_ROLES: &str = "FORGE_ROLES";
/// A file of the added lines: `<path><TAB><line>`, `<path><TAB>*` for a new file.
pub const ENV_ADDED: &str = "FORGE_ADDED";
/// A JSON file: the running check's rule and the workspace packages it runs in.
pub const ENV_CHECK_RULE: &str = "FORGE_CHECK_RULE";
/// A file listing the rule ids of the active checks.
pub const ENV_CHECKS: &str = "FORGE_CHECKS";
/// `changed`, `since`, `all` or `turn`.
pub const ENV_MODE: &str = "FORGE_MODE";
/// The repository root, when the command runs in a workspace package.
pub const ENV_WS_ROOT: &str = "FORGE_WS_ROOT";

/// The running check's rule, as the checker hands it to builtins.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckRule {
    pub rule: Rule,
    #[serde(default)]
    pub dirs: Option<Vec<String>>,
}

/// What a builtin call knows: forge-core's context and the checker's.
pub struct Invocation<'a> {
    pub ctx: BuiltinCtx,
    pub args: Vec<String>,
    /// The check's files.
    pub files: Vec<String>,
    pub snapshot: Option<PathBuf>,
    pub mode: String,
    pub advised: Option<PathBuf>,
    pub roles: Roles,
    pub added: BTreeMap<String, Added>,
    pub check: Option<CheckRule>,
    pub checks: Vec<String>,
    pub cwd: PathBuf,
    pub ws_root: Option<PathBuf>,
    pub registry: &'a Registry,
    pub tools: Arc<dyn ToolResolver>,
}

impl Invocation<'_> {
    /// The context the checker put in the environment.
    pub fn from_env<'r>(
        registry: &'r Registry,
        args: Vec<String>,
        tools: Arc<dyn ToolResolver>,
    ) -> Invocation<'r> {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let read = |k: &str| var(k).and_then(|p| std::fs::read_to_string(p).ok());
        let files = std::env::var(ENV_FILES)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        let roles = read(ENV_ROLES)
            .and_then(|t| serde_json::from_str::<BTreeMap<String, Vec<String>>>(&t).ok())
            .map(Roles::from_map)
            .unwrap_or_else(Roles::defaults);
        let added = read(ENV_ADDED).map(|t| parse_added(&t)).unwrap_or_default();
        let check = read(ENV_CHECK_RULE).and_then(|t| serde_json::from_str(&t).ok());
        let checks = read(ENV_CHECKS)
            .map(|t| t.lines().map(str::to_string).collect())
            .unwrap_or_default();
        Invocation {
            ctx: BuiltinCtx::from_env(),
            args,
            files,
            snapshot: var(ENV_SNAPSHOT),
            mode: std::env::var(ENV_MODE).unwrap_or_else(|_| "all".into()),
            advised: var(ENV_ADVISED),
            roles,
            added,
            check,
            checks,
            cwd: std::env::current_dir().unwrap_or_default(),
            ws_root: var(ENV_WS_ROOT),
            registry,
            tools,
        }
    }

    pub fn root(&self) -> &Path {
        &self.ctx.root
    }

    pub fn rule(&self) -> &str {
        &self.ctx.rule
    }

    pub fn turn(&self) -> bool {
        self.ctx.scope == Scope::Turn && self.snapshot.is_some()
    }

    pub fn note(&self, kind: NoteKind, text: impl Into<String>) {
        let mut n = Note::new(kind, text);
        if !self.ctx.rule.is_empty() {
            n = n.for_rule(self.ctx.rule.clone());
        }
        self.ctx.write_notes(&[n]);
    }

    /// Marks the running check's failure as advice.
    pub fn mark_advised(&self) {
        if let Some(p) = &self.advised {
            let _ = std::fs::write(p, b"");
        }
    }

    /// The base copy of a changed file.
    pub fn base_copy(&self, path: &str) -> Option<PathBuf> {
        let p = self.ctx.base.as_ref()?.join(path);
        p.is_file().then_some(p)
    }

    /// The changed files that exist now.
    pub fn existing(&self) -> Vec<String> {
        self.ctx
            .changed
            .iter()
            .filter(|p| self.root().join(p).is_file())
            .cloned()
            .collect()
    }

    /// The running check's scope.
    pub fn scope(&self) -> Option<RuleScope> {
        let c = self.check.as_ref()?;
        let config = c.rule.enforce.iter().find_map(|t| match t {
            forge_packs::model::Target::Check(ct) => ct.config_files.clone(),
            _ => None,
        });
        Some(RuleScope::new(
            &c.rule,
            &self.roles,
            c.dirs.as_deref(),
            config.as_deref(),
        ))
    }

    /// `forge_scope_files`: the check's files, else (`--all`, or a `when_changed` file changed
    /// and none of its own) every tracked file in its scope.
    pub fn scope_files(&self) -> Vec<String> {
        if !self.files.is_empty() {
            return self.files.clone();
        }
        let git = crate::change::Git::new(self.root());
        let scope = self.scope();
        crate::change::split_z(&git.run(&["ls-files", "-z"]).unwrap_or_default())
            .filter(|p| !crate::scope::is_excluded(p) && self.root().join(p).is_file())
            .filter(|p| scope.as_ref().is_none_or(|s| s.matches(p)))
            .collect()
    }
}

/// Reads an added-lines file.
pub fn parse_added(text: &str) -> BTreeMap<String, Added> {
    let mut out: BTreeMap<String, Added> = BTreeMap::new();
    for l in text.lines() {
        let Some((p, n)) = l.rsplit_once('\t') else {
            continue;
        };
        if n == "*" {
            out.insert(p.to_string(), Added::Whole);
        } else if let Ok(n) = n.parse::<u32>() {
            match out
                .entry(p.to_string())
                .or_insert_with(|| Added::Lines(Default::default()))
            {
                Added::Lines(s) => {
                    s.insert(n);
                }
                Added::Whole => {}
            }
        }
    }
    out
}

/// Writes an added-lines file's text.
pub fn render_added(added: &BTreeMap<String, Added>) -> String {
    let mut s = String::new();
    for (p, a) in added {
        match a {
            Added::Whole => s.push_str(&format!("{p}\t*\n")),
            Added::Lines(l) => {
                for n in l {
                    s.push_str(&format!("{p}\t{n}\n"));
                }
            }
        }
    }
    s
}

/// A builtin of the checker's own: writes its output and returns its exit status.
pub type RawFn = fn(&Invocation<'_>, &mut dyn Write, &mut dyn Write) -> i32;

/// The builtins `forge internal` dispatches.
pub struct Registry {
    raw: BTreeMap<&'static str, RawFn>,
    builtins: BTreeMap<&'static str, Box<dyn Builtin + Send>>,
    helpers: BTreeMap<String, Box<dyn Builtin + Send>>,
}

impl Default for Registry {
    fn default() -> Self {
        Registry::new()
    }
}

impl Registry {
    /// The checker's machinery builtins.
    pub fn new() -> Registry {
        let mut r = Registry {
            raw: BTreeMap::new(),
            builtins: BTreeMap::new(),
            helpers: BTreeMap::new(),
        };
        machinery::register(&mut r);
        repo::register(&mut r);
        tests::register(&mut r);
        r
    }

    pub fn register_raw(&mut self, name: &'static str, f: RawFn) {
        self.raw.insert(name, f);
    }

    /// A domain builtin (`forge_<name>`); replaces one of the same name.
    pub fn register(&mut self, b: Box<dyn Builtin + Send>) {
        self.raw.remove(b.name());
        self.builtins.insert(b.name(), b);
    }

    /// An engine helper (forge-helpers' builtins), by its name: the stem of the script packs
    /// name (`forge_helper python ratchet.py` runs the helper `ratchet`).
    pub fn register_helper(&mut self, b: Box<dyn Builtin + Send>) {
        self.helpers.insert(b.name().to_string(), b);
    }

    pub fn has(&self, name: &str) -> bool {
        self.raw.contains_key(name) || self.builtins.contains_key(name)
    }

    /// The helper a pack's script name stands for (`ratchet.py`, `new_deps.mjs`, `ratchet`).
    pub fn helper(&self, script: &str) -> Option<&(dyn Builtin + Send)> {
        let stem = script.rsplit('/').next().unwrap_or(script);
        let stem = stem
            .strip_suffix(".py")
            .or_else(|| stem.strip_suffix(".mjs"))
            .unwrap_or(stem);
        self.helpers.get(stem).map(|b| b.as_ref())
    }

    /// The helpers it has, by name.
    pub fn helper_names(&self) -> Vec<&str> {
        self.helpers.keys().map(String::as_str).collect()
    }

    /// The names it implements.
    pub fn names(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self
            .raw
            .keys()
            .chain(self.builtins.keys())
            .copied()
            .collect();
        v.sort();
        v
    }

    /// Runs builtin `name`; unknown → a note, status 0.
    pub fn dispatch(
        &self,
        name: &str,
        inv: &Invocation<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> i32 {
        if let Some(f) = self.raw.get(name) {
            return f(inv, out, err);
        }
        if let Some(b) = self.builtins.get(name) {
            let report = b.run(&inv.ctx, &inv.args);
            return finish(inv, report, out);
        }
        inv.note(
            NoteKind::NotEnforced,
            format!("not enforced: forge_{name} is not available yet in this Forge"),
        );
        0
    }
}

/// A builtin's report by the protocol: findings on stdout (accepted ones dropped), notes to
/// the notes file; 1 when an error remains; advice alone marks the check as advice.
pub fn finish(inv: &Invocation<'_>, mut report: Report, out: &mut dyn Write) -> i32 {
    report
        .findings
        .retain(|f| !inv.ctx.accepted.iter().any(|a| *a == f.agent_text()));
    inv.ctx.write_notes(&report.notes);
    for f in &report.findings {
        let _ = writeln!(out, "{}", f.agent_text());
    }
    if report.fails() {
        return 1;
    }
    if report
        .findings
        .iter()
        .any(|f| f.severity == Severity::Advice)
    {
        inv.mark_advised();
        return 1;
    }
    0
}

/// `forge internal <name> [args…]`: the entry point of a builtin process; `tools` answers
/// where its tools are (see [`crate::tools::ENV_DOWNLOAD`] for whether it may download).
pub fn internal_main(
    registry: &Registry,
    name: &str,
    args: Vec<String>,
    tools: Arc<dyn ToolResolver>,
) -> i32 {
    let inv = Invocation::from_env(registry, args, tools);
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    let name = name.strip_prefix("forge_").unwrap_or(name);
    let code = registry.dispatch(name, &inv, &mut out, &mut err);
    let _ = out.flush();
    code
}
