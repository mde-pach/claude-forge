//! Capabilities in UserPromptSubmit (§20.3, §20.5): the active, available capabilities not given
//! yet this session are put to the capability engine, which reads the task's digest; the selected
//! ones are the prompt's `additionalContext`. Failures never reach the agent and never hold the
//! prompt: nothing is selected, and the person may get a note.

use std::path::PathBuf;

use forge_capabilities::engine::{Candidate, EngineError};
use forge_capabilities::render::{Give, render};
use forge_capabilities::{Places, digest, log};
use forge_core::{Note, NoteKind};
use forge_packs::model::{Capability, Deliver};

use crate::input::HookInput;
use crate::{Project, Runtime, state};

/// `<session>/capabilities/`: what this session was given.
fn dir(p: &Project) -> PathBuf {
    p.session_dir.join("capabilities")
}

/// The ids given this session, one per line.
fn given_file(p: &Project) -> PathBuf {
    dir(p).join("given")
}

fn given(p: &Project) -> Vec<String> {
    std::fs::read_to_string(given_file(p))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// The capabilities a prompt may still get: active (the hooks' cache), available here, not given
/// yet this session; in pack-id order.
fn candidates(rt: &Runtime, p: &Project) -> Vec<Capability> {
    let Some(cache) = p.rules(&rt.home) else {
        return Vec::new();
    };
    let given = given(p);
    let provisioner = forge_tools::Provisioner::new(rt.home.clone());
    let mut out: Vec<Capability> = cache
        .capabilities
        .into_iter()
        .filter(|c| c.deliver == Deliver::Inject)
        .filter(|c| !given.contains(&c.id))
        .filter(|c| forge_capabilities::available(c, Some(&provisioner)).is_available())
        .collect();
    out.sort_by(|a, b| (a.pack.as_str(), a.id.as_str()).cmp(&(b.pack.as_str(), b.id.as_str())));
    out
}

/// The note for the person when the engine gave no answer; `None`: no note (off here).
fn failure_note(e: &EngineError) -> Option<Note> {
    let tail = "so no capability was selected for this prompt.";
    match e {
        EngineError::Off => None,
        EngineError::Environment(why) => {
            Some(Note::new(NoteKind::Environment, format!("{why}, {tail}")))
        }
        EngineError::Unauthorized => Some(Note::new(
            NoteKind::Setup,
            format!(
                "The capability engine refused its key (401), {tail} Run `forge apply --user` to give it another."
            ),
        )),
        EngineError::Failure(why) => Some(Note::new(
            NoteKind::ForgeFailure,
            format!("Forge: {why}, {tail}"),
        )),
    }
}

/// The prompt's capabilities, as the agent reads them; `None` when none is given.
pub(crate) fn on_prompt(rt: &Runtime, input: &HookInput, p: &Project) -> Option<String> {
    if !rt.env.capabilities {
        return None;
    }
    let caps = candidates(rt, p);
    if caps.is_empty() {
        return None;
    }
    let prompt = input.prompt.as_deref().unwrap_or("");
    let transcript = input.transcript_path.as_deref().map(std::path::Path::new);
    let d = digest(
        prompt,
        transcript,
        &Places {
            root: &p.root,
            home: rt.env.home_dir.as_deref(),
        },
    );
    let cands: Vec<Candidate> = caps
        .iter()
        .map(|c| Candidate {
            id: &c.id,
            statements: &c.relevant,
            threshold: c.threshold(),
        })
        .collect();
    let decisions = match rt.capabilities.select(&d, &cands) {
        Ok(d) => d,
        Err(e) => {
            if let Some(n) = failure_note(&e) {
                p.note(&[n]);
            }
            return None;
        }
    };
    let log_file = log::log_path(&p.state);
    for dec in &decisions {
        log::decision(&log_file, &p.session, &dec.id, &dec.values, dec.selected);
    }
    let selected: Vec<&Capability> = caps
        .iter()
        .filter(|c| decisions.iter().any(|d| d.id == c.id && d.selected))
        .collect();
    let gives: Vec<Give> = selected
        .iter()
        .map(|c| Give {
            id: &c.id,
            text: &c.text,
        })
        .collect();
    let r = render(&gives);
    if !r.left_out.is_empty() {
        p.note(&[Note::new(
            NoteKind::Environment,
            format!(
                "Capabilities left out of a prompt, their texts being over 8,000 characters together: {}.",
                r.left_out.join(", ")
            ),
        )]);
    }
    if r.given.is_empty() {
        return None;
    }
    if state::ensure_dir(&dir(p)) {
        for id in &r.given {
            state::append_line(&given_file(p), id);
        }
    }
    Some(r.text)
}
