//! Where a tool is: the one question the checker, the builtins, the types engine and the
//! person's report ask, whoever answers it.
//!
//! It lives here, in the crate every other one depends on, so that its two halves need not
//! know each other: forge-check answers for the project's own tools and PATH
//! (`forge_check::tools::LocalTools`), forge-tools for Forge's pinned tools from its cache
//! (`forge_tools::PinnedTools`), and the command line stacks them.
//!
//! Finding a tool never downloads. Installing a missing pinned tool ([`ToolResolver::ensure`])
//! may: only where the person waits anyway (`forge apply`, `forge check`, Stop), never after an
//! edit or before a command.

use std::path::{Path, PathBuf};

/// How to run a tool: the words before its own arguments, and the executable they resolve to
/// (whose changes invalidate what was learnt about it, such as its version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCmd {
    pub argv: Vec<String>,
    pub bin: PathBuf,
}

impl ToolCmd {
    pub fn exe(bin: PathBuf) -> ToolCmd {
        ToolCmd {
            argv: vec![bin.to_string_lossy().into_owned()],
            bin,
        }
    }
}

/// Answers "where is tool X".
pub trait ToolResolver: Send + Sync {
    /// The project's own `name` (node_modules/.bin, .venv, its Python environment), for a
    /// command run in `cwd` (a workspace package falls back to the root's install).
    fn project(&self, _root: &Path, _cwd: &Path, _name: &str) -> Option<ToolCmd> {
        None
    }

    /// The executable `name` on PATH.
    fn on_path(&self, _name: &str) -> Option<PathBuf> {
        None
    }

    /// Forge's pinned `name` at `version`, as already installed: never downloads (a few `stat`
    /// calls, safe on a hook's hot path). `Err` says why it cannot be had, for the person.
    fn pinned(&self, name: &str, version: &str) -> Result<ToolCmd, String>;

    /// The same, installing it first when it is missing and this resolver may download.
    fn ensure(&self, name: &str, version: &str) -> Result<ToolCmd, String> {
        self.pinned(name, version)
    }
}
