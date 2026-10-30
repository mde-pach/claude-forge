//! Builtins: the `forge_*` functions that pack checks call from their `run` commands
//! (`forge_do ruff lint {files}`, `forge_helper python ratchet.py`, `forge_hadolint`, …).
//!
//! A check's `run` is shell. The checker runs it with `sh -c`, after a preamble that defines each
//! builtin as a shell function calling `<forge executable> internal <name> "$@"`, so builtins compose
//! with pipes and `$(…)` as before. The checker passes the context in the environment ([`ENV_ROOT`]…);
//! a builtin process reads it with [`BuiltinCtx::from_env`].
//!
//! Output protocol of a builtin process:
//! - stdout: its findings, as the agent reads them (`path:line message`, one per line; continuation
//!   lines start with whitespace). The checker attributes them to the check's rule.
//! - notes for the person: JSON lines appended to the file named by [`ENV_NOTES`] (never stdout).
//! - exit status: 0 when nothing fails, 1 when a finding fails the check. A builtin that cannot run
//!   (missing tool, Forge failure) writes a note and exits 0: Forge's own problems never fail a check.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Note, Report};

/// The repository root (absolute).
pub const ENV_ROOT: &str = "FORGE_ROOT";
/// A directory holding the base copy of the changed files (what the change is compared with); unset
/// when there is no base (`--all`).
pub const ENV_BASE: &str = "FORGE_BASE";
/// A file listing the changed files, repository-relative, one per line; unset with `--all`.
pub const ENV_CHANGED: &str = "FORGE_CHANGED";
/// The id of the rule whose check is running.
pub const ENV_RULE: &str = "FORGE_RULE";
/// `all`, `changed` or `turn`.
pub const ENV_SCOPE: &str = "FORGE_SCOPE";
/// The file notes are appended to.
pub const ENV_NOTES: &str = "FORGE_NOTES";
/// The file holding the changes the user accepted for this rule (forge.yaml `accept:`), one per line.
pub const ENV_ACCEPTED: &str = "FORGE_ACCEPTED";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// The whole project (CI's `--all`).
    All,
    /// The files changed against a git base (`--changed`, `--since`).
    Changed,
    /// The files changed during the agent's turn (hooks).
    Turn,
}

/// What a builtin knows about the check it runs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltinCtx {
    pub root: PathBuf,
    pub base: Option<PathBuf>,
    /// Changed files, repository-relative; empty with [`Scope::All`].
    pub changed: Vec<String>,
    pub rule: String,
    pub scope: Scope,
    /// Changes the user accepted for this rule: a finding whose text equals one is dropped.
    pub accepted: Vec<String>,
    pub notes_file: Option<PathBuf>,
}

impl BuiltinCtx {
    /// The context the checker put in the environment.
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        let lines = |k: &str| -> Vec<String> {
            var(k)
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|s| {
                    s.lines()
                        .filter(|l| !l.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let scope = match var(ENV_SCOPE).as_deref().and_then(|s| s.to_str()) {
            Some("changed") => Scope::Changed,
            Some("turn") => Scope::Turn,
            _ => Scope::All,
        };
        BuiltinCtx {
            root: var(ENV_ROOT)
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            base: var(ENV_BASE).map(PathBuf::from),
            changed: lines(ENV_CHANGED),
            rule: var(ENV_RULE)
                .and_then(|v| v.into_string().ok())
                .unwrap_or_default(),
            scope,
            accepted: lines(ENV_ACCEPTED),
            notes_file: var(ENV_NOTES).map(PathBuf::from),
        }
    }

    /// Appends notes to the notes file (or drops them when there is none).
    pub fn write_notes(&self, notes: &[Note]) {
        let Some(path) = &self.notes_file else { return };
        if notes.is_empty() {
            return;
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            for n in notes {
                if let Ok(line) = serde_json::to_string(n) {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
    }

    /// Prints a builtin's report by the protocol above and returns its exit status.
    pub fn finish(&self, mut report: Report) -> i32 {
        report
            .findings
            .retain(|f| !self.accepted.iter().any(|a| *a == f.agent_text()));
        self.write_notes(&report.notes);
        let out = report
            .findings
            .iter()
            .map(|f| f.agent_text())
            .collect::<Vec<_>>()
            .join("\n");
        if !out.is_empty() {
            println!("{out}");
        }
        i32::from(report.fails())
    }
}

/// A builtin implemented in Rust.
pub trait Builtin: Sync {
    /// The name without the `forge_` prefix (`do`, `helper`, `hadolint`, …).
    fn name(&self) -> &'static str;
    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report;
}
