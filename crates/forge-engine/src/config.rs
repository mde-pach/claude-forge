//! What the daemon runs with. Serializable, so the process that starts the daemon can hand it over
//! (an argument, a file, the environment): the caller decides.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn idle_default() -> u64 {
    1800
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    /// The repository root.
    pub root: PathBuf,
    /// The socket's path as the caller names it (see [`crate::socket_addr`] for the one bound).
    pub socket: PathBuf,
    /// Seconds without a request after which the daemon exits.
    #[serde(default = "idle_default")]
    pub idle_secs: u64,
    /// Where the daemon and its servers log (none: nowhere).
    #[serde(default)]
    pub log: Option<PathBuf>,
    /// What the daemon was started with (pins, Forge's version, the untyped list's hash…): a client
    /// whose `want` differs stops it and starts another.
    #[serde(default)]
    pub want: String,
    #[serde(default)]
    pub python: Option<PythonConfig>,
    #[serde(default)]
    pub typescript: Option<TypeScriptConfig>,
}

impl EngineConfig {
    pub fn new(root: impl Into<PathBuf>, socket: impl Into<PathBuf>) -> Self {
        EngineConfig {
            root: root.into(),
            socket: socket.into(),
            idle_secs: idle_default(),
            log: None,
            want: String::new(),
            python: None,
            typescript: None,
        }
    }
}

/// Python's engine: the pinned ty.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PythonConfig {
    /// The command that runs ty: its path, or a launcher and its arguments
    /// (`["uvx", "-q", "--from", "ty==0.0.86", "ty"]`). `server` is appended.
    pub ty: Vec<String>,
    /// The project's interpreter and its `sys.prefix`, when ty would not find its environment
    /// itself (given to `ty server` as the active environment).
    #[serde(default)]
    pub python: Option<PathBuf>,
    #[serde(default)]
    pub prefix: Option<PathBuf>,
    /// The installed modules without typing the questions' server sees as Any (the list less the
    /// packages the active rules ask about, spec §15.2).
    #[serde(default)]
    pub untyped: Vec<String>,
    /// The whole list, as `ty check` gets it, for the diagnostics; `None`: the same as `untyped`.
    #[serde(default)]
    pub untyped_all: Option<Vec<String>>,
    /// The ty rules set to error in the servers' inline configuration: the active rules' levels,
    /// where the project does not configure ty itself (§11.2). Empty: ty's own levels.
    #[serde(default)]
    pub rules: Vec<String>,
}

/// TypeScript's engine: the project's own TypeScript (`node_modules/typescript`, resolved from the
/// root): 7 and later through its language server, 5 and 6 through tsserver.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TypeScriptConfig {
    /// A JavaScript runtime (node, bun) for tsserver (TypeScript 5, 6) and for TypeScript 7's
    /// launcher when its native compiler is not found.
    #[serde(default)]
    pub js_runtime: Option<PathBuf>,
    /// Load the project's tsconfig projects in the background once started (questions first).
    #[serde(default = "yes")]
    pub preload: bool,
}

fn yes() -> bool {
    true
}
