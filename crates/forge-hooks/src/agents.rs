//! Forge's own agent: the reviewer runs as a native Claude Code subagent of the session, never as
//! a separate `claude -p` process. `forge apply` writes its definition ([`definitions`]) into
//! Claude Code's user config directory; this module is what the hooks do around it.
//!
//! - **Forge writes the prompt.** PreToolUse on the main agent's Agent call for the reviewer
//!   replaces its `prompt` with the waiting review's, so the main agent starts the reviewer but
//!   never shapes what it is told; it starts with a fresh context.
//! - **The reviewer** answers an ask nobody can answer (§9.5): the ask is refused with
//!   [`REVIEW_CALL`], the reviewer runs on Forge's prompt, its verdict goes to the review cache,
//!   and the retried call is decided from it. Its own calls make no edits, and past its time
//!   every call is refused.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::input::HookInput;
use crate::{Output, Runtime, pre_decision, state};

/// The reviewer's name.
pub const REVIEW_AGENT: &str = "quality-reviewer";

/// The refusal of an ask that waits for the reviewer.
pub const REVIEW_CALL: &str = "An independent review is needed before this call: call the Agent tool with subagent_type \"quality-reviewer\" (any prompt: the tooling sets it), then retry the same call unchanged.";
const NO_REVIEW: &str = "No review is waiting.";
const READ_ONLY: &str = "You never change the project's files.";
const TIME_UP: &str = "Time is up: end now with your answer as it stands.";
const REVIEWED: &str = "The review is recorded: retry the same call unchanged.";
/// What the reviewer is told once its answer is recorded: its last message is what
/// the main agent's Agent call returns, and the answer is not the main agent's to read.
const CLOSE: &str = "Your answer is recorded. End now with only the word: done.";

/// The reviewer's time.
pub const REVIEW_CAP: u64 = 170;
/// Answers the reviewer may give before its schema's refusal ends it.
const ANSWERS_MAX: u32 = 3;
// ---- The agent definitions

/// The agents' system prompt: the task is the message the hook sets.
const BODY: &str = "You do one task for this project's tooling. The task is the message you receive: follow it exactly. You never change the project's files. End with your answer as the message describes, and nothing else.";

/// The reasoning effort of Forge's agents when `FORGE_REVIEW_EFFORT` is unset: Claude Code's
/// default for the current models (Sonnet, Opus and Haiku 5.5, Fable 5.2), which the `claude -p`
/// steps ran at. Without an `effort` field an agent would take the session's level (`--effort
/// max` in a benchmark harness: a step thirty times longer).
pub const DEFAULT_EFFORT: &str = "medium";

/// The levels `effort` accepts.
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The definitions `forge apply` writes into `<claude config dir>/agents/`: (file name, text).
/// `model`: `FORGE_REVIEW_MODEL` as it is when `forge apply` runs, else the session's model;
/// `effort`: `FORGE_REVIEW_EFFORT`, else [`DEFAULT_EFFORT`] (an unknown level too).
pub fn definitions(model: Option<&str>, effort: Option<&str>) -> Vec<(String, String)> {
    let model = model.filter(|m| !m.is_empty()).unwrap_or("inherit");
    let effort = effort
        .filter(|e| EFFORTS.contains(e))
        .unwrap_or(DEFAULT_EFFORT);
    // every tool of the session, Serena's included (no `tools`): the hooks refuse its edits
    [(
            REVIEW_AGENT,
            "An independent review of one action, for this project's tooling.",
    )]
    .into_iter()
    .map(|(name, description)| {
        (
            format!("{name}.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\nmodel: {model}\neffort: {effort}\nbackground: false\n---\n\n{BODY}\n"
            ),
        )
    })
    .collect()
}

/// The agents Forge had before, or whose names they had (their files are removed when they are
/// Forge's): the workflow step agent is gone with the workflows.
const OLD_AGENTS: [&str; 3] = [
    "forge-workflow-step",
    "forge-reviewer",
    "quality-workflow-step",
];

/// Writes the definitions into `claude_dir/agents/` where they differ; the files written.
pub fn install(
    claude_dir: &Path,
    model: Option<&str>,
    effort: Option<&str>,
) -> std::io::Result<Vec<PathBuf>> {
    let dir = claude_dir.join("agents");
    std::fs::create_dir_all(&dir)?;
    let ours = |path: &Path| std::fs::read_to_string(path).is_ok_and(|t| t.contains(BODY));
    for old in OLD_AGENTS {
        let path = dir.join(format!("{old}.md"));
        if ours(&path) {
            std::fs::remove_file(&path)?;
        }
    }
    let mut written = Vec::new();
    for (file, text) in definitions(model, effort) {
        let path = dir.join(file);
        let before = std::fs::read_to_string(&path).ok();
        if before.as_deref() == Some(text.as_str()) {
            continue;
        }
        // a person's own agent of that name is never overwritten
        if before.is_some() && !ours(&path) {
            return Err(std::io::Error::other(format!(
                "{} exists and is not this tooling's agent; rename it so the tooling can install its own",
                path.display()
            )));
        }
        std::fs::write(&path, &text)?;
        written.push(path);
    }
    Ok(written)
}

