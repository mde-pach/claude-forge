//! Claude Forge's checker: what replaces `.forge/check.sh`, `post-edit.sh` and the engine's
//! check machinery.
//!
//! - [`change`]: the change set against a base (turn snapshot, HEAD, merge-base), base copies,
//!   added lines;
//! - [`astgrep`] (ast-grep linked as a library) and [`grep`]: the pattern rules, counts against
//!   the base, forbid and advise on added lines;
//! - [`checker`]: the steps and `check` stages, attribution of tool diagnostics to the change,
//!   comparisons with the base, the edit-time run; [`checker::Checker::check_files`] is what the
//!   hooks call;
//! - [`adapters`]: the tool adapters as data (`adapters/<tool>.yaml`);
//! - [`builtins`]: what `forge internal <name>` dispatches (the `forge_*` functions of check
//!   commands), with a [`builtins::Registry`] other crates add domain builtins and helpers to;
//! - [`tools`]: where a tool is (forge-core's [`tools::ToolResolver`], answered by
//!   [`tools::LocalTools`] and, for Forge's pinned tools, forge-tools' cache);
//! - [`select`]: which rules apply: forge-detect's facts and placement, forge-render's plan;
//! - [`engine`]: the types engine (forge-engine's daemon) as the [`astgrep::TypeOracle`];
//! - [`probe`]: what the machine's tools can do, for the person (forge-render's
//!   [`forge_render::report::ToolProbe`]).
//!
//! Everything a run says is a [`forge_core::Finding`] (the agent's) or a [`forge_core::Note`]
//! (the person's); Forge's own failures are notes and never fail a check.

pub mod adapters;
pub mod astgrep;
pub mod base_tree;
pub mod builtins;
pub mod change;
pub mod checker;
pub mod diagnostics;
pub mod engine;
pub mod grep;
pub mod output;
pub mod probe;
pub mod scope;
pub mod select;
pub mod shell;
pub mod tools;

pub use change::{ChangeSet, Mode};
pub use checker::{ActiveRule, Checker, Options, Outcome, Phase, Status};

/// The Forge executable the builtins' shell functions call (`"$FORGE_EXE" internal <name>`).
pub const ENV_EXE: &str = "FORGE_EXE";
