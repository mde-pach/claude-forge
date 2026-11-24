//! The capability engine (§20.3), gate 3: per prompt, which of the candidates the task needs.
//! A small trait, so the tests run on a scripted engine; the one real implementation is Jev
//! ([`crate::jev`]).

use std::collections::BTreeMap;

use crate::digest::Digest;

/// One capability the engine decides on: its statements and its threshold.
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    pub id: &'a str,
    pub statements: &'a [String],
    pub threshold: f64,
}

/// The question key of a candidate's statement `n` (from 0): `<capability id>#<n>`.
pub fn key(id: &str, n: usize) -> String {
    format!("{id}#{n}")
}

/// The engine's decision on one candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub id: String,
    /// One per statement, in order; `None` when the answer left it out.
    pub values: Vec<Option<f64>>,
    pub selected: bool,
}

/// Why the engine gave no answer. None of these reaches the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// No key or no consent on this machine: capabilities are off here.
    Off,
    /// No answer in time, no network, rate limited (429) or overloaded (529).
    Environment(String),
    /// The key was refused (401).
    Unauthorized,
    /// Anything else (a refused request, an answer that does not read).
    Failure(String),
}

/// Which of `candidates` the task, as `digest` tells it, needs.
pub trait Engine {
    /// The value of each question (key → statement), from 0 to 1; a question the answer left out
    /// has none.
    fn values(
        &self,
        digest: &Digest,
        questions: &[(String, &str)],
    ) -> Result<BTreeMap<String, f64>, EngineError>;

    /// The decision on each candidate: selected when any of its statements gets a value of at
    /// least its threshold. One request for all of them.
    fn select(
        &self,
        digest: &Digest,
        candidates: &[Candidate],
    ) -> Result<Vec<Decision>, EngineError> {
        let questions: Vec<(String, &str)> = candidates
            .iter()
            .flat_map(|c| {
                c.statements
                    .iter()
                    .enumerate()
                    .map(|(n, s)| (key(c.id, n), s.as_str()))
            })
            .collect();
        let values = self.values(digest, &questions)?;
        Ok(candidates
            .iter()
            .map(|c| {
                let v: Vec<Option<f64>> = (0..c.statements.len())
                    .map(|n| values.get(&key(c.id, n)).copied())
                    .collect();
                Decision {
                    id: c.id.to_string(),
                    selected: v.iter().flatten().any(|x| *x >= c.threshold),
                    values: v,
                }
            })
            .collect())
    }
}

/// An engine that answers from a table, for the tests: each question's value by its statement's
/// text, or an error.
#[derive(Debug, Clone, Default)]
pub struct Scripted {
    pub values: BTreeMap<String, f64>,
    pub error: Option<EngineError>,
    /// The questions it was asked, per request.
    pub asked: std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    /// The digest of each request.
    pub digests: std::sync::Arc<std::sync::Mutex<Vec<Digest>>>,
}

impl Scripted {
    /// An engine answering `value` for each statement of `values`.
    pub fn answering(values: &[(&str, f64)]) -> Scripted {
        Scripted {
            values: values.iter().map(|(s, v)| ((*s).to_string(), *v)).collect(),
            ..Scripted::default()
        }
    }

    /// An engine that fails with `error`.
    pub fn failing(error: EngineError) -> Scripted {
        Scripted {
            error: Some(error),
            ..Scripted::default()
        }
    }

    /// The questions of each request so far.
    pub fn requests(&self) -> Vec<Vec<String>> {
        self.asked.lock().map(|a| a.clone()).unwrap_or_default()
    }
}

impl Engine for Scripted {
    fn values(
        &self,
        digest: &Digest,
        questions: &[(String, &str)],
    ) -> Result<BTreeMap<String, f64>, EngineError> {
        if let Ok(mut a) = self.asked.lock() {
            a.push(questions.iter().map(|(k, _)| k.clone()).collect());
        }
        if let Ok(mut d) = self.digests.lock() {
            d.push(digest.clone());
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        Ok(questions
            .iter()
            .filter_map(|(k, s)| self.values.get(*s).map(|v| (k.clone(), *v)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Any one statement at or above the threshold selects; a statement the answer left out
    // selects nothing.
    #[test]
    fn a_statement_at_its_threshold_selects() {
        let e = Scripted {
            values: BTreeMap::from([("A page.".into(), 0.3), ("A table.".into(), 0.9)]),
            ..Scripted::default()
        };
        let d = Digest {
            request: "x".into(),
            previous_answer: None,
            recent_actions: Vec::new(),
        };
        let web = ["Missing.".to_string(), "A page.".to_string()];
        let db = ["A table.".to_string()];
        let decisions = e
            .select(
                &d,
                &[
                    Candidate {
                        id: "a/web",
                        statements: &web,
                        threshold: 0.3,
                    },
                    Candidate {
                        id: "a/db",
                        statements: &db,
                        threshold: 0.95,
                    },
                ],
            )
            .unwrap();
        assert_eq!(decisions[0].values, [None, Some(0.3)]);
        assert!(decisions[0].selected);
        assert!(!decisions[1].selected);
        assert_eq!(e.asked.lock().unwrap()[0], ["a/web#0", "a/web#1", "a/db#0"]);
    }
}
