//! Claude Forge's PreToolUse Bash guard, as a library: the hook runtime fills a [`GuardCtx`]
//! and calls [`GuardSet::evaluate`] (or [`evaluate`]).
//!
//! The command is parsed into the simple commands it runs ([`parse`], tree-sitter-bash), and
//! rules are matched against each of them, never against the raw text: quoted arguments,
//! commit messages and heredoc bodies are data, so `git commit -m "remove chmod 777"` or
//! `gh pr create --body "… git push --force …"` trip nothing. A rule's guard targets are
//! written in the structural form ([`rule::CommandMatch`]), with or without a built-in check
//! ([`checks`]: `ephemeral-exec`, `package-add`, `python-pm`).
//!
//! The answer is deny, ask or nothing. Its reason is the rule's description, plus "Here: …"
//! when a check found something: no rule id, nothing about Forge itself.
//!
//! The rules are the packs' (`target: guard` in their YAML); `CONVERSION.md` records how the
//! former whole-text patterns became structural forms.

pub mod args;
pub mod checks;
pub mod converted;
pub mod ctx;
mod lex;
pub mod parse;
pub mod rule;
pub mod runners;
pub mod set;
pub mod text;

pub use ctx::{FsLookup, GuardCtx, JsPm, Lookup, PythonPm};
pub use rule::{Check, CommandMatch, Decision, GuardSpec, OneOrMany};
pub use set::{GuardRule, GuardSet, Outcome, Verdict};

/// The decision on `command` under `rules`.
pub fn evaluate(rules: &GuardSet, command: &str, ctx: &GuardCtx) -> Outcome {
    rules.evaluate(command, ctx)
}
