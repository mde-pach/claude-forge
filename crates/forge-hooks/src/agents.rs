//! Forge's own agents: the steps of a workflow and the reviewer run as native Claude Code
//! subagents of the session, never as separate `claude -p` processes. `forge apply` writes their
//! definitions ([`definitions`]) into Claude Code's user config directory; this module is what
//! the hooks do around them.
//!
//! - **The gate.** While a workflow step is pending, PreToolUse refuses every tool call of the
//!   main agent but the Agent call for the step agent, and Stop blocks, both with [`STEP_CALL`].
//!   The first prompt held the agent until its workflows were done; the gate does the same.
//! - **Forge writes the prompt.** PreToolUse on that Agent call replaces its `prompt` with the
//!   pending step's (or review's), so the main agent starts Forge's agents but never shapes what
//!   they are told; they start with a fresh context.
//! - **A step's own calls.** No edits; Bash gets the step's own `TMPDIR` and a timeout within the
//!   time left; past the deadline every call is refused, so the step ends with its answer.
//! - **SubagentStart** takes the repository's snapshot ([`workflow::PutBack`]); **SubagentStop**
//!   checks the answer against the step's schema (a refused one goes back to the step, three
//!   answers at most), puts the repository back and moves the run on; PostToolUse on the main
//!   agent's Agent call hands over the next step's instruction or the workflow's result.
//! - **The reviewer** answers an ask nobody can answer (§9.5): the ask is refused with
//!   [`REVIEW_CALL`], the reviewer runs on Forge's prompt, its verdict goes to the review cache,
//!   and the retried call is decided from it.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use forge_core::{Note, NoteKind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::input::HookInput;
use crate::workflow::{self, Done, Next, Pending, PutBack, Run, StepHooks, Trigger};
use crate::{Output, Project, Runtime, pre_decision, state};

/// The step agent's name.
pub const STEP_AGENT: &str = "forge-workflow-step";
/// The reviewer's name.
pub const REVIEW_AGENT: &str = "forge-reviewer";

/// What the main agent is told while a step is pending: in the first prompt's context, as the
/// refusal of any other tool call, and as Stop's reason.
pub const STEP_CALL: &str = "Before anything else, call the Agent tool with subagent_type \"forge-workflow-step\" (any prompt: the tooling sets it). This project's tooling prepares, in a separate agent, the details your work on this request will be checked against; its answer comes back to you here.";
/// The refusal of an ask that waits for the reviewer.
pub const REVIEW_CALL: &str = "An independent review is needed before this call: call the Agent tool with subagent_type \"forge-reviewer\" (any prompt: the tooling sets it), then retry the same call unchanged.";
const STEP_RUNNING: &str =
    "The tooling's step agent is already running; wait for its answer before calling it again.";
const NO_REVIEW: &str = "No review is waiting.";
const READ_ONLY: &str = "You never change the project's files.";
const TIME_UP: &str = "Time is up: end now with your answer as it stands.";
const REVIEWED: &str = "The review is recorded: retry the same call unchanged.";
/// What a step or the reviewer is told once its answer is recorded: its last message is what
/// the main agent's Agent call returns, and the answer is not the main agent's to read.
const CLOSE: &str = "Your answer is recorded. End now with only the word: done.";

/// The longest the first prompt's workflows may take together.
pub const WORKFLOWS_CAP: u64 = 1740;
/// The reviewer's time.
pub const REVIEW_CAP: u64 = 170;
/// Answers a step or the reviewer may give before its schema's refusal ends it.
const ANSWERS_MAX: u32 = 3;
/// Stop blocks of the gate before the workflow is given up.
const STOP_BLOCKS_MAX: u32 = 3;
/// Time left under which a step is told to end.
const WARN_SECS: u64 = 120;
/// Bash's own default timeout, when a call gives none.
const BASH_DEFAULT_MS: u64 = 120_000;

// ---- The agent definitions

/// The agents' system prompt: the task is the message the hook sets.
const BODY: &str = "You do one task for this project's tooling. The task is the message you receive: follow it exactly. You never change the project's files. End with your answer as the message describes, and nothing else.";

/// The definitions `forge apply` writes into `<claude config dir>/agents/`: (file name, text).
/// `model`: `FORGE_REVIEW_MODEL` as it is when `forge apply` runs, else the session's model.
pub fn definitions(model: Option<&str>) -> Vec<(String, String)> {
    let model = model.filter(|m| !m.is_empty()).unwrap_or("inherit");
    [
        (
            STEP_AGENT,
            "One step of a workflow of this project's tooling.",
            "Read, Grep, Glob, Bash",
        ),
        (
            REVIEW_AGENT,
            "An independent review of one action, for this project's tooling.",
            "Read, Grep, Glob",
        ),
    ]
    .into_iter()
    .map(|(name, description, tools)| {
        (
            format!("{name}.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\ntools: {tools}\nmodel: {model}\nbackground: false\n---\n\n{BODY}\n"
            ),
        )
    })
    .collect()
}

