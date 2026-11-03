//! The workflows a session runs ([`crate::workflow`]): on the session's first prompt, each
//! built-in workflow whose trigger is `first-prompt`, with the prompt as its `request`. Their
//! results are the agent's context, again after a compaction, and what the reviewer knows of the
//! request. What a workflow does (its steps, what it asks, what it hands over) is in its file;
//! here is only when it runs and where its result goes. Everything lives in the session's state;
//! nothing is written to the repository.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use forge_core::{Note, NoteKind};
use serde_json::{Map, Value, json};

use crate::input::HookInput;
use crate::workflow::{self, Trigger};
use crate::{Project, Runtime, state};

/// The longest the first prompt waits for its workflows (the hook's timeout, 1800 s, is longer).
const FIRST_PROMPT_CAP: Duration = Duration::from_secs(1740);

fn runs_dir(p: &Project) -> PathBuf {
    p.session_dir.join("workflows")
}

fn log(p: &Project, input: &HookInput, workflow: &str, detail: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    state::append_line(
        &p.state.join("workflows.log"),
        &format!(
            "{ts}\t{}\t{workflow}\t{}",
            input.session_key(),
            detail.replace(['\t', '\n'], " ")
        ),
    );
}

/// UserPromptSubmit: on the session's first prompt, the first-prompt workflows, their results
/// joined as the agent's context. Never twice in a session, never inside Forge's own agents; a
/// workflow that fails leaves a note and the session goes on without its result.
pub fn first_prompt(rt: &Runtime, input: &HookInput, p: &Project) -> Option<String> {
    if !rt.env.spec || rt.env.in_review {
        return None;
    }
    let done = p.session_dir.join("first-prompt-workflows");
    if done.exists() {
        return None;
    }
    state::write_atomic(&done, b"");
    let request = input.prompt.as_deref()?.trim();
    if request.is_empty() {
        return None;
    }
    let cwd = rt.cwd(input);
    let model = workflow::Claude {
        exe: rt.env.claude.clone(),
        model: rt.env.review_model.clone(),
    };
    let mut inputs = Map::new();
    inputs.insert("request".into(), json!(request));
    let started = Instant::now();
    let mut results = Vec::new();
    for (name, text) in workflow::BUILTIN {
        let wf = match workflow::load(text) {
            Ok(wf) if wf.on == Some(Trigger::FirstPrompt) => wf,
            Ok(_) => continue,
            Err(why) => {
                log(p, input, name, &format!("invalid: {why}"));
                continue;
            }
        };
        let run = workflow::run(
            &wf,
            &inputs,
            &cwd,
            &model,
            FIRST_PROMPT_CAP.saturating_sub(started.elapsed()),
        );
        let _ = std::fs::create_dir_all(runs_dir(p));
        state::write_atomic(
            &runs_dir(p).join(format!("{name}.json")),
            &serde_json::to_vec_pretty(&run).unwrap_or_default(),
        );
        for s in &run.steps {
            log(
                p,
                input,
                name,
                &format!(
                    "{} {}",
                    s.step,
                    if s.skipped {
                        "skipped".to_string()
                    } else {
                        format!("{:.0}s", s.seconds)
                    }
                ),
            );
        }
        match (&run.result, &run.error) {
            (Some(r), None) if !r.is_empty() => {
                log(p, input, name, "result handed over");
                results.push(r.clone());
            }
            (_, why) => {
                let why = why.clone().unwrap_or_else(|| "an empty result".into());
                log(p, input, name, &format!("failed: {why}"));
                p.note(&[Note::new(
                    NoteKind::ForgeFailure,
                    format!("The workflow `{name}` did not finish ({why}); the session went on without it."),
                )]);
            }
        }
    }
    (!results.is_empty()).then(|| results.join("\n\n"))
}

/// The results the session's workflows handed over, in their order; None when there are none.
pub fn results(p: &Project) -> Option<String> {
    let out: Vec<String> = workflow::BUILTIN
        .iter()
        .filter_map(|(name, _)| {
            let text = std::fs::read(runs_dir(p).join(format!("{name}.json"))).ok()?;
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
