//! Where Forge keeps its files.
//!
//! The repository only holds what people review (`forge.yaml`, `forge.lock`,
//! `.forge/`, hook entries in `.claude/settings.json`, `.claude/rules/forge-*.md`).
//! Everything else lives in two per-user roots that this crate resolves:
//!
//! * the **cache root** (re-downloadable, safe to delete):
//!   `$FORGE_HOME`, else `$XDG_CACHE_HOME/forge`, else `~/.cache/forge`;
//! * the **state root** (per-repo, per-session runtime data):
//!   `$FORGE_HOME/state`, else `$XDG_STATE_HOME/forge`, else `~/.local/state/forge`.
//!
//! The same rules apply on macOS and Linux, so paths in docs and bug reports
//! are identical on both.
//!
//! Layout:
//!
//! ```text
//! <cache>/bin/forge-<version>          exact Forge binaries the launcher execs
//! <cache>/tools/<name>/<version>/      tools Forge drives (uv, gitleaks, ...)
//! <cache>/path/                        only uv/uvx: prepended to the agent's PATH
//! <cache>/packs/                       downloaded pack sources
//! <cache>/engine/<repo-id>.sock        per-repo engine socket
//! <state>/<repo-id>/sessions/<session-id>/
//! ```
//!
//! Every path component that comes from outside (versions, tool names,
//! session ids) is validated so it can never escape its directory.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// File that pins a repository's Forge version; marks the repo root.
pub const LOCK_FILE: &str = "forge.lock";
/// The repository's Forge configuration; marks the repo root when no lock exists yet.
pub const CONFIG_FILE: &str = "forge.yaml";

/// The environment values that decide the roots. Kept separate from
/// `std::env` so resolution is a pure, testable function.
#[derive(Debug, Clone, Default)]
pub struct HomeVars {
    pub forge_home: Option<OsString>,
    pub xdg_cache_home: Option<OsString>,
    pub xdg_state_home: Option<OsString>,
    pub home: Option<OsString>,
}

impl HomeVars {
    /// Reads `FORGE_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` and `HOME`.
    pub fn from_env() -> Self {
        HomeVars {
            forge_home: std::env::var_os("FORGE_HOME"),
            xdg_cache_home: std::env::var_os("XDG_CACHE_HOME"),
            xdg_state_home: std::env::var_os("XDG_STATE_HOME"),
            home: std::env::var_os("HOME"),
        }
    }
}

/// Why the roots could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeError {
    /// Neither `FORGE_HOME`, the XDG variables nor `HOME` give an absolute path.
    NoHomeDirectory,
}

impl fmt::Display for HomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HomeError::NoHomeDirectory => f.write_str(
                "cannot find a home directory: set HOME or FORGE_HOME to an absolute path",
            ),
        }
    }
}

impl std::error::Error for HomeError {}

/// A path component (version, tool name, session id) that is unsafe to put in a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidComponent {
    pub kind: &'static str,
    pub value: String,
}

impl fmt::Display for InvalidComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: {:?}", self.kind, self.value)
    }
}

impl std::error::Error for InvalidComponent {}

/// The resolved cache and state roots, and every path below them.
#[derive(Debug, Clone)]
pub struct Home {
    cache: PathBuf,
    state: PathBuf,
    /// Roots as stored plus their symlink-resolved forms, computed on first use.
    internal_roots: OnceLock<Vec<PathBuf>>,
}

fn absolute_var(v: &Option<OsString>) -> Option<PathBuf> {
    let v = v.as_ref()?;
    if v.is_empty() {
        return None;
    }
    let p = PathBuf::from(v);
    // The XDG spec says relative values are invalid and must be ignored;
    // we apply the same rule to FORGE_HOME and HOME so that a stray relative
    // value can never scatter files around whatever the cwd happens to be.
    p.is_absolute().then_some(p)
}

impl Home {
    /// Resolves the roots from the process environment.
    pub fn from_env() -> Result<Home, HomeError> {
        Home::from_vars(&HomeVars::from_env())
    }

