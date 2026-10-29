//! What a builtin knows: forge-core's [`BuiltinCtx`], the checker's [`Facts`], and the
//! repository questions the shell functions asked (`forge_role`, `forge_role_files`, the
//! change set's `changed`, `existing`, `inbase`, `deleted` and base copies).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use forge_core::builtin::{BuiltinCtx, Scope};
use forge_core::{Finding, Note, NoteKind, Report};
use regex::Regex;

use crate::Options;

/// The check's files (its in-scope changed files that exist), newline-separated. Set by the
/// checker for each check command (forge-check's `ENV_FILES`).
pub const ENV_FILES: &str = "FORGE_FILES";
/// `changed`, `since`, `all` or `turn` (forge-check's `ENV_MODE`).
pub const ENV_MODE: &str = "FORGE_MODE";
/// A JSON file: every file role's effective patterns, by name (forge-check's `ENV_ROLES`).
pub const ENV_ROLES: &str = "FORGE_ROLES";

/// How the check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The whole project, no base (CI's `--all`).
    All,
    /// The working tree against HEAD (pre-commit).
    Changed,
    /// Against a merge base (`--since`).
    Since,
    /// The agent's turn (Stop).
    Turn,
}

/// The checker's context beyond [`BuiltinCtx`].
#[derive(Debug, Clone)]
pub struct Facts {
    /// The check's files: its in-scope changed files that exist; empty with `--all`.
    pub files: Vec<String>,
    pub mode: Mode,
    /// Every file role's effective patterns ("!" before an exclusion); None: Forge's defaults.
    pub roles: Option<BTreeMap<String, Vec<String>>>,
    /// The builtin's standard input (`forge_do alembic heads | forge_alembic_files`); None: the
    /// process's, read when a builtin needs it.
    pub stdin: Option<String>,
    /// `DATABASE_URL` from the environment.
    pub database_url: Option<String>,
}

impl Facts {
    /// The context the checker put in the environment.
    pub fn from_env() -> Facts {
        let files = std::env::var(ENV_FILES)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        let mode = match std::env::var(ENV_MODE).as_deref() {
            Ok("changed") => Mode::Changed,
            Ok("since") => Mode::Since,
            Ok("turn") => Mode::Turn,
            _ => Mode::All,
        };
        let roles = std::env::var_os(ENV_ROLES)
            .filter(|v| !v.is_empty())
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok());
        Facts {
            files,
            mode,
            roles,
            stdin: None,
            database_url: std::env::var("DATABASE_URL").ok().filter(|v| !v.is_empty()),
        }
    }

    /// A check of the change: `files` in `mode`, Forge's default roles.
    pub fn new(mode: Mode, files: Vec<String>) -> Facts {
        Facts {
            files,
            mode,
            roles: None,
            stdin: None,
            database_url: None,
        }
    }
}

/// One builtin run.
pub struct Run<'a> {
    pub ctx: &'a BuiltinCtx,
    pub facts: Facts,
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
        self.facts.mode == Mode::All || self.ctx.scope == Scope::All
    }

    /// The check's files.
    pub fn files(&self) -> &[String] {
        &self.facts.files
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
        let name = role.trim_start_matches('@');
        let globs = match &self.facts.roles {
            Some(m) => m.get(name).cloned().unwrap_or_default(),
            None => forge_packs::roles::default_patterns(name),
        };
        globs_match(&globs, path)
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
        if let Some(s) = &self.facts.stdin {
            return s.clone();
        }
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
        if let Some(u) = &self.facts.database_url {
            return u.clone();
        }
        if let Some(env) = self.read(".env") {
            let re = crate::util::re(r"^[ \t]*(?:export[ \t]+)?DATABASE_URL[ \t]*=[ \t]*(.*)$");
            if let Some(v) = env
                .lines()
                .filter_map(|l| re.captures(l).map(|c| c[1].trim_end().to_owned()))
                .next_back()
            {
                let v = unquote(&v);
                if !v.is_empty() {
                    return v;
                }
            }
        }
        if let Some(ini) = self.read("alembic.ini") {
            let re = crate::util::re(r"^[ \t]*sqlalchemy\.url[ \t]*=[ \t]*(.*)$");
            if let Some(c) = ini.lines().find_map(|l| re.captures(l)) {
                return c[1].trim_end().to_owned();
            }
        }
        String::new()
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

fn unquote(v: &str) -> String {
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return v[1..v.len() - 1].to_owned();
        }
    }
    v.to_owned()
}