// ---- Who a call belongs to

/// Whose event or tool call a hook sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Main,
    Reviewer,
    /// Another subagent: Forge's hooks treat it as the main agent's work.
    Other,
}

pub fn who(input: &HookInput) -> Who {
    if input.agent_id.is_none() {
        return Who::Main;
    }
    match input.agent_type.as_deref() {
        Some(REVIEW_AGENT) => Who::Reviewer,
        _ => Who::Other,
    }
}

/// The Agent tool (`Task` in older Claude Code).
fn is_agent_tool(tool: &str) -> bool {
    matches!(tool, "Agent" | "Task")
}

fn subagent_type(input: &HookInput) -> Option<&str> {
    input.input_str("subagent_type")
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

// ---- Reviews

/// An ask waiting for the reviewer (§9.5).
#[derive(Debug, Serialize, Deserialize)]
pub struct Review {
    /// The review cache's key.
    pub key: String,
    pub prompt: String,
    pub tool: String,
    pub tool_input: String,
    /// Where the verdict is logged (`reviews.log`).
    pub log_state: PathBuf,
    /// The session's review cache.
    pub cache: PathBuf,
    pub created: f64,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub started: Option<f64>,
    #[serde(default)]
    pub refused: u32,
}

/// Where a session's reviews wait: `dir` is the session's directory (the project's, or the user
/// level's).
pub fn reviews_dir(dir: &Path) -> PathBuf {
    dir.join("agents").join("reviews")
}

/// Records a review to run; the ask is refused with [`REVIEW_CALL`] until its verdict is in.
pub fn request_review(dir: &Path, review: &Review) {
    let d = reviews_dir(dir);
    if state::ensure_dir(&d) {
        state::write_atomic(
            &d.join(format!("{}.json", review.key)),
            &serde_json::to_vec(review).unwrap_or_default(),
        );
    }
}

/// The reviews waiting, oldest first.
fn reviews(dir: &Path) -> Vec<(PathBuf, Review)> {
    let mut all: Vec<(PathBuf, Review)> = std::fs::read_dir(reviews_dir(dir))
        .map(|d| {
            d.flatten()
                .filter_map(|e| {
                    let path = e.path();
                    let r: Review = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
                    Some((path, r))
                })
                .collect()
        })
        .unwrap_or_default();
    all.sort_by(|a, b| a.1.created.total_cmp(&b.1.created));
    all
}

fn save_review(path: &Path, r: &Review) {
    state::write_atomic(path, &serde_json::to_vec(r).unwrap_or_default());
}

/// The session directory a call's reviews live in: the project's, else the user level's.
fn session_dir_of(rt: &Runtime, input: &HookInput) -> PathBuf {
    rt.project(input).map_or_else(
        || rt.user_state().join("sessions").join(input.session_key()),
        |p| p.session_dir,
    )
}

// ---- Closing: the agent's answer stays out of the main agent's context

/// The marker of an agent whose answer is recorded and which now ends with [`CLOSE`].
fn closing_file(dir: &Path, agent: &str) -> PathBuf {
    dir.join("agents").join("closing").join(agent)
}

/// Marks `input`'s agent as closing and sends it back with [`CLOSE`].
fn close(dir: &Path, input: &HookInput) -> Output {
    if let Some(agent) = input.agent_key() {
        let f = closing_file(dir, &agent);
        if f.parent().is_some_and(state::ensure_dir) {
            state::write_atomic(&f, b"");
        }
    }
    Output::json(json!({"decision": "block", "reason": CLOSE}))
}

fn is_closing(dir: &Path, input: &HookInput) -> bool {
    input
        .agent_key()
        .is_some_and(|a| closing_file(dir, &a).is_file())
}

// ---- The hook

/// `forge hook agents` (project level) and `user-agents` (user level: only outside a project
/// set up for reviews, so each review is handled once).
pub fn hook(rt: &Runtime, input: &HookInput, user: bool) -> Output {
    // the user level handles reviews when the project level does not; the project level
    // defers its reviews to the user level when that one is set up
    let user_set = rt.user_lock().is_some_and(|l| l.is_file());
    let reviews_here = if user { true } else { !user_set };
    if !reviews_here {
        return Output::silent();
    }
    let event = input.hook_event_name.as_deref().unwrap_or("");
    let me = who(input);
    let dir = session_dir_of(rt, input);
    if me == Who::Reviewer {
        if event == "PreToolUse" && input.tool() == HANDBACK {
            if is_closing(&dir, input) {
                return handed_back(input);
            }
            let message = input.input_str("message").unwrap_or("").to_string();
            return review_answered(input, &dir, &message, true);
        }
        if is_closing(&dir, input) {
            return match event {
                "SubagentStop" => {
                    if let Some(a) = input.agent_key() {
                        let _ = std::fs::remove_file(closing_file(&dir, &a));
                    }
                    Output::silent()
                }
                "PreToolUse" => pre_decision("deny", CLOSE),
                _ => Output::silent(),
            };
        }
    }
    // messages: the reviewer neither sends nor receives any (its answer stays Forge's); every
    // other agent of the session messages as it likes
    if event == "PreToolUse" && input.tool() == MESSAGE_TOOL {
        if me == Who::Reviewer {
            return pre_decision("deny", NO_MESSAGES);
        }
        let to = input.input_str("to").and_then(crate::input::safe_key);
        if to.is_some_and(|t| ours(&dir, &t)) {
            return pre_decision("deny", NOT_REACHABLE);
        }
    }
    match (event, me) {
        ("PreToolUse", Who::Main)
            if is_agent_tool(input.tool()) && subagent_type(input) == Some(REVIEW_AGENT) =>
        {
            review_prompt(input, &dir)
        }
        ("PreToolUse", Who::Reviewer) => reviewer_call(input, &dir),
        ("PostToolUse", Who::Main)
            if is_agent_tool(input.tool()) && subagent_type(input) == Some(REVIEW_AGENT) =>
        {
            context("PostToolUse", REVIEWED)
        }
        ("SubagentStart", Who::Reviewer) => {
            remember(&dir, input);
            review_started(input, &dir);
            Output::silent()
        }
        ("SubagentStop", Who::Reviewer) => review_stopped(input, &dir),
        _ => Output::silent(),
    }
}

fn context(event: &str, text: &str) -> Output {
    Output::json(json!({
        "hookSpecificOutput": {"hookEventName": event, "additionalContext": text}
    }))
}

/// A PreToolUse answer that lets the call run with `input` instead of the one given.
fn rewrite(input: Value) -> Output {
    Output::json(json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": input,
        }
    }))
}

