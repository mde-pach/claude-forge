//! What a builtin knows beyond [`BuiltinCtx`]: forge-check's [`Extra`] (its `CheckCtx`, the one
//! reader of what the checker passes a check command), and small readers of the change.

use std::path::{Path, PathBuf};

use forge_core::builtin::{BuiltinCtx, Scope};

pub use forge_check::builtins::{CheckCtx as Extra, CheckRule, parse_added};
pub use forge_check::change::Added;

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