/// Writes the definitions into `claude_dir/agents/` where they differ; the files written.
pub fn install(claude_dir: &Path, model: Option<&str>) -> std::io::Result<Vec<PathBuf>> {
    let dir = claude_dir.join("agents");
    std::fs::create_dir_all(&dir)?;
    let mut written = Vec::new();
    for (file, text) in definitions(model) {
        let path = dir.join(file);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(&path, &text)?;
            written.push(path);
        }
    }
    Ok(written)
}

// ---- Who a call belongs to

/// Whose event or tool call a hook sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Main,
    Step,
    Reviewer,
    /// Another subagent: Forge's hooks treat it as the main agent's work.
    Other,
}

pub fn who(input: &HookInput) -> Who {
    if input.agent_id.is_none() {
        return Who::Main;
    }
    match input.agent_type.as_deref() {
        Some(STEP_AGENT) => Who::Step,
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

// ---- Workflow state

/// The session's workflows, saved between the hook calls that drive them.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Workflows {
    /// Workflows still to start, by name, with the inputs they get.
    queue: Vec<String>,
    inputs: Map<String, Value>,
    /// When all of them must be done (seconds since the Unix epoch).
    deadline: u64,
    pending: Option<Pending>,
    running: Option<Running>,
    /// What the main agent is told after the step agent returns.
    notice: Option<String>,
    stop_blocks: u32,
}

/// The step agent while it runs.
#[derive(Debug, Serialize, Deserialize)]
struct Running {
    agent: String,
    started: f64,
    put_back: Option<PutBack>,
    tmp: PathBuf,
}

fn agents_dir(p: &Project) -> PathBuf {
    p.session_dir.join("agents")
}

fn wf_file(p: &Project) -> PathBuf {
    agents_dir(p).join("workflows.json")
}

fn load_wf(p: &Project) -> Option<Workflows> {
    serde_json::from_slice(&std::fs::read(wf_file(p)).ok()?).ok()
}

fn save_wf(p: &Project, w: &Workflows) {
    if state::ensure_dir(&agents_dir(p)) {
        state::write_atomic(&wf_file(p), &serde_json::to_vec(w).unwrap_or_default());
    }
}

fn runs_dir(p: &Project) -> PathBuf {
    p.session_dir.join("workflows")
}

fn log(p: &Project, session: &str, workflow: &str, detail: &str) {
    let ts = now() as u64;
    state::append_line(
        &p.state.join("workflows.log"),
        &format!(
            "{ts}\t{session}\t{workflow}\t{}",
            detail.replace(['\t', '\n'], " ")
        ),
    );
}

/// A finished run: its record, its log line, its note when it failed; its result, if any.
fn finish(p: &Project, session: &str, name: &str, run: &Run) -> Option<String> {
    let _ = std::fs::create_dir_all(runs_dir(p));
    state::write_atomic(
        &runs_dir(p).join(format!("{name}.json")),
        &serde_json::to_vec_pretty(run).unwrap_or_default(),
    );
    match (&run.result, &run.error) {
        (Some(r), None) if !r.is_empty() => {
            log(p, session, name, "result handed over");
            Some(r.clone())
        }
        (_, why) => {
            let why = why.clone().unwrap_or_else(|| "an empty result".into());
            log(p, session, name, &format!("failed: {why}"));
            p.note(&[Note::new(
                NoteKind::ForgeFailure,
                format!(
                    "The workflow `{name}` did not finish ({why}); the session went on without it."
                ),
            )]);
            None
        }
    }
}

/// Starts the queued workflows until one waits for a step; the results of the ones that ended
/// at once.
fn advance_queue(rt: &Runtime, p: &Project, session: &str, w: &mut Workflows) -> Vec<String> {
    let mut results = Vec::new();
    while w.pending.is_none() && !w.queue.is_empty() {
        let name = w.queue.remove(0);
        let wf = match workflow::named(rt.env.workflows_dir.as_deref(), &name) {
            Ok(wf) => wf,
            Err(why) => {
                log(p, session, &name, &format!("invalid: {why}"));
                continue;
            }
        };
        match workflow::start(&wf, &name, &w.inputs, &p.root, w.deadline) {
            Next::Step(pending) => {
                log(p, session, &name, "started");
                w.pending = Some(*pending);
            }
            Next::Done(run) => results.extend(finish(p, session, &name, &run)),
        }
    }
    results
}

/// What the main agent is told: the results so far, then the instruction for a pending step.
fn tell(results: &[String], pending: bool) -> Option<String> {
    let mut parts: Vec<String> = results.to_vec();
    if pending {
        parts.push(STEP_CALL.to_string());
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// UserPromptSubmit: on the session's first prompt, the first-prompt workflows start with the
/// prompt as `request`; the main agent is told to run the first step (or gets the results of
/// workflows that needed no step). Never twice in a session.
pub(crate) fn first_prompt(rt: &Runtime, input: &HookInput, p: &Project) -> Option<String> {
    if !rt.env.spec {
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
    let queue: Vec<String> = workflow::available(rt.env.workflows_dir.as_deref())
        .into_iter()
        .filter_map(|(name, text)| match text.and_then(|t| workflow::load(&t)) {
            Ok(wf) if wf.on == Some(Trigger::FirstPrompt) => Some(name),
            Ok(_) => None,
            Err(why) => {
                log(p, &p.session, &name, &format!("invalid: {why}"));
                None
            }
        })
        .collect();
    let mut inputs = Map::new();
    inputs.insert("request".into(), json!(request));
    let mut w = Workflows {
        queue,
        inputs,
        deadline: now() as u64 + WORKFLOWS_CAP,
        ..Workflows::default()
    };
    let results = advance_queue(rt, p, &p.session.clone(), &mut w);
    save_wf(p, &w);
    tell(&results, w.pending.is_some())
}

/// Stop of the main agent: blocks while a step is pending (the gate), at most
/// [`STOP_BLOCKS_MAX`] times; then the workflow is given up.
pub(crate) fn stop(rt: &Runtime, p: &Project) -> Option<Output> {
    let mut w = load_wf(p)?;
    let pending = w.pending.take()?;
    if w.running.is_none() && w.stop_blocks >= STOP_BLOCKS_MAX {
        let name = pending.name.clone();
        let failed = named_fail(rt, pending, "the agent did not run the step");
        let _ = finish(p, &p.session, &name, &failed);
        w.queue.clear();
        w.stop_blocks = 0;
        save_wf(p, &w);
        return None;
    }
    w.pending = Some(pending);
    w.stop_blocks += 1;
    save_wf(p, &w);
    Some(Output::json(
        json!({"decision": "block", "reason": STEP_CALL}),
    ))
}

fn named_fail(rt: &Runtime, pending: Pending, why: &str) -> Run {
    match workflow::named(rt.env.workflows_dir.as_deref(), &pending.name) {
        Ok(wf) => workflow::fail(&wf, pending, why, Done::default()),
        Err(e) => {
            let mut run = pending.run;
            run.error = Some(e);
            run
        }
    }
}

/// The step that applies to a step agent's call: its hooks; None outside a run.
pub(crate) fn step_hooks(rt: &Runtime, p: &Project) -> Option<StepHooks> {
    let w = load_wf(p)?;
    let pending = w.pending?;
    let wf = workflow::named(rt.env.workflows_dir.as_deref(), &pending.name).ok()?;
    workflow::step_at(&wf, pending.index).map(|(_, s)| s.forge)
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

/// `forge hook agents` (project level) and `user-agents` (user level: reviews only, and only
/// outside a project set up for them, so each review is handled once).
pub fn hook(rt: &Runtime, input: &HookInput, user: bool) -> Output {
    let project = rt.project(input);
    // the user level handles reviews when the project level does not; the project level
    // defers its reviews to the user level when that one is set up
    let user_set = rt.user_lock().is_some_and(|l| l.is_file());
    let reviews_here = if user { true } else { !user_set };
    let workflows_here = !user;
    let event = input.hook_event_name.as_deref().unwrap_or("");
    let me = who(input);
    if matches!(me, Who::Step | Who::Reviewer) {
        let dir = session_dir_of(rt, input);
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
    match (event, me) {
        ("PreToolUse", Who::Main) => {
            if workflows_here {
                if let Some(out) = project.as_ref().and_then(|p| gate(input, p)) {
                    return out;
                }
            }
            if reviews_here
                && is_agent_tool(input.tool())
                && subagent_type(input) == Some(REVIEW_AGENT)
            {
                return review_prompt(input, &session_dir_of(rt, input));
            }
            Output::silent()
        }
        ("PreToolUse", Who::Step) if workflows_here => project
            .as_ref()
            .map_or_else(Output::silent, |p| step_call(input, p)),
        ("PreToolUse", Who::Reviewer) if reviews_here => {
            reviewer_call(input, &session_dir_of(rt, input))
        }
        ("PostToolUse", Who::Main) if is_agent_tool(input.tool()) => match subagent_type(input) {
            Some(STEP_AGENT) if workflows_here => project
                .as_ref()
                .and_then(|p| {
                    let mut w = load_wf(p)?;
                    let notice = w.notice.take();
                    save_wf(p, &w);
                    notice
                })
                .map_or_else(Output::silent, |n| context("PostToolUse", &n)),
            Some(REVIEW_AGENT) if reviews_here => context("PostToolUse", REVIEWED),
            _ => Output::silent(),
        },
        ("SubagentStart", Who::Step) if workflows_here => {
            if let Some(p) = &project {
                step_started(input, p);
            }
            Output::silent()
        }
        ("SubagentStart", Who::Reviewer) if reviews_here => {
            review_started(input, &session_dir_of(rt, input));
            Output::silent()
        }
        ("SubagentStop", Who::Step) if workflows_here => project
            .as_ref()
            .map_or_else(Output::silent, |p| step_stopped(rt, input, p)),
        ("SubagentStop", Who::Reviewer) if reviews_here => {
            review_stopped(input, &session_dir_of(rt, input))
        }
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

/// The gate: while a step is pending, only the Agent call for the step agent, with Forge's
/// prompt; every other call of the main agent is refused.
fn gate(input: &HookInput, p: &Project) -> Option<Output> {
    let w = load_wf(p)?;
    let pending = w.pending.as_ref()?;
    if !(is_agent_tool(input.tool()) && subagent_type(input) == Some(STEP_AGENT)) {
        return Some(pre_decision("deny", STEP_CALL));
    }
    if w.running.is_some() {
        return Some(pre_decision("deny", STEP_RUNNING));
    }
    let mut call = match &input.tool_input {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    call.insert("prompt".into(), json!(pending.prompt));
    call.remove("model");
    call.insert("run_in_background".into(), json!(false));
    Some(rewrite(Value::Object(call)))
}

const EDITS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// A step agent's own call: no edits; past the deadline nothing; Bash with the step's `TMPDIR`
/// and a timeout within the time left.
fn step_call(input: &HookInput, p: &Project) -> Output {
    if EDITS.contains(&input.tool()) {
        return pre_decision("deny", READ_ONLY);
    }
    let Some(w) = load_wf(p) else {
        return Output::silent();
    };
    let (Some(pending), Some(running)) = (&w.pending, &w.running) else {
        return Output::silent();
    };
    let left = pending.deadline as f64 - now();
    if left <= 0.0 {
        return pre_decision("deny", TIME_UP);
    }
    if input.tool() == "Bash" {
        return bash_within(input, &running.tmp, left);
    }
    Output::silent()
}

/// A Bash call that runs with `TMPDIR` set to `tmp` and ends before the time left.
fn bash_within(input: &HookInput, tmp: &Path, left: f64) -> Output {
    let Value::Object(mut call) = input.tool_input.clone() else {
        return Output::silent();
    };
    let Some(cmd) = call.get("command").and_then(Value::as_str) else {
        return Output::silent();
    };
    let tmp = tmp.display().to_string().replace('\'', "'\\''");
    let cmd = format!("export TMPDIR='{tmp}'\n{cmd}");
    call.insert("command".into(), json!(cmd));
    let asked = call
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(BASH_DEFAULT_MS);
    let left_ms = (left * 1000.0) as u64;
    call.insert("timeout".into(), json!(asked.min(left_ms.max(1000))));
    rewrite(Value::Object(call))
}

/// PostToolUse of a step agent's call: told to end when little time is left.
pub(crate) fn step_after_call(p: &Project) -> Option<Output> {
    let w = load_wf(p)?;
    let pending = w.pending.as_ref()?;
    w.running.as_ref()?;
    let left = pending.deadline as f64 - now();
    (left < WARN_SECS as f64).then(|| {
        context(
            "PostToolUse",
            &format!("{} s left: end with your answer now.", left.max(0.0) as u64),
        )
    })
}

/// SubagentStart of the step agent: the repository's snapshot, the step's own temp directory.
fn step_started(input: &HookInput, p: &Project) {
    let (Some(mut w), Some(agent)) = (load_wf(p), input.agent_key()) else {
        return;
    };
    if w.pending.is_none() || w.running.is_some() {
        return;
    }
    let dir = agents_dir(p).join(&agent);
    let put_back = PutBack::take(&p.root, &dir.join("snap"));
    let tmp = step_tmp(&agent);
    w.running = Some(Running {
        agent,
        started: now(),
        put_back,
        tmp,
    });
    save_wf(p, &w);
}

/// A step's own temp directory: short, under `/tmp`, private to the user.
fn step_tmp(agent: &str) -> PathBuf {
    use std::os::unix::fs::DirBuilderExt;
    let short: String = agent.chars().rev().take(12).collect();
    let base = if Path::new("/tmp").is_dir() {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!("fw-{short}"));
    let _ = std::fs::DirBuilder::new().mode(0o700).create(&dir);
    dir
}

/// SubagentStop of the step agent: its answer checked (a refused one goes back to it, with
/// time left and fewer than [`ANSWERS_MAX`] answers), the repository put back, the run moved
/// on; what the main agent is told next waits in `notice`.
fn step_stopped(rt: &Runtime, input: &HookInput, p: &Project) -> Output {
    let Some(mut w) = load_wf(p) else {
        return Output::silent();
    };
    let (Some(mut pending), Some(running)) = (w.pending.take(), w.running.take()) else {
        return Output::silent();
    };
    let Ok(wf) = workflow::named(rt.env.workflows_dir.as_deref(), &pending.name) else {
        return Output::silent();
    };
    let Some((_, step)) = workflow::step_at(&wf, pending.index) else {
        return Output::silent();
    };
    let message = input.last_assistant_message.as_deref().unwrap_or("");
    let answered = workflow::answer(step, message);
    let time_left = (pending.deadline as f64) > now();
    if let Err(why) = &answered {
        if pending.refused + 1 < ANSWERS_MAX && time_left {
            pending.refused += 1;
            w.pending = Some(pending);
            w.running = Some(running);
            save_wf(p, &w);
            return Output::json(json!({
                "decision": "block",
                "reason": format!("{why}. End again with the corrected answer: one JSON object, alone in a ```json block."),
            }));
        }
    }
    let undone = running.put_back.as_ref().map_or(Ok(()), PutBack::put_back);
    let _ = std::fs::remove_dir_all(&running.tmp);
    let _ = std::fs::remove_dir_all(agents_dir(p).join(&running.agent));
    let transcript = input.agent_transcript_path.clone();
    let done = Done {
        seconds: now() - running.started,
        meta: transcript
            .as_ref()
            .and_then(|t| std::fs::read(t).ok())
            .map(|b| workflow::meta(&b)),
        transcript,
    };
    let name = pending.name.clone();
    let step_name = workflow::step_at(&wf, pending.index).map_or(String::new(), |(s, _)| s.clone());
    log(
        p,
        &p.session,
        &name,
        &format!("{step_name} {:.0}s", done.seconds),
    );
    let mut results = Vec::new();
    match answered.and_then(|v| undone.map(|()| v)) {
        Ok(output) => match workflow::accept(&wf, pending, output, done) {
            Next::Step(next) => w.pending = Some(*next),
            Next::Done(run) => results.extend(finish(p, &p.session, &name, &run)),
        },
        Err(why) => {
            let run = workflow::fail(&wf, pending, &why, done);
            let _ = finish(p, &p.session, &name, &run);
        }
    }
    results.extend(advance_queue(rt, p, &p.session.clone(), &mut w));
    w.stop_blocks = 0;
    w.notice = tell(&results, w.pending.is_some());
    save_wf(p, &w);
    close(&p.session_dir, input)
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
    if EDITS.contains(&input.tool()) {
        return pre_decision("deny", READ_ONLY);
    }
    match review_of(input, dir) {
        Some((_, r)) if r.started.is_some_and(|s| now() - s > REVIEW_CAP as f64) => {
            pre_decision("deny", TIME_UP)
        }
        _ => Output::silent(),
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
    let Some((path, mut r)) = review_of(input, dir) else {
        return Output::silent();
    };
    let schema = verdict_schema();
    let step = workflow::Step {
        condition: None,
        tools: Vec::new(),
        max_turns: None,
        output_schema: schema,
        prompt: String::new(),
        forge: StepHooks::default(),
    };
    let message = input.last_assistant_message.as_deref().unwrap_or("");
    let in_time = r.started.is_none_or(|s| now() - s <= REVIEW_CAP as f64);
    match workflow::answer(&step, message) {
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
            close(dir, input)
        }
        Err(why) if r.refused + 1 < ANSWERS_MAX && in_time => {
            r.refused += 1;
            save_review(&path, &r);
            Output::json(json!({
                "decision": "block",
                "reason": format!("{why}. End again with the corrected answer: one JSON object, alone in a ```json block."),
            }))
        }
        Err(_) => {
            // no verdict: the ask stands
            let _ = std::fs::remove_file(&path);
            Output::silent()
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