const EDITS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// A call the reviewer may not make: an edit, or a Serena tool other than its
/// navigation (its edits, notes, shell and configuration).
fn edits(tool: &str) -> bool {
    EDITS.contains(&tool)
        || forge_core::serena::tool_of(tool)
            .is_some_and(|t| forge_core::serena::kind(t) != forge_core::serena::Kind::Read)
}

/// Claude Code's tool through which agents of a session message each other.
const MESSAGE_TOOL: &str = "SendMessage";
/// To the reviewer sending a message.
const NO_MESSAGES: &str = "This agent sends no messages: end with your answer.";
/// To an agent messaging the reviewer.
const NOT_REACHABLE: &str =
    "That agent worked for this project's tooling and has finished: it takes no messages.";

/// `<session>/agents/ids`: the ids of the session's reviewers, one per line.
fn ids_file(dir: &Path) -> PathBuf {
    dir.join("agents").join("ids")
}

/// Records the starting reviewer.
fn remember(dir: &Path, input: &HookInput) {
    if let Some(a) = input.agent_key() {
        let f = ids_file(dir);
        if f.parent().is_some_and(state::ensure_dir) {
            state::append_line(&f, &a);
        }
    }
}

/// Whether `agent` (an id, file-name safe) is one of the session's reviewers.
fn ours(dir: &Path, agent: &str) -> bool {
    std::fs::read_to_string(ids_file(dir)).is_ok_and(|t| t.lines().any(|l| l == agent))
}

/// The name of Claude Code's tool through which a subagent hands its report to its caller (newer
/// versions); its message is what the caller reads.
pub const HANDBACK: &str = "SubagentHandback";

/// A refused answer goes back to the agent: its stop is blocked, or its hand-back refused.
fn answer_again(why: &str, handback: bool) -> Output {
    if handback {
        return pre_decision(
            "deny",
            &format!(
                "{why}. Hand back again with the corrected answer: one JSON object, alone in a ```json block."
            ),
        );
    }
    Output::json(json!({
        "decision": "block",
        "reason": format!("{why}. End again with the corrected answer: one JSON object, alone in a ```json block."),
    }))
}

