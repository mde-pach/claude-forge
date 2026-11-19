//! Forge's pinned tools (gitleaks, osv-scanner, editorconfig-checker, lockfile-lint), through
//! forge-check's [`forge_check::builtins::BuiltinTools`]. A tool a builtin cannot have is a note
//! for the person (the check then passes), never a finding.

use std::process::Command;

use forge_core::{Note, NoteKind};

use crate::Options;

/// The words that run Forge's pinned `name`; a note when it cannot be had.
pub fn pinned(opts: &Options, name: &str) -> Result<Vec<String>, Note> {
    opts.tools.pinned(name).map_err(|why| {
        Note::new(
            NoteKind::MissingTool,
            format!("{why}, so this check is skipped"),
        )
    })
}

/// A command running `words` (a pinned tool's), its own arguments to follow.
pub fn command(words: &[String]) -> Command {
    let mut c = Command::new(words.first().map(String::as_str).unwrap_or_default());
    c.args(words.iter().skip(1));
    c
}
