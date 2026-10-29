//! What the builtins share: the check they run in ([`Cx`]), the change, file roles and scopes,
//! running tools, accepted changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use forge_core::builtin::{BuiltinCtx, Scope};
use forge_core::{Finding, Note, NoteKind, Report};
use forge_packs::model::{Language, Rule};
use regex::Regex;

use crate::Options;

/// The checker's variables for a check command, beyond forge-core's [`BuiltinCtx`].
pub const ENV_FILES: &str = "FORGE_FILES";
pub const ENV_ROLES: &str = "FORGE_ROLES";
pub const ENV_CHECK_RULE: &str = "FORGE_CHECK_RULE";
pub const ENV_TMP: &str = "FORGE_TMP";

/// What the checker passes a check command besides [`BuiltinCtx`].
#[derive(Debug, Clone, Default)]
pub struct Extra {
    /// The check's files: its in-scope changed files that exist (`FORGE_FILES`). Empty or None:
    /// every tracked file in the rule's scope (`--all`, or a `when_changed` file changed).
    pub files: Option<Vec<String>>,
    /// Every file role's effective patterns (`FORGE_ROLES`); None: Forge's defaults.
    pub roles: Option<BTreeMap<String, Vec<String>>>,
    /// The running rule (`FORGE_CHECK_RULE`), for its scope.
    pub rule: Option<Rule>,
    /// The workspace packages the rule runs in.
    pub dirs: Option<Vec<String>>,
    /// The run's scratch directory (`FORGE_TMP`), shared by the checks of one run.
    pub tmp: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
struct CheckRule {
    rule: Rule,
    #[serde(default)]
    dirs: Option<Vec<String>>,
}

impl Extra {
    pub fn from_env() -> Extra {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        let read = |k: &str| var(k).and_then(|p| std::fs::read_to_string(p).ok());
        let check = read(ENV_CHECK_RULE).and_then(|t| serde_json::from_str::<CheckRule>(&t).ok());
        Extra {
            files: std::env::var(ENV_FILES).ok().map(|s| lines(&s)),
            roles: read(ENV_ROLES).and_then(|t| serde_json::from_str(&t).ok()),
            dirs: check.as_ref().and_then(|c| c.dirs.clone()),
            rule: check.map(|c| c.rule),
            tmp: var(ENV_TMP).map(PathBuf::from),
        }
    }
}

pub fn lines(s: &str) -> Vec<String> {
    s.lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A builtin's view of its check.
pub struct Cx<'a> {
    pub ctx: &'a BuiltinCtx,
    pub extra: Extra,
    pub opts: &'a Options,
    roles: BTreeMap<String, Vec<String>>,
}

/// Forge's own files and the paths no rule looks at.
pub fn is_excluded(path: &str) -> bool {
    const DIRS: &[&str] = &[".git", "node_modules", ".venv"];
    path.starts_with(".forge/")
        || path.starts_with(".claude/skills/forge-project/")
        || path.starts_with(".claude/worktrees/")
        || path.split('/').rev().skip(1).any(|s| DIRS.contains(&s))
}

impl<'a> Cx<'a> {
    pub fn new(ctx: &'a BuiltinCtx, extra: Extra, opts: &'a Options) -> Cx<'a> {
        let roles = extra.roles.clone().unwrap_or_else(|| {
            forge_packs::roles::ROLES
                .iter()
                .map(|(n, _)| (n.to_string(), forge_packs::roles::default_patterns(n)))
                .collect()
        });
        Cx {
            ctx,
            extra,
            opts,
            roles,
        }
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
        self.roles
            .get(role.trim_start_matches('@'))
            .is_some_and(|g| GlobList::new(g).matches(path))
    }

    fn expand(&self, globs: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for g in globs {
            match forge_packs::roles::role_ref(g) {
                Some((neg, name)) => {
                    for p in self.roles.get(&name).into_iter().flatten() {
                        let (pneg, pg) = match p.strip_prefix('!') {
                            Some(r) => (true, r),
                            None => (false, p.as_str()),
                        };
                        out.push(if neg != pneg {
                            format!("!{pg}")
                        } else {
                            pg.to_string()
                        });
                    }
                }
                None => out.push(g.clone()),
            }
        }
        out
    }

    /// Whether `path` is in the running rule's scope (true when the rule is unknown).
    pub fn in_rule_scope(&self, path: &str) -> bool {
        let Some(rule) = &self.extra.rule else {
            return true;
        };
        let files = self.expand(rule.scope.files.as_deref().unwrap_or(&[]));
        let files = in_dirs(&files, self.extra.dirs.as_deref());
        if !GlobList::new(&files).matches(path) {
            return false;
        }
        let lang: &[&str] = match rule.scope.language {
            Language::Python => &["*.py", "*.pyi"],
            Language::Typescript => &[
                "*.ts", "*.tsx", "*.mts", "*.cts", "*.js", "*.jsx", "*.mjs", "*.cjs",
            ],
            Language::Tsx => &["*.tsx", "*.jsx"],
            Language::Yaml => &["*.yaml", "*.yml"],
            Language::Any => &[],
        };
        lang.is_empty() || lang.iter().any(|g| glob_regex(g).is_match(path))
    }

