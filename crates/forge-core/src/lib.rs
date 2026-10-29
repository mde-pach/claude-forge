//! What every Forge crate shares.
//!
//! Every output has one reader, decided by its type:
//! - a [`Finding`] is for the coding agent: a problem in the change it is making, which it can fix.
//!   Hook output, Stop block reasons and deny/ask reasons are made of findings only.
//! - a [`Note`] is for the person: a missing tool, what is not enforced, a failure of Forge itself,
//!   environment state. Notes go to `forge apply`, `forge status` and `forge check`, never to the agent.
//!
//! A failure of Forge itself is always a note: it never fails a check and never blocks the agent.

pub mod builtin;
pub mod internals;
pub mod tools;

use std::fmt;

use serde::{Deserialize, Serialize};

/// How a finding counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Must be fixed: fails the check, blocks Stop.
    Error,
    /// Reported once for the agent to weigh; never fails anything.
    Advice,
}

/// A problem in the agent's change, which the agent can act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// The rule that found it. Shown to the person, never in agent text.
    pub rule: String,
    pub severity: Severity,
    /// Repository-relative path, when the finding is about a file.
    pub path: Option<String>,
    /// 1-based line, when known.
    pub line: Option<u32>,
    /// One or more lines, written for the agent: what is wrong and what to do.
    pub message: String,
}

impl Finding {
    pub fn error(rule: impl Into<String>, message: impl Into<String>) -> Self {
        Finding {
            rule: rule.into(),
            severity: Severity::Error,
            path: None,
            line: None,
            message: message.into(),
        }
    }

    pub fn advice(rule: impl Into<String>, message: impl Into<String>) -> Self {
        Finding {
            severity: Severity::Advice,
            ..Finding::error(rule, message)
        }
    }

    pub fn at(mut self, path: impl Into<String>, line: Option<u32>) -> Self {
        self.path = Some(path.into());
        self.line = line;
        self
    }

    /// The text the agent reads: `path:line message`, `Advice: ` before advice. No rule id.
    pub fn agent_text(&self) -> String {
        let mut s = String::new();
        if let Some(p) = &self.path {
            s.push_str(p);
            if let Some(l) = self.line {
                s.push_str(&format!(":{l}"));
            }
            s.push(' ');
        }
        if self.severity == Severity::Advice {
            s.push_str("Advice: ");
        }
        s.push_str(&self.message);
        s
    }

    /// The text a person reads: the agent text after the rule id.
    pub fn person_text(&self) -> String {
        format!("{} {}", self.rule, self.agent_text())
    }
}

/// What a note is about, for grouping in `forge status` and for replacing an older note of the same kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoteKind {
    /// A tool a rule needs is not installed, so the rule is skipped.
    MissingTool,
    /// The installed tool cannot do what a rule asks (its version), so it is not enforced.
    NotEnforced,
    /// Forge itself failed (an engine did not start, a helper broke, a budget ran out).
    ForgeFailure,
    /// The environment (no network, no local database, a large working tree…).
    Environment,
    /// Forge's own files or versions (integrity, version pins).
    Setup,
}

/// Information for the person. Never shown to the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub kind: NoteKind,
    /// The rule it concerns, when there is one.
    pub rule: Option<String>,
    pub text: String,
}

impl Note {
    pub fn new(kind: NoteKind, text: impl Into<String>) -> Self {
        Note {
            kind,
            rule: None,
            text: text.into(),
        }
    }

    pub fn for_rule(mut self, rule: impl Into<String>) -> Self {
        self.rule = Some(rule.into());
        self
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.rule {
            Some(r) => write!(f, "{r}: {}", self.text),
            None => f.write_str(&self.text),
        }
    }
}

/// What a check, a hook step or a builtin produced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub notes: Vec<Note>,
}

impl Report {
    pub fn extend(&mut self, other: Report) {
        self.findings.extend(other.findings);
        self.notes.extend(other.notes);
    }

    /// Whether anything must be fixed (advice and notes never fail).
    pub fn fails(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// The agent's text: errors first, then advice; capped at `max_lines` lines, with the count of
    /// what was left out. Notes are never part of it.
    pub fn agent_text(&self, max_lines: usize) -> String {
        let mut ordered: Vec<&Finding> = self
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .collect();
        ordered.extend(
            self.findings
                .iter()
                .filter(|f| f.severity == Severity::Advice),
        );
        let mut lines = Vec::new();
        let mut left = 0;
        for f in ordered {
            for l in f.agent_text().lines() {
                if lines.len() < max_lines {
                    lines.push(l.to_owned());
                } else {
                    left += 1;
                }
            }
        }
        if left > 0 {
            lines.push(format!("({left} more lines)"));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_text_has_no_rule_id_and_no_notes() {
        let mut r = Report::default();
        r.findings
            .push(Finding::advice("python/x", "Prefer y.").at("a.py", Some(3)));
        r.findings
            .push(Finding::error("python/no-eval", "No eval().").at("b.py", Some(1)));
        r.notes
            .push(Note::new(NoteKind::MissingTool, "ruff is not installed"));
        assert_eq!(
            r.agent_text(10),
            "b.py:1 No eval().\na.py:3 Advice: Prefer y."
        );
        assert!(r.fails());
    }

    #[test]
    fn agent_text_is_capped() {
        let mut r = Report::default();
        for i in 0..5 {
            r.findings.push(Finding::error("r", format!("m{i}")));
        }
        assert_eq!(r.agent_text(2), "m0\nm1\n(3 more lines)");
    }
}
