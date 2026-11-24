//! A capability's examples (§20.8): each `selected` prompt must select it and each
//! `not_selected` one must not, asked of the engine as a first prompt (the request alone). They
//! need the real engine: `forge test` runs them on this machine's, and Forge's own suite only
//! when opted in (`FORGE_TEST_JEV=1`).

use forge_core::examples::{ExampleKind, ExampleResult, ExampleVerdict};
use forge_core::{Note, NoteKind};
use forge_packs::model::Capability;

use crate::digest::Digest;
use crate::engine::{Candidate, Engine, EngineError};

/// The target kind its results are labelled with.
pub const TARGET: &str = "capability";

/// The results of `cap`'s examples, one request each.
pub fn run(cap: &Capability, engine: &dyn Engine) -> Vec<ExampleResult> {
    let Some(ex) = &cap.examples else {
        return Vec::new();
    };
    let lists = [
        (ExampleKind::Match, &ex.selected, true),
        (ExampleKind::NoMatch, &ex.not_selected, false),
    ];
    let mut out = Vec::new();
    for (kind, list, want) in lists {
        for (i, prompt) in list.iter().enumerate() {
            let digest = Digest {
                request: prompt.chars().take(crate::digest::REQUEST_MAX).collect(),
                previous_answer: None,
                recent_actions: Vec::new(),
            };
            let cand = Candidate {
                id: &cap.id,
                statements: &cap.relevant,
                threshold: cap.threshold(),
            };
            let verdict = match engine.select(&digest, &[cand]) {
                Ok(d) => {
                    let got = d.first().is_some_and(|d| d.selected);
                    let values: Vec<String> = d
                        .first()
                        .map(|d| {
                            d.values
                                .iter()
                                .map(|v| v.map_or("-".into(), |v| format!("{v:.2}")))
                                .collect()
                        })
                        .unwrap_or_default();
                    if got == want {
                        ExampleVerdict::Pass
                    } else {
                        ExampleVerdict::Fail {
                            why: format!(
                                "{} for {prompt:?} (values {}, threshold {})",
                                if got { "selected" } else { "not selected" },
                                values.join(", "),
                                cap.threshold()
                            ),
                        }
                    }
                }
                Err(e) => ExampleVerdict::Skip {
                    note: Note::new(
                        NoteKind::Environment,
                        match e {
                            EngineError::Off => "the capability engine is off on this machine (forge apply --user sets it up)".to_string(),
                            EngineError::Unauthorized => "the capability engine refused its key (401)".to_string(),
                            EngineError::Environment(w) | EngineError::Failure(w) => w,
                        },
                    ),
                },
            };
            out.push(ExampleResult::new(&cap.id, TARGET, kind, i + 1, verdict));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Scripted;

    fn cap() -> Capability {
        serde_json::from_value(serde_json::json!({
            "id": "a/web", "description": "d", "relevant": ["The task involves a page."],
            "deliver": "inject", "text": "t",
            "examples": {"selected": ["Fix the page"], "not_selected": ["Bump it"]},
        }))
        .unwrap()
    }

    // An example passes when the engine agrees with its list, fails saying the values when it does
    // not, and is skipped (with a note) when the engine cannot answer.
    #[test]
    fn examples_by_the_engine() {
        let all = Scripted::answering(&[("The task involves a page.", 0.9)]);
        let r = run(&cap(), &all);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].verdict, ExampleVerdict::Pass);
        assert!(
            matches!(&r[1].verdict, ExampleVerdict::Fail { why } if why.contains("selected for \"Bump it\" (values 0.90, threshold 0.3)"))
        );
        assert_eq!(r[1].label(), "a/web capability no_match example 1");
        let off = Scripted::failing(EngineError::Off);
        assert!(
            run(&cap(), &off)
                .iter()
                .all(|r| matches!(r.verdict, ExampleVerdict::Skip { .. }))
        );
    }
}
