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

mod context;
mod domain;
mod machinery;
pub use context::{CheckCtx, CheckRule, RunMode, parse_added, render_added};
pub use domain::{
    AdapterTools, BuiltinTools, CheckBuiltin, NoTools, ToolOutput, ToolRun, rule_note,
};
pub use machinery::{ACCEPT_HINT, RATCHET_HINT, database_url, is_local_db};
mod repo;
mod tests;
pub use tests::{Block, blocks};

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_core::builtin::{Builtin, BuiltinCtx, Scope};
use forge_core::{NoteKind, Report, Severity};

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
/// The run's scratch directory, shared by the checks of one run.
pub const ENV_TMP: &str = "FORGE_TMP";
/// The ty rules the active rules need at error, one per line (§11.2): `forge_do ty` passes
/// them as `--error <rule>` where the project does not configure ty itself.
pub const ENV_TY_RULES: &str = "FORGE_TY_RULES";

/// What a builtin call knows: forge-core's context, the checker's ([`CheckCtx`], which the
/// invocation derefs to), its arguments, and where tools are.
pub struct Invocation<'a> {
    pub ctx: BuiltinCtx,
    pub cx: CheckCtx,
    pub args: Vec<String>,
    pub registry: &'a Registry,
    pub tools: Arc<dyn ToolResolver>,
}

impl std::ops::Deref for Invocation<'_> {
    type Target = CheckCtx;
    fn deref(&self) -> &CheckCtx {
        &self.cx
    }
}

impl Invocation<'_> {
    /// The context the checker put in the environment.
    pub fn from_env<'r>(
        registry: &'r Registry,
        args: Vec<String>,
        tools: Arc<dyn ToolResolver>,
    ) -> Invocation<'r> {
        Invocation {
            ctx: BuiltinCtx::from_env(),
            cx: CheckCtx::from_env(),
            args,
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
        self.ctx.write_notes(&[rule_note(&self.ctx, kind, text)]);
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

    /// `forge_scope_files`: the check's files, else (`--all`, or a `when_changed` file changed
    /// and none of its own) every tracked file in its scope.
    pub fn scope_files(&self) -> Vec<String> {
        self.cx.scope_files(self.root())
    }
}

/// A builtin of the checker's own: writes its output and returns its exit status.
pub type RawFn = fn(&Invocation<'_>, &mut dyn Write, &mut dyn Write) -> i32;

/// The builtins `forge internal` dispatches.
pub struct Registry {
    raw: BTreeMap<&'static str, RawFn>,
    builtins: BTreeMap<&'static str, Box<dyn Builtin + Send>>,
    domain: BTreeMap<&'static str, Box<dyn CheckBuiltin>>,
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
            domain: BTreeMap::new(),
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
        self.domain.remove(b.name());
        self.builtins.insert(b.name(), b);
    }

    /// A domain crate's builtin (`forge_<name>`), which also reads the checker's context;
    /// replaces one of the same name.
    pub fn register_check(&mut self, b: Box<dyn CheckBuiltin>) {
        self.raw.remove(b.name());
        self.builtins.remove(b.name());
        self.domain.insert(b.name(), b);
    }

    /// An engine helper (forge-helpers' builtins), by its name: the stem of the script packs
    /// name (`forge_helper python ratchet.py` runs the helper `ratchet`).
    pub fn register_helper(&mut self, b: Box<dyn Builtin + Send>) {
        self.helpers.insert(b.name().to_string(), b);
    }

    pub fn has(&self, name: &str) -> bool {
        self.raw.contains_key(name)
            || self.builtins.contains_key(name)
            || self.domain.contains_key(name)
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
            .chain(self.domain.keys())
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
        if let Some(b) = self.domain.get(name) {
            let report = b.run(&inv.ctx, &inv.cx, &inv.args);
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

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// A builtin nobody registered is a note for the person and passes: never a finding.
    #[test]
    fn an_unregistered_builtin_is_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let registry = Registry::new();
        let inv = Invocation {
            ctx: BuiltinCtx {
                root: dir.path().to_path_buf(),
                base: None,
                changed: vec![],
                rule: "local/y".into(),
                scope: Scope::All,
                accepted: vec![],
                notes_file: Some(notes.clone()),
            },
            cx: CheckCtx::default(),
            args: vec![],
            registry: &registry,
            tools: Arc::new(crate::tools::LocalTools::default()),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(registry.dispatch("no_such", &inv, &mut out, &mut err), 0);
        assert!(out.is_empty());
        let text = std::fs::read_to_string(notes).unwrap();
        assert!(
            text.contains("not-enforced") && text.contains("local/y"),
            "{text}"
        );
    }
}
