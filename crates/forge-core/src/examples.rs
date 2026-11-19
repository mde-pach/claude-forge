//! What running a rule's examples gives (`forge test`, and the built-in packs' example tests):
//! one result per example and target. A skipped example carries the note that says why, so a
//! skip is told apart by its structure, never by reading text.

use serde::Serialize;

use crate::{Note, NoteKind};

/// Which list of a rule's `examples` an example is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExampleKind {
    /// It must be caught (the check fails, the pattern matches, the guard decides).
    Match,
    /// It must pass.
    NoMatch,
}

impl ExampleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExampleKind::Match => "match",
            ExampleKind::NoMatch => "no_match",
        }
    }
}

impl std::fmt::Display for ExampleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How one example went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum ExampleVerdict {
    /// It behaved as its list says.
    Pass,
    /// It did not: why, for the rule's author.
    Fail { why: String },
    /// It could not run here (a tool, a builtin, the types engine): the note says why.
    Skip { note: Note },
}

impl ExampleVerdict {
    /// The verdict of a run that did not catch what a `match` example holds, or caught a
    /// `no_match` one, given the run's notes about the rule: a note of a kind that means the
    /// rule did not run (a missing tool, not enforced here, Forge's own failure, the
    /// environment) is a skip; otherwise it fails with `why`.
    pub fn missed(notes: &[&Note], why: impl Into<String>) -> ExampleVerdict {
        match notes.iter().find(|n| skips(n.kind)) {
            Some(n) => ExampleVerdict::Skip { note: (*n).clone() },
            None => ExampleVerdict::Fail { why: why.into() },
        }
    }
}

/// Whether a note of this kind says a rule did not run.
pub fn skips(kind: NoteKind) -> bool {
    matches!(
        kind,
        NoteKind::MissingTool
            | NoteKind::NotEnforced
            | NoteKind::ForgeFailure
            | NoteKind::Environment
    )
}

/// One example of one rule, through one of its targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExampleResult {
    pub rule: String,
    /// The target kind that ran it (`guard`, `ast-grep`, `grep`, `check`).
    pub target: &'static str,
    pub kind: ExampleKind,
    /// Its place in its list, from 1.
    pub index: usize,
    #[serde(flatten)]
    pub verdict: ExampleVerdict,
    /// What the agent would read from the run (findings), so callers can hold it to §1.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agent_texts: Vec<String>,
}

impl ExampleResult {
    pub fn new(
        rule: &str,
        target: &'static str,
        kind: ExampleKind,
        index: usize,
        verdict: ExampleVerdict,
    ) -> ExampleResult {
        ExampleResult {
            rule: rule.to_string(),
            target,
            kind,
            index,
            verdict,
            agent_texts: Vec::new(),
        }
    }

    /// `<rule> <target> <kind> example <n>`.
    pub fn label(&self) -> String {
        format!(
            "{} {} {} example {}",
            self.rule, self.target, self.kind, self.index
        )
    }
}