    /// Resolves the roots from explicit values (pure; used by tests and tools).
    pub fn from_vars(vars: &HomeVars) -> Result<Home, HomeError> {
        let home = absolute_var(&vars.home);
        if let Some(fh) = absolute_var(&vars.forge_home) {
            let state = fh.join("state");
            return Ok(Home::new(fh, state));
        }
        let cache = match absolute_var(&vars.xdg_cache_home) {
            Some(x) => x.join("forge"),
            None => home
                .as_ref()
                .ok_or(HomeError::NoHomeDirectory)?
                .join(".cache/forge"),
        };
        let state = match absolute_var(&vars.xdg_state_home) {
            Some(x) => x.join("forge"),
            None => home
                .as_ref()
                .ok_or(HomeError::NoHomeDirectory)?
                .join(".local/state/forge"),
        };
        Ok(Home::new(cache, state))
    }

    /// Uses the given roots as they are.
    pub fn new(cache: PathBuf, state: PathBuf) -> Home {
        Home {
            cache: normalize_lexically(&cache),
            state: normalize_lexically(&state),
            internal_roots: OnceLock::new(),
        }
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache
    }

    pub fn state_root(&self) -> &Path {
        &self.state
    }

    /// `<cache>/bin`: exact Forge binaries, one file per version.
    pub fn bin_dir(&self) -> PathBuf {
        self.cache.join("bin")
    }

    /// `<cache>/bin/forge-<version>`.
    pub fn forge_binary(&self, version: &str) -> Result<PathBuf, InvalidComponent> {
        check_version(version)?;
        Ok(self.bin_dir().join(format!("forge-{version}")))
    }

    /// `<cache>/tools`.
    pub fn tools_dir(&self) -> PathBuf {
        self.cache.join("tools")
    }

    /// `<cache>/tools/<name>/<version>/`.
    pub fn tool_dir(&self, name: &str, version: &str) -> Result<PathBuf, InvalidComponent> {
        check_name("tool name", name)?;
        check_version(version)?;
        Ok(self.tools_dir().join(name).join(version))
    }

    /// `<cache>/path`: holds only uv/uvx; this directory goes on the agent's PATH.
    pub fn path_dir(&self) -> PathBuf {
        self.cache.join("path")
    }

    /// `<cache>/packs`.
    pub fn packs_dir(&self) -> PathBuf {
        self.cache.join("packs")
    }

    /// `<cache>/engine`.
    pub fn engine_dir(&self) -> PathBuf {
        self.cache.join("engine")
    }

    /// `<cache>/engine/<repo-id>.sock`.
    ///
    /// Unix socket paths are limited to about 104 bytes (macOS) / 108 (Linux);
    /// callers binding the socket should check the length and report it.
    pub fn engine_socket(&self, repo: &RepoId) -> PathBuf {
        self.engine_dir().join(format!("{}.sock", repo.as_str()))
    }

    /// `<state>/<repo-id>`.
    pub fn repo_state_dir(&self, repo: &RepoId) -> PathBuf {
        self.state.join(repo.as_str())
    }

    /// `<state>/<repo-id>/sessions`.
    pub fn sessions_dir(&self, repo: &RepoId) -> PathBuf {
        self.repo_state_dir(repo).join("sessions")
    }

    /// `<state>/<repo-id>/sessions/<session-id>`.
    pub fn session_dir(&self, repo: &RepoId, session: &SessionId) -> PathBuf {
        self.sessions_dir(repo).join(session.as_str())
    }

    /// Whether `p` is inside the cache or state root.
    ///
    /// Used to refuse agent reads of Forge internals. Relative paths are taken
    /// from the current directory. Both the path as written (`..` removed
    /// lexically) and its symlink-resolved form are checked, so a symlink in
    /// the repo that points into the cache is caught too. Hard links cannot be
    /// detected by path and are out of scope.
    pub fn is_internal_path(&self, p: &Path) -> bool {
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(p),
                Err(_) => return false,
            }
        };
        let roots = self.internal_roots.get_or_init(|| {
            let mut v = vec![self.cache.clone(), self.state.clone()];
            for r in [&self.cache, &self.state] {
                if let Ok(c) = r.canonicalize() {
                    if !v.contains(&c) {
                        v.push(c);
                    }
                }
            }
            v
        });
        let lexical = normalize_lexically(&abs);
        if roots.iter().any(|r| lexical.starts_with(r)) {
            return true;
        }
        let physical = resolve_existing_prefix(&abs);
        roots.iter().any(|r| physical.starts_with(r))
    }
}

