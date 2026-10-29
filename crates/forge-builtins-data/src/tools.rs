//! The two ways the builtins reach tools: the project's own (through the checker's adapters)
//! and Forge's pinned ones (through forge-tools' resolver).

use std::path::Path;

/// The Squawk release Forge pins (forge-tools' catalog).
pub const SQUAWK_VERSION: &str = "2.70.0";

/// What a tool printed and how it exited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl ToolOutput {
    /// stdout then stderr.
    pub fn text(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// The outcome of asking for one operation of a project tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRun {
    /// It ran (whatever its exit status).
    Ran(ToolOutput),
    /// The project does not have the tool.
    Missing,
    /// The installed version cannot do the operation; the text says why (for the person).
    Unsupported(String),
}

/// The project's own tools, run as the checker's adapters run them (`forge_do <tool> <op>
/// <args>`): the operation's arguments for the installed version, then `args`, in `root`.
///
/// Operations the builtins ask for: `alembic upgrade-sql <range>` (`upgrade <range> --sql`),
/// `sqlfluff lint <files>` (`lint --format github-annotation-native --nocolor`), and for
/// `typeorm`, `typeorm-ts-node-esm` and `typeorm-ts-node-commonjs`: `migrate <data source>`
/// (`migration:run -d`) and `migrations-check <data source> <path>` (`migration:generate -d
/// … --check`).
pub trait ProjectTools: Send + Sync {
    fn run(&self, root: &Path, tool: &str, op: &str, args: &[String]) -> ToolRun;
}

/// No project tool is available.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProjectTools;

impl ProjectTools for NoProjectTools {
    fn run(&self, _: &Path, _: &str, _: &str, _: &[String]) -> ToolRun {
        ToolRun::Missing
    }
}

/// Forge's pinned tools: the words that run one (its executable, or a runner and its
/// arguments). `Err` says why it cannot be had, for the person.
pub trait PinnedTools: Send + Sync {
    fn command(&self, name: &str) -> Result<Vec<String>, String>;
}

/// No pinned tool is installed.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPinnedTools;

impl PinnedTools for NoPinnedTools {
    fn command(&self, name: &str) -> Result<Vec<String>, String> {
        Err(format!("{name} is not installed (forge apply installs it)"))
    }
}

/// The pinned tools a forge-tools resolver has installed (never downloads: `resolve`).
pub struct ResolverPinned<R>(pub R);

impl<R: forge_tools::ToolResolver + Send + Sync> PinnedTools for ResolverPinned<R> {
    fn command(&self, name: &str) -> Result<Vec<String>, String> {
        self.0
            .resolve(name)
            .map(|p| vec![p.to_string_lossy().into_owned()])
            .ok_or_else(|| format!("{name} is not installed (forge apply installs it)"))
    }
}
