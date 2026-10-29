//! Where Forge's pinned tools are. A builtin asks [`PinnedTools::pinned`]; a tool it cannot have
//! is a note for the person (the check then passes), never a finding.

use std::path::PathBuf;
use std::sync::Arc;

use forge_core::{Note, NoteKind};
use forge_tools::ToolResolver;

/// Finds one of Forge's pinned tools (gitleaks, osv-scanner, editorconfig-checker,
/// lockfile-lint).
pub trait PinnedTools: Send + Sync {
    /// The tool's executable; Err: why it cannot be had, for the person.
    fn pinned(&self, name: &str) -> Result<PathBuf, Note>;
}

/// The tools of a [`forge_tools::ToolResolver`] (Forge's cache): installed ones only, or
/// installing a missing one first when `install` is set (where a person waits anyway: `forge
/// check`, never a hook).
pub struct Provisioned<R> {
    pub resolver: R,
    pub install: bool,
}

impl<R: ToolResolver + Send + Sync> PinnedTools for Provisioned<R> {
    fn pinned(&self, name: &str) -> Result<PathBuf, Note> {
        if let Some(p) = self.resolver.resolve(name) {
            return Ok(p);
        }
        if self.install {
            return self.resolver.ensure(name);
        }
        Err(missing(name))
    }
}

/// The note for a pinned tool that is not installed.
pub fn missing(name: &str) -> Note {
    Note::new(
        NoteKind::MissingTool,
        format!("{name} is not installed (forge apply installs it), so this check is skipped"),
    )
}

/// Tools found in given directories, then on PATH (tests; machines with the tools installed).
#[derive(Debug, Clone, Default)]
pub struct OnPath {
    pub dirs: Vec<PathBuf>,
}

impl PinnedTools for OnPath {
    fn pinned(&self, name: &str) -> Result<PathBuf, Note> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        self.dirs
            .iter()
            .cloned()
            .chain(std::env::split_paths(&path))
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .ok_or_else(|| missing(name))
    }
}

/// Forge's cache from the environment, without downloads; when the cache cannot be found, the
/// tools on PATH.
pub fn default_tools() -> Arc<dyn PinnedTools> {
    match forge_home::Home::from_env() {
        Ok(home) => Arc::new(Provisioned {
            resolver: forge_tools::Provisioner::new(home),
            install: false,
        }),
        Err(_) => Arc::new(OnPath::default()),
    }
}