/// Removes `.` and resolves `..` without touching the filesystem.
fn normalize_lexically(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(c);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Canonicalizes the longest existing ancestor of `p` and appends the rest.
fn resolve_existing_prefix(p: &Path) -> PathBuf {
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(c) = cur.canonicalize() {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return normalize_lexically(&out);
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                cur = parent;
            }
            _ => return normalize_lexically(p),
        }
    }
}

fn invalid(kind: &'static str, value: &str) -> InvalidComponent {
    InvalidComponent {
        kind,
        value: value.to_owned(),
    }
}

/// A single safe path component: `[A-Za-z0-9._+-]`, 1..=128 bytes, not
/// starting with `.` or `-` (so never `.`/`..`, hidden, or option-like).
fn check_component(kind: &'static str, s: &str, extra: &[u8]) -> Result<(), InvalidComponent> {
    let b = s.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 128
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_' || extra.contains(c));
    if ok { Ok(()) } else { Err(invalid(kind, s)) }
}

/// Versions as they appear in paths: semver characters only.
pub fn check_version(v: &str) -> Result<(), InvalidComponent> {
    check_component("version", v, b".+")
}

fn check_name(kind: &'static str, v: &str) -> Result<(), InvalidComponent> {
    check_component(kind, v, b".")
}

/// A Claude Code session id, validated before it is used in a path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Accepts `[A-Za-z0-9_-]`, 1..=128 bytes, starting with a letter or digit.
    /// Claude Code ids are UUIDs, which pass. No dots, so no `..`.
    pub fn parse(s: &str) -> Result<SessionId, InvalidComponent> {
        check_component("session id", s, b"")?;
        Ok(SessionId(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// First 16 hex digits of sha256 of the repo root's physical path.
///
/// Stable for a checkout as long as it does not move; two clones of the same
/// repo get different ids, which is what we want (separate engines/state).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoId(String);

impl RepoId {
    /// Hashes `root` as given. Pass a physical path (see [`RepoId::for_root`]).
    pub fn from_physical_path(root: &Path) -> RepoId {
        let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
        let mut s = String::with_capacity(16);
        for b in &digest[..8] {
            s.push_str(&format!("{b:02x}"));
        }
        RepoId(s)
    }

    /// Resolves symlinks in `root`, then hashes it.
    pub fn for_root(root: &Path) -> io::Result<RepoId> {
        Ok(RepoId::from_physical_path(&root.canonicalize()?))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which file marked the repo root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootMarker {
    Lock,
    Config,
}

/// A Forge repository root: a physical (symlink-free) path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRoot {
    path: PathBuf,
    marker: RootMarker,
}

impl RepoRoot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn marker(&self) -> RootMarker {
        self.marker
    }

    /// `<root>/forge.lock` (may not exist when the marker is `Config`).
    pub fn lock_path(&self) -> PathBuf {
        self.path.join(LOCK_FILE)
    }

    pub fn config_path(&self) -> PathBuf {
        self.path.join(CONFIG_FILE)
    }

    /// The root is already physical, so this does no I/O.
    pub fn id(&self) -> RepoId {
        RepoId::from_physical_path(&self.path)
    }
}

/// Finds the repo root from `CLAUDE_PROJECT_DIR` (set by Claude Code for hooks),
/// else the current directory.
pub fn find_repo_root_from_env() -> io::Result<Option<RepoRoot>> {
    let start = match std::env::var_os("CLAUDE_PROJECT_DIR").filter(|v| !v.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => std::env::current_dir()?,
    };
    find_repo_root(&start)
}

/// Walks up from `start` to the nearest directory containing `forge.lock`;
/// if there is none, the nearest containing `forge.yaml`. The walk stops at
/// the git root (a directory containing `.git`, file or dir) or the
/// filesystem root, so a stray `forge.lock` in `$HOME` never captures an
/// unrelated repo.
pub fn find_repo_root(start: &Path) -> io::Result<Option<RepoRoot>> {
    let start = start.canonicalize()?;
    let mut config: Option<PathBuf> = None;
    for dir in start.ancestors() {
        if dir.join(LOCK_FILE).is_file() {
            return Ok(Some(RepoRoot {
                path: dir.to_path_buf(),
                marker: RootMarker::Lock,
            }));
        }
        if config.is_none() && dir.join(CONFIG_FILE).is_file() {
            config = Some(dir.to_path_buf());
        }
        if dir.join(".git").exists() {
            break;
        }
    }
    Ok(config.map(|path| RepoRoot {
        path,
        marker: RootMarker::Config,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vars(fh: Option<&str>, xc: Option<&str>, xs: Option<&str>, h: Option<&str>) -> HomeVars {
        HomeVars {
            forge_home: fh.map(Into::into),
            xdg_cache_home: xc.map(Into::into),
            xdg_state_home: xs.map(Into::into),
            home: h.map(Into::into),
        }
    }

    #[test]
    fn roots_default_to_home() {
        let h = Home::from_vars(&vars(None, None, None, Some("/u"))).unwrap();
        assert_eq!(h.cache_root(), Path::new("/u/.cache/forge"));
        assert_eq!(h.state_root(), Path::new("/u/.local/state/forge"));
    }

    #[test]
    fn roots_follow_xdg() {
        let h = Home::from_vars(&vars(None, Some("/c"), Some("/s"), None)).unwrap();
        assert_eq!(h.cache_root(), Path::new("/c/forge"));
        assert_eq!(h.state_root(), Path::new("/s/forge"));
    }

    #[test]
    fn forge_home_wins_and_holds_state() {
        let h = Home::from_vars(&vars(Some("/f"), Some("/c"), Some("/s"), Some("/u"))).unwrap();
        assert_eq!(h.cache_root(), Path::new("/f"));
        assert_eq!(h.state_root(), Path::new("/f/state"));
    }

    #[test]
    fn empty_and_relative_values_are_ignored() {
        let h = Home::from_vars(&vars(Some(""), Some("rel"), Some(""), Some("/u"))).unwrap();
        assert_eq!(h.cache_root(), Path::new("/u/.cache/forge"));
        assert_eq!(h.state_root(), Path::new("/u/.local/state/forge"));
        assert_eq!(
            Home::from_vars(&vars(None, None, None, Some("rel"))).unwrap_err(),
            HomeError::NoHomeDirectory
        );
        // XDG for one root is enough only for that root.
        assert!(Home::from_vars(&vars(None, Some("/c"), None, None)).is_err());
    }

    #[test]
    fn layout() {
        let h = Home::new("/f".into(), "/f/state".into());
        let r = RepoId("0123456789abcdef".into());
        let s = SessionId::parse("9b1d-ab").unwrap();
        assert_eq!(
            h.forge_binary("0.7.0").unwrap(),
            Path::new("/f/bin/forge-0.7.0")
        );
        assert_eq!(
            h.tool_dir("uv", "0.5.1").unwrap(),
            Path::new("/f/tools/uv/0.5.1")
        );
        assert_eq!(h.path_dir(), Path::new("/f/path"));
        assert_eq!(h.packs_dir(), Path::new("/f/packs"));
        assert_eq!(
            h.engine_socket(&r),
            Path::new("/f/engine/0123456789abcdef.sock")
        );
        assert_eq!(
            h.session_dir(&r, &s),
            Path::new("/f/state/0123456789abcdef/sessions/9b1d-ab")
        );
    }

    #[test]
    fn components_cannot_escape() {
        for bad in [
            "", ".", "..", "../x", "a/b", "a\\b", ".hidden", "-rf", "a b", "a\0", "é",
        ] {
            assert!(SessionId::parse(bad).is_err(), "{bad:?}");
            assert!(check_version(bad).is_err(), "{bad:?}");
        }
        assert!(SessionId::parse(&"a".repeat(129)).is_err());
        assert!(SessionId::parse("1.2").is_err());
        assert!(SessionId::parse("550e8400-e29b-41d4-a716-446655440000").is_ok());
        assert!(check_version("1.2.3-rc.1+build.5").is_ok());
        let h = Home::new("/f".into(), "/f/state".into());
        assert!(h.tool_dir("..", "1").is_err());
        assert!(h.tool_dir("uv", "../../x").is_err());
    }

    #[test]
    fn repo_id_is_16_hex_of_sha256() {
        let a = RepoId::from_physical_path(Path::new("/repo"));
        assert_eq!(a.as_str().len(), 16);
        assert!(a.as_str().bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(a, RepoId::from_physical_path(Path::new("/repo")));
        assert_ne!(a, RepoId::from_physical_path(Path::new("/repo2")));
        let full = Sha256::digest(b"/repo");
        let hex: String = full[..8].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(a.as_str(), hex);
    }

    #[test]
    fn repo_id_resolves_symlinks() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = t.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            RepoId::for_root(&real).unwrap(),
            RepoId::for_root(&link).unwrap()
        );
    }

    #[test]
    fn finds_lock_then_config_and_stops_at_git() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        let repo = root.join("repo");
        let deep = repo.join("a/b");
        fs::create_dir_all(&deep).unwrap();
        fs::create_dir(repo.join(".git")).unwrap();

        // Nothing: a lock above the git root is not used.
        fs::write(root.join(LOCK_FILE), "").unwrap();
        assert_eq!(find_repo_root(&deep).unwrap(), None);

        fs::write(repo.join(CONFIG_FILE), "").unwrap();
        let r = find_repo_root(&deep).unwrap().unwrap();
        assert_eq!((r.path(), r.marker()), (repo.as_path(), RootMarker::Config));

        // A lock wins over a nearer config.
        fs::write(repo.join("a").join(CONFIG_FILE), "").unwrap();
        fs::write(repo.join(LOCK_FILE), "").unwrap();
        let r = find_repo_root(&deep).unwrap().unwrap();
        assert_eq!((r.path(), r.marker()), (repo.as_path(), RootMarker::Lock));
        assert_eq!(r.id(), RepoId::for_root(&repo).unwrap());
    }

    #[test]
    fn finds_root_through_symlinked_start() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().canonicalize().unwrap().join("repo");
        fs::create_dir_all(repo.join("sub")).unwrap();
        fs::write(repo.join(LOCK_FILE), "").unwrap();
        let link = t.path().join("l");
        std::os::unix::fs::symlink(repo.join("sub"), &link).unwrap();
        let r = find_repo_root(&link).unwrap().unwrap();
        assert_eq!(r.path(), repo.as_path());
    }

    #[test]
    fn internal_paths() {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let h = Home::new(base.join("cache"), base.join("state"));
        fs::create_dir_all(h.bin_dir()).unwrap();
        fs::create_dir_all(base.join("repo")).unwrap();

        assert!(h.is_internal_path(&h.bin_dir().join("forge-1.0.0")));
        assert!(h.is_internal_path(&base.join("state/x/y")));
        assert!(h.is_internal_path(&base.join("repo/../cache/bin")));
        assert!(!h.is_internal_path(&base.join("repo/file")));
        assert!(!h.is_internal_path(&base.join("cache-other")));
        assert!(!h.is_internal_path(&base.join("cache/../repo")));

        // A symlink in the repo pointing into the cache is internal.
        let link = base.join("repo/sneaky");
        std::os::unix::fs::symlink(h.bin_dir(), &link).unwrap();
        assert!(h.is_internal_path(&link.join("forge-1.0.0")));

        // Roots configured through a symlink are matched in both forms.
        let alias = base.join("alias");
        std::os::unix::fs::symlink(&base, &alias).unwrap();
        let h2 = Home::new(alias.join("cache"), alias.join("state"));
        assert!(h2.is_internal_path(&base.join("cache/bin")));
        assert!(h2.is_internal_path(&alias.join("cache/bin")));
    }
}
