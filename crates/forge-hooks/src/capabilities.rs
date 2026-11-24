//! Capabilities in UserPromptSubmit (§20.3, §20.5): the active, available capabilities not given
//! yet this session are put to the capability engine, which reads the task's digest; the selected
//! ones are the prompt's `additionalContext`. Failures never reach the agent and never hold the
//! prompt: nothing is selected, and the person may get a note.

use std::path::PathBuf;

use forge_capabilities::engine::{Candidate, EngineError};
use forge_capabilities::render::{Give, display_name, render, resolve};
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

/// `<session>/capabilities/turn`: what the current turn was given, as the agent read it (given
/// again after a compaction).
fn turn_file(p: &Project) -> PathBuf {
    dir(p).join("turn")
}

/// The active capabilities (the hooks' cache).
fn active(rt: &Runtime, p: &Project) -> Vec<Capability> {
    p.rules(&rt.home)
        .map(|c| c.capabilities)
        .unwrap_or_default()
}

/// The capabilities a prompt may still get: active, available here, not given yet this session;
/// in pack-id order.
fn candidates(rt: &Runtime, p: &Project, active: &[Capability]) -> Vec<Capability> {
    let given = given(p);
    let provisioner = forge_tools::Provisioner::new(rt.home.clone());
    let mut out: Vec<Capability> = active
        .iter()
        .filter(|c| !given.contains(&c.id))
        .filter(|c| forge_capabilities::available(c, Some(&provisioner)).is_available())
        .cloned()
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

fn give<'a>(c: &'a Capability, active: &[Capability]) -> Give<'a> {
    Give {
        id: &c.id,
        name: display_name(c, active),
        description: &c.description,
        text: &c.text,
    }
}

/// The prompt's capabilities, as the agent reads them; `None` when none is given. `new_turn`: the
/// prompt starts a turn (one that continues the turn adds to its selection).
pub(crate) fn on_prompt(
    rt: &Runtime,
    input: &HookInput,
    p: &Project,
    new_turn: bool,
) -> Option<String> {
    if new_turn {
        let _ = std::fs::remove_file(turn_file(p));
    }
    if !rt.env.capabilities {
        return None;
    }
    let active = active(rt, p);
    let caps = candidates(rt, p, &active);
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
    let inject: Vec<Give> = selected
        .iter()
        .filter(|c| c.deliver == Deliver::Inject)
        .map(|c| give(c, &active))
        .collect();
    let on_demand: Vec<Give> = selected
        .iter()
        .filter(|c| c.deliver == Deliver::OnDemand)
        .map(|c| give(c, &active))
        .collect();
    let r = render(&inject, &on_demand);
    if !r.as_lines.is_empty() {
        p.note(&[Note::new(
            NoteKind::Environment,
            format!(
                "Capabilities given as a line to load rather than their text, the prompt's capabilities being over 8,000 characters together: {}.",
                r.as_lines.join(", ")
            ),
        )]);
    }
    if !r.left_out.is_empty() {
        p.note(&[Note::new(
            NoteKind::Environment,
            format!(
                "Capabilities left out of a prompt, its capabilities being over 8,000 characters together: {}.",
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
        let mut turn = std::fs::read_to_string(turn_file(p)).unwrap_or_default();
        if !turn.is_empty() {
            turn.push_str("\n\n");
        }
        turn.push_str(&r.text);
        state::write_atomic(&turn_file(p), turn.as_bytes());
    }
    Some(r.text)
}

/// After a compaction (SessionStart `compact`): the current turn's capabilities again, as the
/// agent read them. Text a hook injected does not survive a compaction (§20.9).
pub(crate) fn after_compaction(rt: &Runtime, p: &Project) -> Option<String> {
    if !rt.env.capabilities {
        return None;
    }
    std::fs::read_to_string(turn_file(p))
        .ok()
        .filter(|t| !t.trim().is_empty())
}

/// PreToolUse of a Bash command that loads capabilities (`claude-forge use <name>`): each load
/// is logged (§20.7), by the id its name resolves to among the active capabilities.
pub(crate) fn record_loads(rt: &Runtime, p: &Project, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let active = active(rt, p);
    let log_file = log::log_path(&p.state);
    for n in names {
        if let Some(c) = resolve(n, &active) {
            log::load(&log_file, &p.session, &c.id);
        }
    }
}
