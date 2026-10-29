//! What the checker tells a check command beyond forge-core's [`BuiltinCtx`]: the one reader of
//! the variables it sets ([`CheckCtx::from_env`]), shared by the machinery builtins and the
//! domain crates' ([`super::CheckBuiltin`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use forge_core::builtin::{BuiltinCtx, Scope};
use forge_packs::model::Rule;

use super::{
    ENV_ADDED, ENV_ADVISED, ENV_CHECK_RULE, ENV_CHECKS, ENV_FILES, ENV_MODE, ENV_ROLES,
    ENV_SNAPSHOT, ENV_TMP, ENV_WS_ROOT,
};
use crate::change::Added;
use crate::scope::{Roles, RuleScope, is_excluded};

/// The running check's rule, as the checker hands it to builtins.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckRule {
    pub rule: Rule,
    #[serde(default)]
    pub dirs: Option<Vec<String>>,
}

/// How the run compares the change (`FORGE_MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunMode {
    /// The whole project, no base (CI's `--all`).
    #[default]
    All,
    /// The working tree against HEAD (pre-commit).
    Changed,
    /// Against a merge base (`--since`).
    Since,
    /// The agent's turn (hooks).
    Turn,
}

impl RunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::All => "all",
            RunMode::Changed => "changed",
            RunMode::Since => "since",
            RunMode::Turn => "turn",
        }
    }

    pub fn parse(s: &str) -> RunMode {
        match s {
            "changed" => RunMode::Changed,
            "since" => RunMode::Since,
            "turn" => RunMode::Turn,
            _ => RunMode::All,
        }
    }
}

/// The checker's context for one check command.
#[derive(Debug, Clone)]
pub struct CheckCtx {
    /// The check's files: its in-scope changed files that exist. Empty with `--all`, or when
    /// only a `when_changed` file changed.
    pub files: Vec<String>,
    pub mode: RunMode,
    /// The turn snapshot directory (`--turn`).
    pub snapshot: Option<PathBuf>,
    /// The file whose creation marks the check's failure as advice.
    pub advised: Option<PathBuf>,
    /// Every file role's effective patterns.
    pub roles: Roles,
    /// The added lines of the change, by file.
    pub added: BTreeMap<String, Added>,
    /// The running check's rule and the workspace packages it runs in.
    pub check: Option<CheckRule>,
    /// The rule ids of the active checks.
    pub checks: Vec<String>,
    /// Where the command runs (a workspace package, or the root).
    pub cwd: PathBuf,
    /// The repository root, when the command runs in a workspace package.
    pub ws_root: Option<PathBuf>,
    /// The run's scratch directory, shared by the checks of one run.
    pub tmp: Option<PathBuf>,
}

impl Default for CheckCtx {
    fn default() -> Self {
        CheckCtx {
            files: Vec::new(),
            mode: RunMode::All,
            snapshot: None,
            advised: None,
            roles: Roles::defaults(),
            added: BTreeMap::new(),
            check: None,
            checks: Vec::new(),
            cwd: PathBuf::new(),
            ws_root: None,
            tmp: None,
        }
    }
}

impl CheckCtx {
    /// The context the checker put in the environment.
    pub fn from_env() -> CheckCtx {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let read = |k: &str| var(k).and_then(|p| std::fs::read_to_string(p).ok());
        CheckCtx {
            files: lines(&std::env::var(ENV_FILES).unwrap_or_default()),
            mode: RunMode::parse(&std::env::var(ENV_MODE).unwrap_or_default()),
            snapshot: var(ENV_SNAPSHOT),
            advised: var(ENV_ADVISED),
            roles: read(ENV_ROLES)
                .and_then(|t| serde_json::from_str::<BTreeMap<String, Vec<String>>>(&t).ok())
                .map(Roles::from_map)
                .unwrap_or_else(Roles::defaults),
            added: read(ENV_ADDED).map(|t| parse_added(&t)).unwrap_or_default(),
            check: read(ENV_CHECK_RULE).and_then(|t| serde_json::from_str(&t).ok()),
            checks: read(ENV_CHECKS).map(|t| lines(&t)).unwrap_or_default(),
            cwd: std::env::current_dir().unwrap_or_default(),
            ws_root: var(ENV_WS_ROOT),
            tmp: var(ENV_TMP),
        }
    }

    /// The context the checker would give `rule`'s check of a change (`ctx`): the changed files
    /// that exist and are in the rule's scope. For callers outside the checker (tests).
    pub fn for_rule(ctx: &BuiltinCtx, rule: Option<Rule>) -> CheckCtx {
        let mut cx = CheckCtx {
            mode: match ctx.scope {
                Scope::All => RunMode::All,
                Scope::Changed => RunMode::Changed,
                Scope::Turn => RunMode::Turn,
            },
            cwd: ctx.root.clone(),
            check: rule.map(|rule| CheckRule { rule, dirs: None }),
            ..CheckCtx::default()
        };
        if ctx.scope != Scope::All {
            let scope = cx.scope();
            cx.files = ctx
                .changed
                .iter()
                .filter(|p| ctx.root.join(p).is_file() && !is_excluded(p))
                .filter(|p| scope.as_ref().is_none_or(|s| s.matches(p)))
                .cloned()
                .collect();
        }
        cx
    }

    /// The running check's scope.
    pub fn scope(&self) -> Option<RuleScope> {
        let c = self.check.as_ref()?;
        Some(RuleScope::of_check(&c.rule, &self.roles, c.dirs.as_deref()))
    }

    /// Whether `path` is in the running check's scope (true when the rule is unknown).
    pub fn in_scope(&self, path: &str) -> bool {
        self.scope().is_none_or(|s| s.matches(path))
    }

    /// `forge_scope_files`: the check's files, else (`--all`, or a `when_changed` file changed
    /// and none of its own) every tracked file in its scope.
    pub fn scope_files(&self, root: &Path) -> Vec<String> {
        if !self.files.is_empty() {
            return self.files.clone();
        }
        let git = crate::change::Git::new(root);
        let scope = self.scope();
        crate::change::split_z(&git.run(&["ls-files", "-z"]).unwrap_or_default())
            .filter(|p| !is_excluded(p) && root.join(p).is_file())
            .filter(|p| scope.as_ref().is_none_or(|s| s.matches(p)))
            .collect()
    }

    /// Whether the file has added lines.
    pub fn has_added(&self, path: &str) -> bool {
        self.added.contains_key(path)
    }

    /// Whether `line` of `path` was added.
    pub fn on_added(&self, path: &str, line: u32) -> bool {
        self.added.get(path).is_some_and(|a| a.contains(line))
    }
}

fn lines(s: &str) -> Vec<String> {
    s.lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
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