    /// `forge_scope_files`: the check's files; else (`--all`, or a `when_changed` file changed
    /// and none of the check's own) every tracked file in the rule's scope. Outside the checker
    /// (no [`Extra::files`]), the check's files are the changed files that exist in its scope.
    pub fn scope_files(&self) -> Vec<String> {
        let files = match &self.extra.files {
            Some(f) => f.clone(),
            None if !self.all() => self
                .existing()
                .into_iter()
                .filter(|p| !is_excluded(p) && self.in_rule_scope(p))
                .collect(),
            None => vec![],
        };
        if !files.is_empty() {
            return files;
        }
        self.git_files(&["ls-files"])
            .into_iter()
            .filter(|p| !is_excluded(p) && self.exists(p) && self.in_rule_scope(p))
            .collect()
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

    /// Forge's pinned `tool`: installed only when this is not a hook (see the crate docs).
    pub fn pinned(&self, tool: &str) -> Result<PathBuf, Note> {
        let r = if self.ctx.scope == Scope::Turn {
            self.opts.pinned.resolve(tool).ok_or_else(|| {
                Note::new(
                    NoteKind::MissingTool,
                    format!("{tool} is not installed yet (forge apply installs it)"),
                )
            })
        } else {
            self.opts.pinned.ensure(tool)
        };
        r.map_err(|n| match n.rule {
            Some(_) => n,
            None => self.note(n.kind, n.text),
        })
    }

    /// The project's `tool` for `op`.
    pub fn project(&self, tool: &str, op: &str) -> Result<Vec<String>, Note> {
        self.opts
            .project
            .command(self.root(), tool, op)
            .map_err(|n| match n.rule {
                Some(_) => n,
                None => self.note(n.kind, n.text),
            })
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
        if let Some(t) = &self.extra.tmp
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

/// A glob list relative to each workspace package.
pub fn in_dirs(files: &[String], dirs: Option<&[String]>) -> Vec<String> {
    let Some(dirs) = dirs else {
        return files.to_vec();
    };
    let mut include: Vec<String> = files
        .iter()
        .filter(|g| !g.starts_with('!'))
        .cloned()
        .collect();
    if include.is_empty() {
        include.push("**".into());
    }
    let mut out: Vec<String> = include
        .iter()
        .flat_map(|g| dirs.iter().map(move |d| format!("{d}/{g}")))
        .collect();
    for g in files.iter().filter_map(|g| g.strip_prefix('!')) {
        for d in dirs {
            out.push(format!("!{d}/{g}"));
        }
    }
    out
}

/// Include and exclude globs ("!" before an exclusion).
pub struct GlobList {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
}

impl GlobList {
    pub fn new(globs: &[String]) -> GlobList {
        let mut l = GlobList {
            include: vec![],
            exclude: vec![],
        };
        for g in globs {
            match g.strip_prefix('!') {
                Some(x) => l.exclude.push(glob_regex(x)),
                None => l.include.push(glob_regex(g)),
            }
        }
        l
    }

    /// Not excluded, and included (or there are no include globs).
    pub fn matches(&self, path: &str) -> bool {
        !self.exclude.iter().any(|r| r.is_match(path))
            && (self.include.is_empty() || self.include.iter().any(|r| r.is_match(path)))
    }
}

/// A glob with the generated scripts' `case` semantics: `*` also matches `/`, `**/` is zero
/// or more directories, `{a,b}` alternatives, `[…]` one character.
pub fn glob_regex(glob: &str) -> Regex {
    Regex::new(&format!("^(?:{})$", glob_source(glob)))
        .unwrap_or_else(|_| Regex::new("[^\\s\\S]").expect("regex"))
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
                } else {
                    out.push_str(".*");
                    i = j;
                }
                continue;
            }
            '?' => out.push('.'),
            '[' => match g.iter().skip(i + 2).position(|&x| x == ']') {
                Some(p) => {
                    let end = p + i + 2;
                    let mut body: String = g[i + 1..end].iter().collect();
                    let neg = body.starts_with('!') || body.starts_with('^');
                    if neg {
                        body.remove(0);
                    }
                    let body: String = body
                        .chars()
                        .map(|c| {
                            if matches!(c, '\\' | ']' | '^' | '[' | '&' | '~') {
                                format!("\\{c}")
                            } else {
                                c.to_string()
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
                let (mut depth, mut start, mut k, mut end) = (0, i + 1, i + 1, None);
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
    fn globs_have_case_semantics() {
        assert!(glob_regex("**/Dockerfile").is_match("Dockerfile"));
        assert!(glob_regex("**/Dockerfile").is_match("a/b/Dockerfile"));
        assert!(glob_regex("**/*.{yaml,yml}").is_match("a/x.yml"));
        assert!(!glob_regex("**/Dockerfile.*").is_match("Dockerfile"));
        let l = GlobList::new(&["**/Dockerfile.*".into(), "!**/*.dockerignore".into()]);
        assert!(l.matches("api/Dockerfile.prod"));
        assert!(!l.matches("api/Dockerfile.dockerignore"));
    }

    #[test]
    fn last_line_and_dirs() {
        assert_eq!(last_line("a\n\x1b[31mb\x1b[0m\n\n"), "b");
        assert_eq!(dir_of("Dockerfile"), ".");
        assert_eq!(dir_of("a/b/c"), "a/b");
        assert_eq!(join(".", "x"), "x");
    }
}
