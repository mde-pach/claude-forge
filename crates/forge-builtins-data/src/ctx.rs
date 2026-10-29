//! What a builtin knows: forge-core's [`BuiltinCtx`], the checker's [`Facts`], and the
//! repository questions the shell functions asked (`forge_role`, `forge_role_files`, the
//! change set's `changed`, `existing`, `inbase`, `deleted` and base copies).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use forge_check::builtins::{CheckCtx, RunMode};
use forge_core::builtin::{BuiltinCtx, Scope};
use forge_core::{Finding, Note, NoteKind, Report};

use crate::Options;

/// One builtin run.
pub struct Run<'a> {
    pub ctx: &'a BuiltinCtx,
    pub check: &'a CheckCtx,
    pub opts: &'a Options,
}

static STDIN: OnceLock<String> = OnceLock::new();

impl Run<'_> {
    pub fn root(&self) -> &Path {
        &self.ctx.root
    }

    pub fn path(&self, p: &str) -> PathBuf {
        self.ctx.root.join(p)
    }

    pub fn exists(&self, p: &str) -> bool {
        self.path(p).is_file()
    }

    pub fn read(&self, p: &str) -> Option<String> {
        std::fs::read(self.path(p))
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// The base copy of a changed file.
    pub fn base_path(&self, p: &str) -> Option<PathBuf> {
        let b = self.ctx.base.as_ref()?.join(p);
        b.is_file().then_some(b)
    }

    pub fn base(&self, p: &str) -> Option<String> {
        self.base_path(p)
            .and_then(|b| std::fs::read(b).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// `--all`: no change to compare.
    pub fn all(&self) -> bool {
        self.check.mode == RunMode::All || self.ctx.scope == Scope::All
    }

    /// The check's files.
    pub fn files(&self) -> &[String] {
        &self.check.files
    }

    /// Every changed file (deleted ones included).
    pub fn changed(&self) -> &[String] {
        &self.ctx.changed
    }

    /// The changed files that exist now.
    pub fn existing(&self) -> Vec<String> {
        self.changed()
            .iter()
            .filter(|p| self.exists(p))
            .cloned()
            .collect()
    }

    /// The changed files that exist in the base.
    pub fn inbase(&self) -> Vec<String> {
        self.changed()
            .iter()
            .filter(|p| self.base_path(p).is_some())
            .cloned()
            .collect()
    }

    /// The changed files the change deleted.
    pub fn deleted(&self) -> Vec<String> {
        self.changed()
            .iter()
            .filter(|p| !self.exists(p) && self.base_path(p).is_some())
            .cloned()
            .collect()
    }

    /// The files the change added (they exist now, not in the base).
    pub fn added(&self) -> Vec<String> {
        self.changed()
            .iter()
            .filter(|p| self.exists(p) && self.base_path(p).is_none())
            .cloned()
            .collect()
    }

    /// `forge_role <@role> <path>`.
    pub fn role(&self, role: &str, path: &str) -> bool {
        self.check.roles.matches(role, path)
    }

    /// `git <args>` in the root: its stdout, None when it fails.
    pub fn git(&self, args: &[&str]) -> Option<String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.root())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// `git ls-files <args>`, one path per entry.
    pub fn ls_files(&self, args: &[&str]) -> Vec<String> {
        let mut a = vec!["ls-files", "-z"];
        a.extend_from_slice(args);
        self.git(&a)
            .unwrap_or_default()
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// `forge_role_files <@role>`: the repository's files (tracked, and untracked but not
    /// ignored) of a role, in git's order.
    pub fn role_files(&self, role: &str) -> Vec<String> {
        self.ls_files(&["-co", "--exclude-standard"])
            .into_iter()
            .filter(|f| self.role(role, f))
            .collect()
    }

    /// The builtin's standard input.
    pub fn stdin(&self) -> String {
        STDIN
            .get_or_init(|| {
                let mut s = String::new();
                let _ = std::io::stdin().read_to_string(&mut s);
                s
            })
            .clone()
    }

    /// `forge_db_url`: DATABASE_URL from the environment, then .env, then alembic.ini.
    pub fn db_url(&self) -> String {
        forge_check::builtins::database_url(self.root()).unwrap_or_default()
    }

    pub fn finding(&self, message: impl Into<String>) -> Finding {
        Finding::error(self.ctx.rule.clone(), message)
    }

    pub fn at(&self, path: &str, line: Option<u32>, message: impl Into<String>) -> Finding {
        self.finding(message).at(path, line)
    }

    pub fn note(&self, kind: NoteKind, text: impl Into<String>) -> Note {
        let n = Note::new(kind, text);
        if self.ctx.rule.is_empty() {
            n
        } else {
            n.for_rule(self.ctx.rule.clone())
        }
    }

    /// A report holding one note.
    pub fn only_note(&self, kind: NoteKind, text: impl Into<String>) -> Report {
        Report {
            findings: vec![],
            notes: vec![self.note(kind, text)],
        }
    }
}

/// Whether `path` matches a glob (shell `case` semantics, as the checker matches).
pub fn glob_matches(glob: &str, path: &str) -> bool {
    forge_check::scope::glob_regex(glob).is_match(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_globs() {
        assert!(glob_matches(
            "supabase/migrations/*.sql",
            "supabase/migrations/a.sql"
        ));
        assert!(glob_matches(
            "**/migrations/[0-9]*.py",
            "a/migrations/0001_x.py"
        ));
        assert!(!glob_matches(
            "**/migrations/[0-9]*.py",
            "a/migrations/x.py"
        ));
    }
}