/// Whether `path` matches a glob list ("!" before an exclusion; exclusions first).
pub fn globs_match(globs: &[String], path: &str) -> bool {
    let mut any = false;
    for g in globs {
        if let Some(x) = g.strip_prefix('!') {
            if glob_matches(x, path) {
                return false;
            }
        } else if glob_matches(g, path) {
            any = true;
        }
    }
    any
}

/// A glob as the engine's shell `case` patterns matched it (and as the checker matches roles):
/// `*` also matches `/`, `**/` is zero or more directories, `{a,b}` alternatives, `[0-9]` /
/// `[!x]` one character.
pub fn glob_matches(glob: &str, path: &str) -> bool {
    Regex::new(&format!("^(?:{})$", glob_source(glob)))
        .map(|r| r.is_match(path))
        .unwrap_or(false)
}

fn glob_source(glob: &str) -> String {
    let g: Vec<char> = glob.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < g.len() {
        match g[i] {
            '*' => {
                let mut j = i;
                while j < g.len() && g[j] == '*' {
                    j += 1;
                }
                if j - i >= 2 && g.get(j) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i = j + 1;
                    continue;
                }
                out.push_str(".*");
                i = j;
                continue;
            }
            '?' => out.push('.'),
            '[' => match g
                .iter()
                .skip(i + 2)
                .position(|&x| x == ']')
                .map(|p| p + i + 2)
            {
                Some(end) => {
                    let mut body: String = g[i + 1..end].iter().collect();
                    let neg = body.starts_with('!') || body.starts_with('^');
                    if neg {
                        body.remove(0);
                    }
                    let body: String = body
                        .chars()
                        .map(|ch| {
                            if matches!(ch, '\\' | ']' | '^' | '[' | '&' | '~') {
                                format!("\\{ch}")
                            } else {
                                ch.to_string()
                            }
                        })
                        .collect();
                    out.push_str(&format!("[{}{body}]", if neg { "^" } else { "" }));
                    i = end;
                }
                None => out.push_str("\\["),
            },
            '{' => {
                let mut alts = Vec::new();
                let mut depth = 0;
                let mut start = i + 1;
                let mut end = None;
                let mut k = i + 1;
                while k < g.len() && end.is_none() {
                    match g[k] {
                        '{' => depth += 1,
                        '}' if depth > 0 => depth -= 1,
                        '}' => end = Some(k),
                        ',' if depth == 0 => {
                            alts.push(g[start..k].iter().collect::<String>());
                            start = k + 1;
                        }
                        _ => {}
                    }
                    k += 1;
                }
                match end {
                    None => out.push_str("\\{"),
                    Some(end) => {
                        alts.push(g[start..end].iter().collect());
                        let parts: Vec<String> = alts.iter().map(|a| glob_source(a)).collect();
                        out.push_str(&format!("(?:{})", parts.join("|")));
                        i = end;
                    }
                }
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out
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
            "drizzle/*/migration.sql",
            "drizzle/x/migration.sql"
        ));
        assert!(glob_matches("**/.env", ".env"));
        assert!(glob_matches(
            "**/migrations/[0-9]*.py",
            "a/migrations/0001_x.py"
        ));
        assert!(!glob_matches(
            "**/migrations/[0-9]*.py",
            "a/migrations/x.py"
        ));
        let env = forge_packs::roles::default_patterns("env-files");
        assert!(globs_match(&env, "api/.env.local"));
        assert!(!globs_match(&env, ".env.example"));
    }
}
