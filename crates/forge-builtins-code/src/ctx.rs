//! What a builtin knows beyond [`BuiltinCtx`]: the checker's context for the running check, which
//! it passes to a builtin process in the environment (forge-check's `builtins` module names the
//! variables).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use forge_core::builtin::{BuiltinCtx, Scope};
use forge_packs::model::Rule;

use crate::scope::{Roles, RuleScope, is_excluded};
use crate::util::git_lines;

/// The check's files: its in-scope changed files, newline-separated.
pub const ENV_FILES: &str = "FORGE_FILES";
/// A file of the added lines: `<path><TAB><line>`, `<path><TAB>*` for a new file.
pub const ENV_ADDED: &str = "FORGE_ADDED";
/// A JSON file: every file role's effective patterns.
pub const ENV_ROLES: &str = "FORGE_ROLES";
/// A JSON file: the running check's rule and the workspace packages it runs in.
pub const ENV_CHECK_RULE: &str = "FORGE_CHECK_RULE";
/// A file listing the rule ids of the active checks.
pub const ENV_CHECKS: &str = "FORGE_CHECKS";
/// The turn snapshot directory (`--turn`).
pub const ENV_SNAPSHOT: &str = "FORGE_SNAPSHOT";

/// The added lines of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    /// A new file: every line.
    Whole,
    Lines(BTreeSet<u32>),
}

impl Added {
    pub fn contains(&self, line: u32) -> bool {
        match self {
            Added::Whole => true,
            Added::Lines(l) => l.contains(&line),
        }
    }
}

/// The running check's rule, as the checker writes it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CheckRule {
    pub rule: Rule,
    #[serde(default)]
    pub dirs: Option<Vec<String>>,
}

/// The checker's context for a builtin call.
#[derive(Debug, Clone, Default)]
pub struct Extra {
    /// The check's files (in its scope, changed); empty with `--all`, or when only a
    /// `when_changed` file changed.
    pub files: Vec<String>,
    pub added: BTreeMap<String, Added>,
    pub roles: Roles,
    pub check: Option<CheckRule>,
    /// The rule ids of the active checks.
    pub checks: Vec<String>,
    pub snapshot: Option<PathBuf>,
}

impl Extra {
    /// The context the checker put in the environment.
    pub fn from_env(_ctx: &BuiltinCtx) -> Extra {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let read = |k: &str| var(k).and_then(|p| std::fs::read_to_string(p).ok());
        Extra {
            files: std::env::var(ENV_FILES)
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect(),
            added: read(ENV_ADDED).map(|t| parse_added(&t)).unwrap_or_default(),
            roles: read(ENV_ROLES)
                .and_then(|t| serde_json::from_str::<BTreeMap<String, Vec<String>>>(&t).ok())
                .map(Roles::from_map)
                .unwrap_or_else(Roles::defaults),
            check: read(ENV_CHECK_RULE).and_then(|t| serde_json::from_str(&t).ok()),
            checks: read(ENV_CHECKS)
                .map(|t| t.lines().map(str::to_string).collect())
                .unwrap_or_default(),
            snapshot: var(ENV_SNAPSHOT),
        }
    }

    /// `forge_scope_files`: the check's files, else every tracked file in its scope.
    pub fn scope_files(&self, ctx: &BuiltinCtx) -> Vec<String> {
        if !self.files.is_empty() {
            return self.files.clone();
        }
        let scope = self
            .check
            .as_ref()
            .map(|c| RuleScope::of_check(&c.rule, &self.roles, c.dirs.as_deref()));
        git_lines(&ctx.root, &["ls-files"])
            .into_iter()
            .filter(|p| !is_excluded(p) && ctx.root.join(p).is_file())
            .filter(|p| scope.as_ref().is_none_or(|s| s.matches(p)))
            .collect()
    }

    /// Whether the file has added lines.
    pub fn has_added(&self, path: &str) -> bool {
        self.added.contains_key(path)
    }

    pub fn on_added(&self, path: &str, line: u32) -> bool {
        self.added.get(path).is_some_and(|a| a.contains(line))
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
                .or_insert_with(|| Added::Lines(BTreeSet::new()))
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

/// The changed files that exist now.
pub fn existing(ctx: &BuiltinCtx) -> Vec<String> {
    ctx.changed
        .iter()
        .filter(|p| ctx.root.join(p).is_file())
        .cloned()
        .collect()
}

/// The base copy of a changed file, when it was in the base.
pub fn base_copy(ctx: &BuiltinCtx, path: &str) -> Option<PathBuf> {
    let p = ctx.base.as_ref()?.join(path);
    p.is_file().then_some(p)
}

/// `--all`: the whole project, no base to compare with.
pub fn is_all(ctx: &BuiltinCtx) -> bool {
    ctx.scope == Scope::All
}

pub fn read(root: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(root.join(path)).ok()
}
