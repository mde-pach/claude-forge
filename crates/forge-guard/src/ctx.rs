//! What the guard knows about the project, filled by the hook runtime.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How the project manages its Python packages (the `python.pm` fact).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PythonPm {
    Uv,
    Poetry,
    Pdm,
    Pipenv,
    /// A `pyproject.toml` declaring the project, no manager's mark (no lock, no uv settings).
    Pyproject,
    /// `requirements*.txt`, no packaging metadata.
    Pip,
    /// Python files and no packaging file at all.
    Scripts,
}

impl PythonPm {
    /// The fact's value as forge-detect writes it.
    pub fn from_fact(s: &str) -> Option<Self> {
        Some(match s {
            "uv" => PythonPm::Uv,
            "poetry" => PythonPm::Poetry,
            "pdm" => PythonPm::Pdm,
            "pipenv" => PythonPm::Pipenv,
            "pyproject" => PythonPm::Pyproject,
            "pip" => PythonPm::Pip,
            "scripts" | "none" => PythonPm::Scripts,
            _ => return None,
        })
    }
}

/// The project's JavaScript package manager (the `js.pm` fact).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JsPm {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

/// File lookups the checks make (is `tsc` in `node_modules/.bin`?); the hook runtime may
/// answer them from a cache.
pub trait Lookup {
    fn exists(&self, path: &Path) -> bool;
    fn is_executable(&self, path: &Path) -> bool;
}

/// Lookups on the file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsLookup;

impl Lookup for FsLookup {
    fn exists(&self, path: &Path) -> bool {
        path.symlink_metadata().is_ok()
    }

    fn is_executable(&self, path: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            path.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        {
            path.is_file()
        }
    }
}

/// The project and the directory a command runs from.
pub struct GuardCtx<'a> {
    /// The repository root.
    pub root: PathBuf,
    /// Where the command starts (the hook input's `cwd`).
    pub cwd: PathBuf,
    /// `~` and `$HOME`; None: a `cd ~` cannot be followed.
    pub home: Option<PathBuf>,
    pub python_pm: Option<PythonPm>,
    pub js_pm: Option<JsPm>,
    /// The turn snapshot (§10.2), when the turn has one: the paths uncommitted at its start are
    /// read from it when a check needs them ([`pre_turn_changes`]). None: the turn is not known.
    pub turn: Option<PathBuf>,
    pub lookup: &'a dyn Lookup,
}

/// The tracked paths that had uncommitted changes at the start of the turn (modified, staged or
/// deleted; untracked files left out), from the turn snapshot's `dirty` less its `untracked`.
/// None when the snapshot cannot be read. A snapshot without `untracked` (written before it
/// existed) counts every dirty path.
pub fn pre_turn_changes(snapshot: &Path) -> Option<Vec<String>> {
    let dirty = std::fs::read_to_string(snapshot.join("dirty")).ok()?;
    let untracked = std::fs::read_to_string(snapshot.join("untracked")).unwrap_or_default();
    let untracked: std::collections::BTreeSet<&str> = untracked.lines().collect();
    Some(
        dirty
            .lines()
            .filter_map(|l| l.split_once('\t').map(|(p, _)| p))
            .filter(|p| !p.is_empty() && !untracked.contains(p))
            .map(str::to_owned)
            .collect(),
    )
}

impl GuardCtx<'static> {
    /// A context with no facts, looking files up on disk, `home` from `$HOME`.
    pub fn new(root: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        GuardCtx {
            root: root.into(),
            cwd: cwd.into(),
            home: std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(PathBuf::from),
            python_pm: None,
            js_pm: None,
            turn: None,
            lookup: &FsLookup,
        }
    }
}

impl<'a> GuardCtx<'a> {
    pub fn with_python_pm(mut self, pm: Option<PythonPm>) -> Self {
        self.python_pm = pm;
        self
    }

    pub fn with_js_pm(mut self, pm: Option<JsPm>) -> Self {
        self.js_pm = pm;
        self
    }

    pub fn with_turn(mut self, snapshot: Option<PathBuf>) -> Self {
        self.turn = snapshot;
        self
    }

    pub fn with_home(mut self, home: Option<PathBuf>) -> Self {
        self.home = home;
        self
    }

    pub fn with_lookup<'b>(self, lookup: &'b dyn Lookup) -> GuardCtx<'b> {
        GuardCtx {
            root: self.root,
            cwd: self.cwd,
            home: self.home,
            python_pm: self.python_pm,
            js_pm: self.js_pm,
            turn: self.turn,
            lookup,
        }
    }

    /// Whether `p` is the root or below it.
    pub fn in_repo(&self, p: &Path) -> bool {
        p.starts_with(&self.root)
    }

    /// `p` relative to the root, for messages (`.` for the root itself).
    pub fn shown(&self, p: &Path) -> String {
        match p.strip_prefix(&self.root) {
            Ok(r) if r.as_os_str().is_empty() => "the repository root".into(),
            Ok(r) => r.display().to_string(),
            Err(_) => p.display().to_string(),
        }
    }
}
