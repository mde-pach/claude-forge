//! Capabilities (§20): something the agent is able to use or should use, given to it only when
//! the task at hand needs it. A capability enforces nothing and grants nothing.
//!
//! Three gates, each with one owner: **active** (its pack is active and its `when` holds:
//! `forge_render::capabilities`), **available** (what it needs is on this machine: [`available`])
//! and **relevant** (the capability engine says so, per prompt).

pub mod available;
pub mod digest;
pub mod engine;
pub mod examples;
pub mod jev;
pub mod log;
pub mod machine;
pub mod render;
pub mod setup;

pub use available::{Availability, available};
pub use digest::{Digest, Places, digest};
pub use engine::{Candidate, Decision, Engine, EngineError, Scripted};

/// Creates `dir` and its parents, readable by the user only (mode 0700, §8.3).
pub(crate) fn private_dir(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .is_ok()
}

/// Opens `path` for writing, created with mode 0600: appended to, or truncated.
pub(crate) fn private_file(path: &std::path::Path, append: bool) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .mode(0o600)
        .open(path)
}
