//! What the builtins share: the check they run in ([`Cx`]), the change, file roles and scopes,
//! running tools, accepted changes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use forge_check::builtins::{CheckCtx, ToolRun};
use forge_core::builtin::{BuiltinCtx, Scope};
use forge_core::{Finding, Note, NoteKind, Report};
use regex::Regex;

use crate::Options;

pub use forge_check::scope::is_excluded;

pub fn lines(s: &str) -> Vec<String> {
    s.lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A builtin's view of its check.
pub struct Cx<'a> {
    pub ctx: &'a BuiltinCtx,
    pub check: &'a CheckCtx,
    pub opts: &'a Options,
}

impl<'a> Cx<'a> {
    pub fn new(ctx: &'a BuiltinCtx, check: &'a CheckCtx, opts: &'a Options) -> Cx<'a> {
        Cx { ctx, check, opts }
    }

    pub fn root(&self) -> &Path {
        &self.ctx.root
    }

    /// `--all`: the whole project, no base to compare with.
    pub fn all(&self) -> bool {
        self.ctx.scope == Scope::All || self.ctx.base.is_none()
    }

    pub fn path(&self, p: &str) -> PathBuf {
        self.ctx.root.join(p)
    }

    pub fn exists(&self, p: &str) -> bool {
        self.path(p).is_file()
    }

    pub fn now(&self, p: &str) -> Option<String> {
        std::fs::read_to_string(self.path(p)).ok()
    }

    pub fn is_changed(&self, p: &str) -> bool {
        self.ctx.changed.iter().any(|c| c == p)
    }

    /// The base copy of a changed file.
    pub fn base_path(&self, p: &str) -> Option<PathBuf> {
        let b = self.ctx.base.as_ref()?.join(p);
        b.is_file().then_some(b)
    }

    /// The file as it was at the base: the file itself when it did not change.
    pub fn before(&self, p: &str) -> Option<String> {
        if self.is_changed(p) {
            self.base_path(p)
                .and_then(|b| std::fs::read_to_string(b).ok())
        } else {
            self.now(p)
        }
    }

    /// The changed files that exist now.
    pub fn existing(&self) -> Vec<String> {
        self.ctx
            .changed
            .iter()
            .filter(|p| self.exists(p))
            .cloned()
            .collect()
    }

    /// Whether `path` is one of the role's files.
    pub fn role(&self, role: &str, path: &str) -> bool {
        self.check.roles.matches(role, path)
    }

    /// Whether `path` is in the running rule's scope (true when the rule is unknown).
    pub fn in_rule_scope(&self, path: &str) -> bool {
        self.check.in_scope(path)
    }

    /// `forge_scope_files`: the check's files; else (`--all`, or a `when_changed` file changed
    /// and none of the check's own) every tracked file in the rule's scope.
    pub fn scope_files(&self) -> Vec<String> {
        self.check.scope_files(self.root())
    }

    /// `git <args>` in the root, one path per line (empty when git fails).
    pub fn git_files(&self, args: &[&str]) -> Vec<String> {
        Command::new("git")
            .args(args)
            .current_dir(self.root())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| lines(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }

    /// The repository's files (tracked, and untracked but not ignored) matching `specs`.
    pub fn repo_files(&self, specs: &[&str]) -> Vec<String> {
        let mut args = vec![
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
        ];
        args.extend_from_slice(specs);
        let mut v = self.git_files(&args);
        v.sort();
        v.dedup();
        v
    }

    /// A note about the running rule.
    pub fn note(&self, kind: NoteKind, text: impl Into<String>) -> Note {
        let n = Note::new(kind, text);
        if self.ctx.rule.is_empty() {
            n
        } else {
            n.for_rule(self.ctx.rule.clone())
        }
    }

    pub fn error(&self, msg: impl Into<String>) -> Finding {
        Finding::error(self.ctx.rule.clone(), msg)
    }

    /// A finding at `path:line`; line 0 is about the file as a whole (`path:0 …` on stdout).
    pub fn at(&self, path: &str, line: u32, msg: impl Into<String>) -> Finding {
        self.error(msg).at(path, Some(line))
    }

    /// Forge's pinned `tool`: the words that run it (installed first only where the checker
    /// allows downloads).
    pub fn pinned(&self, tool: &str) -> Result<Vec<String>, Note> {
        self.opts
            .tools
            .pinned(tool)
            .map_err(|why| self.note(NoteKind::MissingTool, why))
    }

    /// Runs `op` of the project's `tool` in `cwd`, `args` after (its adapter's words first).
    pub fn project(&self, tool: &str, op: &str, args: &[String], cwd: &Path) -> Result<Out, Note> {
        match self.opts.tools.run(self.root(), cwd, tool, op, args) {
            ToolRun::Ran(o) => Ok(Out {
                code: Some(o.status),
                stdout: o.stdout,
                stderr: o.stderr,
                error: None,
            }),
            ToolRun::Missing(why) => Err(self.note(NoteKind::MissingTool, why)),
            ToolRun::Unsupported(why) => Err(self.note(NoteKind::NotEnforced, why)),
        }
    }

    /// Whether the user accepted `text` for this rule: an entry holds the finding as it was
    /// reported (a part of it, as the engine matched it).
    pub fn accepted(&self, text: &str) -> bool {
        self.ctx
            .accepted
            .iter()
            .any(|a| a == text || a.contains(text))
    }

    /// The findings the user did not accept, then `hint` when some remain.
    pub fn unaccepted(&self, findings: Vec<Finding>, hint: &str) -> Report {
        let mut r = Report {
            findings: findings
                .into_iter()
                .filter(|f| !self.accepted(&f.agent_text()))
                .collect(),
            notes: vec![],
        };
        if !r.findings.is_empty() {
            r.findings.push(self.error(hint));
        }
        r
    }

    /// A scratch directory: in the run's when there is one (so a later check of the same run
    /// can reuse what it holds), else a temporary one removed with the value.
    pub fn scratch(&self) -> Scratch {
        if let Some(t) = &self.check.tmp
            && t.is_dir()
        {
            return Scratch::Run(t.clone());
        }
        match tempfile::tempdir() {
            Ok(d) => Scratch::Temp(d),
            Err(_) => Scratch::Run(std::env::temp_dir()),
        }
    }

    /// Forge's cache directory for `name` (created), else a scratch one.
    pub fn cache(&self, name: &str, scratch: &Scratch) -> PathBuf {
        if let Some(c) = &self.opts.cache_dir {
            let d = c.join(name);
            if std::fs::create_dir_all(&d).is_ok() {
                return d;
            }
        }
        let d = scratch.path().join(format!("cache-{name}"));
        let _ = std::fs::create_dir_all(&d);
        d
    }
}

pub enum Scratch {
    Run(PathBuf),
    Temp(tempfile::TempDir),
}

impl Scratch {
    pub fn path(&self) -> &Path {
        match self {
            Scratch::Run(p) => p,
            Scratch::Temp(d) => d.path(),
        }
    }
}

/// A tool run's outcome.
#[derive(Debug, Clone, Default)]
pub struct Out {
    /// None: killed by a signal, or it could not start.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Why it could not start.
    pub error: Option<String>,
}

impl Out {
    /// stdout then stderr, colour codes removed.
    pub fn both(&self) -> String {
        let sep = if self.stdout.is_empty() || self.stdout.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        strip_ansi(&format!("{}{sep}{}", self.stdout, self.stderr))
    }
}

/// Runs `argv` in `cwd`, stdin closed.
pub fn run(argv: &[String], cwd: &Path, env: &[(&str, &str)]) -> Out {
    let Some((exe, args)) = argv.split_first() else {
        return Out {
            error: Some("no command".into()),
            ..Out::default()
        };
    };
    let mut c = Command::new(exe);
    c.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env("NO_COLOR", "1");
    for (k, v) in env {
        c.env(k, v);
    }
    match c.output() {
        Ok(o) => Out {
            code: o.status.code(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            error: None,
        },
        Err(e) => Out {
            error: Some(e.to_string()),
            ..Out::default()
        },
    }
}

pub fn path_arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new("\x1b\\[[0-9;]*m").expect("regex"));

pub fn strip_ansi(s: &str) -> String {
    ANSI.replace_all(s, "").into_owned()
}

/// The last non-blank line (why a tool could not run).
pub fn last_line(s: &str) -> String {
    strip_ansi(s)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

pub fn is_exe(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The directory part of a repository path ("." for a file at the root).
pub fn dir_of(p: &str) -> String {
    match p.rfind('/') {
        Some(i) => p[..i].to_string(),
        None => ".".to_string(),
    }
}

/// `dir/name`, or `name` in ".".
pub fn join(dir: &str, name: &str) -> String {
    if dir == "." || dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// The words of a pinned tool, then `args`.
pub fn with(words: &[String], args: Vec<String>) -> Vec<String> {
    let mut v = words.to_vec();
    v.extend(args);
    v
}

/// The executable of a pinned tool's words (a release binary is one word: its path).
pub fn exe_path(words: &[String]) -> PathBuf {
    PathBuf::from(words.last().cloned().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_line_and_dirs() {
        assert_eq!(last_line("a\n\x1b[31mb\x1b[0m\n\n"), "b");
        assert_eq!(dir_of("Dockerfile"), ".");
        assert_eq!(dir_of("a/b/c"), "a/b");
        assert_eq!(join(".", "x"), "x");
    }
}
