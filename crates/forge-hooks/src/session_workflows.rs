//! The workflows a session runs ([`crate::workflow`]): on the session's first prompt, each
//! built-in workflow whose trigger is `first-prompt`, with the prompt as its `request`, its steps
//! run by Forge's step agent ([`crate::agents`]). Their results are the agent's context, again
//! after a compaction, and what the reviewer knows of the request. What a workflow does (its
//! steps, what it asks, what it hands over) is in its file; here is only when it runs and where
//! its result goes. Everything lives in the session's state; nothing is written to the repository.

use std::path::PathBuf;

use serde_json::Value;

use crate::input::HookInput;
use crate::{Project, Runtime, agents};

fn runs_dir(p: &Project) -> PathBuf {
    p.session_dir.join("workflows")
}

/// UserPromptSubmit: on the session's first prompt, the first-prompt workflows start; the agent
/// is told to run the first step (or gets the results of workflows that needed none).
pub fn first_prompt(rt: &Runtime, input: &HookInput, p: &Project) -> Option<String> {
    agents::first_prompt(rt, input, p)
}

/// The results the session's workflows handed over, in name order; None when there are none.
pub fn results(p: &Project) -> Option<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(runs_dir(p))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|f| f.extension().is_some_and(|x| x == "json"));
    files.sort();
    let out: Vec<String> = files
        .iter()
        .filter_map(|file| {
            let text = std::fs::read(file).ok()?;
            let run: Value = serde_json::from_slice(&text).ok()?;
            run["error"]
                .is_null()
                .then(|| run["result"].as_str().map(str::to_string))
                .flatten()
                .filter(|r| !r.is_empty())
        })
        .collect();
    (!out.is_empty()).then(|| out.join("\n\n"))
}

/// SessionStart after a compaction: the results again, so the agent keeps working from them.
pub fn after_compaction(input: &HookInput, p: &Project) -> Option<String> {
    if input.source.as_deref() != Some("compact") {
        return None;
    }
    results(p)
}
