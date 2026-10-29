//! What the hooks ask the checker (forge-check): the edit-time run, the Stop run, and the
//! test-edit rules. The CLI wires the real checker in; [`NoChecker`] answers nothing.

use std::path::Path;

use forge_core::Report;

/// How a Stop run went (forge-check's `Status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopStatus {
    Pass,
    Fail,
    /// The only failures are tests that also fail at the start of the turn.
    Unverified,
    /// Advice was the only finding.
    AdviceOnly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StopOutcome {
    pub report: Report,
    pub status: StopStatus,
}

impl StopOutcome {
    pub fn pass() -> StopOutcome {
        StopOutcome {
            report: Report::default(),
            status: StopStatus::Pass,
        }
    }
}

/// A test-edit rule (`forge_test_edits`) whose scope holds a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestEditRule {
    pub id: String,
    /// The question a reviewer answers when nobody can.
    pub review: Option<String>,
}

/// A test (or test support code) of a file: its key and its fingerprint (forge-check's
/// `builtins::tests::Block`). Support code is keyed `support:<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestBlock {
    pub key: String,
    pub print: String,
}

/// The checker, as the hooks use it. Every method is called on the repository root.
/// A failure of the checker itself must come back as notes, never as failing findings.
pub trait Checker {
    /// After an edit (PostToolUse): `files` (repository-relative) changed, against the turn
    /// snapshot when there is one (else HEAD). Forge's `check_files(files, snapshot, PostEdit)`.
    fn post_edit(&self, root: &Path, files: &[String], snapshot: Option<&Path>) -> Report;

    /// At Stop: what changed during the turn (`snapshot`), or every uncommitted change when there
    /// is no snapshot. Forge's `check(Turn(snapshot) | Changed, Stop)`.
    fn stop(&self, root: &Path, snapshot: Option<&Path>) -> StopOutcome;

    /// The test-edit rule whose scope holds `path`, if any.
    fn test_edit_rule(&self, root: &Path, path: &str) -> Option<TestEditRule>;

    /// The tests of a file's text.
    fn test_blocks(&self, path: &str, source: &str) -> Vec<TestBlock>;
}

/// No checks: the hooks that need the checker stay silent.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoChecker;

impl Checker for NoChecker {
    fn post_edit(&self, _: &Path, _: &[String], _: Option<&Path>) -> Report {
        Report::default()
    }

    fn stop(&self, _: &Path, _: Option<&Path>) -> StopOutcome {
        StopOutcome::pass()
    }

    fn test_edit_rule(&self, _: &Path, _: &str) -> Option<TestEditRule> {
        None
    }

    fn test_blocks(&self, _: &str, _: &str) -> Vec<TestBlock> {
        Vec::new()
    }
}