/// The hand-back goes through with "done" as its message: the answer stays Forge's.
fn handed_back(input: &HookInput) -> Output {
    let mut call = match &input.tool_input {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    call.insert("message".into(), json!("done"));
    rewrite(Value::Object(call))
}

/// PreToolUse of the main agent's Agent call for the reviewer: the oldest waiting review's
/// prompt.
fn review_prompt(input: &HookInput, dir: &Path) -> Output {
    let Some((_, r)) = reviews(dir).into_iter().find(|(_, r)| r.agent.is_none()) else {
        return pre_decision("deny", NO_REVIEW);
    };
    let mut call = match &input.tool_input {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    call.insert("prompt".into(), json!(r.prompt));
    call.remove("model");
    call.insert("run_in_background".into(), json!(false));
    rewrite(Value::Object(call))
}

/// SubagentStart of the reviewer: it takes the oldest waiting review.
fn review_started(input: &HookInput, dir: &Path) {
    let Some(agent) = input.agent_key() else {
        return;
    };
    if let Some((path, mut r)) = reviews(dir).into_iter().find(|(_, r)| r.agent.is_none()) {
        r.agent = Some(agent);
        r.started = Some(now());
        save_review(&path, &r);
    }
}

fn review_of(input: &HookInput, dir: &Path) -> Option<(PathBuf, Review)> {
    let agent = input.agent_key()?;
    reviews(dir)
        .into_iter()
        .find(|(_, r)| r.agent.as_deref() == Some(agent.as_str()))
}

/// The reviewer's own call: no edits; past its time, nothing.
fn reviewer_call(input: &HookInput, dir: &Path) -> Output {
    if edits(input.tool()) {
        return pre_decision("deny", READ_ONLY);
    }
    match review_of(input, dir) {
        Some((_, r)) if r.started.is_some_and(|s| now() - s > REVIEW_CAP as f64) => {
            pre_decision("deny", TIME_UP)
        }
        _ => Output::silent(),
    }
}

/// An answer: the last ```json block of the final message (else the whole message), checked
/// against `schema`. The error says what to correct.
pub fn answer(schema: &Value, message: &str) -> Result<Value, String> {
    let block = message
        .rsplit("```json")
        .next()
        .filter(|_| message.contains("```json"))
        .and_then(|rest| rest.split("```").next());
    let text = block.unwrap_or(message).trim();
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("your answer is not one JSON object in a ```json block ({e})"))?;
    let validator = jsonschema::validator_for(schema).map_err(|e| e.to_string())?;
    let problems: Vec<String> = validator
        .iter_errors(&value)
        .take(10)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    if problems.is_empty() {
        Ok(value)
    } else {
        Err(format!(
            "your answer does not match its JSON Schema: {}",
            problems.join("; ")
        ))
    }
}

/// The verdict schema: the reason first, the verdict following from it.
pub fn verdict_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "reason": {"type": "string"},
            "decision": {"type": "string", "enum": ["allow", "deny"]},
        },
        "required": ["decision", "reason"],
        "additionalProperties": false,
    })
}

/// SubagentStop of the reviewer: its verdict into the review cache and `reviews.log`; a refused
/// answer goes back to it.
fn review_stopped(input: &HookInput, dir: &Path) -> Output {
    let message = input.last_assistant_message.as_deref().unwrap_or("");
    review_answered(input, dir, message, false)
}

/// The reviewer's answer, from its stop or its hand-back: a refused answer goes back to it (its
/// stop blocked, or its hand-back refused); an accepted one lets the hand-back through as "done".
fn review_answered(input: &HookInput, dir: &Path, message: &str, handback: bool) -> Output {
    let Some((path, mut r)) = review_of(input, dir) else {
        return Output::silent();
    };
    let in_time = r.started.is_none_or(|s| now() - s <= REVIEW_CAP as f64);
    match answer(&verdict_schema(), message) {
        Ok(v) => {
            let allow = v["decision"] == "allow";
            let reason = v["reason"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("no reason given")
                .replace(['\t', '\n'], " ");
            state::append_line(
                &r.cache,
                &format!(
                    "{}\t{}\t{reason}",
                    r.key,
                    if allow { "allow" } else { "deny" }
                ),
            );
            crate::review::log_verdict(&r.log_state, input, allow, &r.tool, &r.tool_input, &reason);
            let _ = std::fs::remove_file(&path);
            if handback {
                handed_back(input)
            } else {
                close(dir, input)
            }
        }
        Err(why) if r.refused + 1 < ANSWERS_MAX && in_time => {
            r.refused += 1;
            save_review(&path, &r);
            answer_again(&why, handback)
        }
        Err(_) => {
            // no verdict: the ask stands
            let _ = std::fs::remove_file(&path);
            if handback {
                handed_back(input)
            } else {
                Output::silent()
            }
        }
    }
}

/// The review cache's verdict for `key`, when there is one: (allow, reason).
pub fn cached_verdict(cache: &Path, key: &str) -> Option<(bool, String)> {
    std::fs::read_to_string(cache).ok().and_then(|t| {
        t.lines().rev().find_map(|l| {
            let mut f = l.splitn(3, '\t');
            (f.next()? == key).then(|| {
                (
                    f.next() == Some("allow"),
                    f.next().unwrap_or("").to_string(),
                )
            })
        })
    })
}
