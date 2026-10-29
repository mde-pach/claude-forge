//! What `forge apply` writes, and what it tells the person.
//!
//! - [`plan`]: the active packs, the rules that apply and what enforces each, the rules files
//!   ([`context`]) and the settings entries (permissions, sandbox, [`hooks`]);
//! - [`settings`]: in-place edits of Claude Code's settings files;
//! - [`lock`]: the lock file;
//! - [`changes`]: what apply would change, drift, and the migration from the old layout;
//! - [`precommit`]: the commit-time check;
//! - [`report`]: enforcement as the person reads it, and the [`report::ToolProbe`] question
//!   the checker answers.
//!
//! The rules files are the only text here the agent reads: they depend on the repository's
//! content alone, and name no Forge internals.

pub mod changes;
pub mod context;
pub mod hooks;
pub mod lock;
pub mod plan;
pub mod precommit;
pub mod report;
pub mod rules;
pub mod settings;

pub use changes::{Changes, Migration, Previous};
pub use plan::{Plan, PlanInput, plan};
