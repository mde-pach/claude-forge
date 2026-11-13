//! Workflows: a chain of read-only steps described in a YAML file, and the text the chain ends
//! with. Each step runs as Forge's step agent, a native Claude Code subagent of the session
//! (`crate::agents`); this module is the chain itself: what each step is asked, what its answer
//! must be, and what the workflow ends with. The spec step is one workflow; the engine knows
//! nothing of specs.
//!
//! A workflow file (strict YAML, as packs are) has an optional trigger `on` (when a session runs
//! it), `inputs` (names given by the caller), `steps` (run in the file's order) and `result` (a
//! template: the text the caller gets). Each step has a `prompt` template, the `tools` it may use,
//! an `output_schema` (JSON Schema its answer must match), an optional `if` (a Jinja expression;
//! false skips the step, whose `output` is then `none`) and `forge`, the hooks of Forge that apply
//! to its tool calls.
//!
//! Templates are Jinja ([minijinja]) with strict undefined values. They see each input by its
//! name, `repo` (the directory the steps read) and `steps.<name>.output`. A template may only
//! name earlier steps: that is checked when the file is loaded, so a run never fails on a typo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use minijinja::{Environment, UndefinedBehavior};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::proc;
use crate::snapshot::BashSnapshots;

/// A workflow file.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When a session runs it on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<Trigger>,
    /// The names the caller gives values for; templates see each by its name.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Run in this order.
    pub steps: IndexMap<String, Step>,
    /// A template: the text the workflow ends with.
    pub result: String,
}

/// When a session runs a workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    /// The session's first prompt, given as the input `request`.
    FirstPrompt,
}

/// One step: one run of Forge's step agent.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// A Jinja expression; when false the step is skipped and its output is `none`.
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Accepted from older files and not used: a step has every tool of the session, Serena's
    /// included. It never changes the repository: an edit is refused, and what its Bash writes is
    /// put back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Accepted from older files and not used: the clock bounds a step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    /// The JSON Schema the step's answer follows.
    pub output_schema: Value,
    /// A Jinja template: what the step is asked.
    pub prompt: String,
    /// Forge's hooks that apply to the step's tool calls.
    #[serde(default)]
    pub forge: StepHooks,
}

/// Which of Forge's hooks apply to a step's own tool calls (the step runs inside the session,
/// where every hook fires).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct StepHooks {
    /// The Bash guard (§9.2).
    pub guard: bool,
    /// The refusal of Forge's own paths (§9.3).
    pub internals: bool,
    /// The edit-time checks after a Bash command (§8.6).
    pub checks: bool,
}

impl Default for StepHooks {
    fn default() -> Self {
        Self {
            guard: true,
            internals: true,
            checks: false,
        }
    }
}

/// The names templates may use besides the inputs.
const BUILTIN_NAMES: [&str; 2] = ["repo", "steps"];
/// Jinja's own global functions, which templates may call.
const JINJA_GLOBALS: [&str; 4] = ["namespace", "range", "dict", "debug"];

/// The JSON Schema of a workflow file.
pub fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Workflow)).unwrap_or(Value::Null)
}

/// A workflow parsed from strict YAML and checked: its schema, its steps' output schemas, its
/// templates (they compile and name only inputs, `repo` and earlier steps) and its tools.
pub fn load(text: &str) -> Result<Workflow, String> {
    let parsed = forge_packs::yaml::parse_strict(text).map_err(|e| e.to_string())?;
    let validator = jsonschema::validator_for(&schema()).map_err(|e| e.to_string())?;
    let problems: Vec<String> = validator
        .iter_errors(&parsed.value)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let wf: Workflow = serde_json::from_value(parsed.value).map_err(|e| e.to_string())?;
    check(&wf)?;
    Ok(wf)
}

fn env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    // A block tag's own line leaves no trace in the text: templates can put tags on lines.
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env
}

/// `{% if expr %}` around the condition: a condition is a template like any other.
fn condition_template(expr: &str) -> String {
    format!("{{% if {expr} %}}true{{% endif %}}")
}

/// The names a template uses that are not its own: `steps.<name>` as `steps.<name>`.
fn names(source: &str) -> Result<BTreeSet<String>, String> {
    let env = env();
    let t = env.template_from_str(source).map_err(|e| e.to_string())?;
    Ok(t.undeclared_variables(true)
        .into_iter()
        .map(|n| {
            let mut parts = n.split('.');
            match (parts.next(), parts.next()) {
                (Some("steps"), Some(step)) => format!("steps.{step}"),
                (Some(first), _) => first.to_string(),
                _ => n,
            }
        })
        .collect())
}

fn check(wf: &Workflow) -> Result<(), String> {
    if wf.steps.is_empty() {
        return Err("a workflow has at least one step".into());
    }
    let known = |earlier: &[&String], n: &str, what: &str| -> Result<(), String> {
        if let Some(step) = n.strip_prefix("steps.") {
            return if earlier.iter().any(|s| s.as_str() == step) {
                Ok(())
            } else {
                Err(format!(
                    "{what} names steps.{step}, which is not an earlier step"
                ))
            };
        }
        if n == "steps" {
            return Err(format!(
                "{what} names `steps` without a step: write steps.<name>"
            ));
        }
        if BUILTIN_NAMES.contains(&n)
            || JINJA_GLOBALS.contains(&n)
            || wf.inputs.iter().any(|i| i == n)
        {
            Ok(())
        } else {
            Err(format!(
                "{what} names `{n}`, which is not an input, `repo` or a step"
            ))
        }
    };
    let mut earlier: Vec<&String> = Vec::new();
    for (name, step) in &wf.steps {
        if BUILTIN_NAMES.contains(&name.as_str()) || wf.inputs.contains(name) {
            return Err(format!(
                "step `{name}` has the name of an input or of `repo`/`steps`"
            ));
        }
        jsonschema::validator_for(&step.output_schema)
            .map_err(|e| format!("step `{name}`: output_schema: {e}"))?;
        let mut sources = vec![(step.prompt.clone(), format!("step `{name}`'s prompt"))];
        if let Some(c) = &step.condition {
            sources.push((condition_template(c), format!("step `{name}`'s if")));
        }
        for (src, what) in sources {
            for n in names(&src).map_err(|e| format!("{what}: {e}"))? {
                known(&earlier, &n, &what)?;
            }
        }
        earlier.push(name);
    }
    for n in names(&wf.result).map_err(|e| format!("result: {e}"))? {
        known(&earlier, &n, "result")?;
    }
    Ok(())
}

// ---- The repository around a step

/// How long one snapshot of the repository may take.
const SNAPSHOT_CAP: std::time::Duration = std::time::Duration::from_secs(60);

/// The repository around a step: the step may write in it while it runs, so the project's own
/// programs work as they would for the agent (one that logs what it does, a database that keeps a
/// journal), then it is put back as it was. When the step starts, the working tree is a git tree
/// in a private object store under `dir` ([`BashSnapshots`]: the user's index and objects are
/// never touched), and its directories are listed; when it ends, the files the step added are
/// removed (ignored ones too), the ones it changed or deleted restored, and the directories it
/// made removed. Saved between the step's start and its end, which are separate hook calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutBack {
    root: PathBuf,
    dir: PathBuf,
    before: String,
    ignored: Vec<String>,
    dirs: BTreeSet<PathBuf>,
    /// What the walk of directories skips: `.git`, Claude's worktrees, the ignored paths that
    /// were there.
    skip: Vec<PathBuf>,
}

impl PutBack {
    /// The repository of `repo` as it is now, its snapshot kept in `dir`. None outside a git
    /// repository, or when the snapshot fails.
    pub fn take(repo: &Path, dir: &Path) -> Option<Self> {
        let out = proc::git(
            repo,
            &["rev-parse", "--show-toplevel", "--absolute-git-dir"],
            proc::GIT_CAP,
        )?;
        let out = String::from_utf8_lossy(&out).into_owned();
        let mut lines = out.lines();
        let (root, git_dir) = (PathBuf::from(lines.next()?), PathBuf::from(lines.next()?));
        std::fs::create_dir_all(dir).ok()?;
        let snaps = BashSnapshots::new(&root, dir, dir);
        let before = snaps.tree("before", true, SNAPSHOT_CAP)?;
        let ignored = ignored(&root, &[], true)?;
        let mut skip = vec![git_dir, root.join(".git"), root.join(".claude/worktrees")];
        skip.extend(ignored.iter().map(|p| root.join(p)));
        let mut dirs = BTreeSet::new();
        walk_dirs(&root, &skip, &mut dirs);
        Some(Self {
            root,
            dir: dir.to_path_buf(),
            before,
            ignored,
            dirs,
            skip,
        })
    }

    /// The repository as it was when the step started; an error naming what could not be put
    /// back.
    pub fn put_back(&self) -> Result<(), String> {
        let not = |what: &str| {
            format!("the step's changes to the repository could not be undone: {what}")
        };
        let snaps = BashSnapshots::new(&self.root, &self.dir, &self.dir);
        let after = snaps
            .tree("after", true, SNAPSHOT_CAP)
            .ok_or_else(|| not("no snapshot after the step"))?;
        let mut ok = snaps.put_back(&self.before, &after, SNAPSHOT_CAP);
        // ignored files the step wrote, outside the ignored paths that were there
        for path in ignored(&self.root, &self.ignored, false).unwrap_or_default() {
            ok &= std::fs::remove_file(self.root.join(path)).is_ok();
        }
        let mut now = BTreeSet::new();
        walk_dirs(&self.root, &self.skip, &mut now);
        // a directory the step made holds only what it wrote (a parent comes first and takes
        // the ones it holds with it)
        for made in now.difference(&self.dirs) {
            if made.exists() {
                ok &= std::fs::remove_dir_all(made).is_ok();
            }
        }
        let check = snaps
            .tree("check", true, SNAPSHOT_CAP)
            .ok_or_else(|| not("no snapshot after putting it back"))?;
        let left: Vec<String> = snaps
            .diff(&self.before, &check)
            .into_iter()
            .map(|(_, p)| p)
            .collect();
        match (ok, left.is_empty()) {
            (true, true) => Ok(()),
            (_, false) => Err(not(&left.join(", "))),
            (false, true) => Err(not("a file or directory it wrote")),
        }
    }
}

/// The ignored paths of the working tree outside `skip`: with `collapse`, a wholly ignored
/// directory as one path; otherwise each file.
fn ignored(root: &Path, skip: &[String], collapse: bool) -> Option<Vec<String>> {
    let mut args = vec![
        "ls-files",
        "--others",
        "--ignored",
        "--exclude-standard",
        "-z",
    ];
    if collapse {
        args.push("--directory");
    }
    let excludes: Vec<String> = skip
        .iter()
        .map(|p| format!(":(exclude,top,literal){p}"))
        .collect();
    args.extend(["--", "."]);
    args.extend(excludes.iter().map(String::as_str));
    let out = proc::git(root, &args, proc::GIT_CAP)?;
    Some(
        out.split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).trim_end_matches('/').to_string())
            .collect(),
    )
}

/// Every directory under `dir`, outside `skip`, symbolic links not followed.
fn walk_dirs(dir: &Path, skip: &[PathBuf], into: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) && !skip.contains(&p) {
            walk_dirs(&p, skip, into);
            into.insert(p);
        }
    }
}

// ---- What a step did

/// What an agent did, read from its transcript (one JSON event per line, as Claude Code writes
/// it): `model`, `turns` (the model's messages), `tool_calls` by tool, `last_tool`, and `tokens`
/// (`input`, `output`, `cache_read`, `cache_write`; each message counted once, at its last
/// usage).
pub fn meta(transcript: &[u8]) -> Value {
    let mut model = Value::Null;
    let mut messages: Vec<(String, Value)> = Vec::new();
    let mut tools: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut last_tool = Value::Null;
    for line in transcript.split(|b| *b == b'\n') {
        let Ok(e) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if e["type"] != "assistant" {
            continue;
        }
        let m = &e["message"];
        if m["model"].is_string() {
            model = m["model"].clone();
        }
        let id = m["id"].as_str().unwrap_or_default().to_string();
        match messages.iter_mut().find(|(i, _)| *i == id) {
            Some(seen) => seen.1 = m["usage"].clone(),
            None => messages.push((id, m["usage"].clone())),
        }
        for c in m["content"].as_array().into_iter().flatten() {
            let name = c["name"].as_str().unwrap_or_default();
            if c["type"] == "tool_use" && seen.insert(c["id"].to_string()) {
                *tools.entry(name.to_string()).or_default() += 1;
                last_tool = json!(name);
            }
        }
    }
    let sum = |k: &str| -> u64 {
        messages
            .iter()
            .map(|(_, u)| u[k].as_u64().unwrap_or(0))
            .sum()
    };
    let mut m = Map::new();
    for (k, v) in [
        ("model", model),
        ("turns", json!(messages.len())),
        ("tool_calls", json!(tools)),
        ("last_tool", last_tool),
        (
            "tokens",
            json!({
                "input": sum("input_tokens"),
                "output": sum("output_tokens"),
                "cache_read": sum("cache_read_input_tokens"),
                "cache_write": sum("cache_creation_input_tokens"),
            }),
        ),
    ] {
        if !v.is_null() {
            m.insert(k.into(), v);
        }
    }
    Value::Object(m)
}

// ---- A run, one step at a time

/// One step of a run, for the log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRun {
    pub step: String,
    pub skipped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub output: Value,
    pub seconds: f64,
    /// How many answers the step gave (the ones its schema refused, then the last).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
    /// The step agent's transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    /// What the step agent did (turns, tools, tokens): [`meta`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A whole run: every step, then the result (absent when a step failed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub workflow: String,
    pub steps: Vec<StepRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A run in progress, saved between the hook calls that drive it: the step it waits for and
/// everything earlier steps gave.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pending {
    /// The workflow's name (its file name).
    pub name: String,
    pub inputs: Map<String, Value>,
    pub repo: PathBuf,
    /// The step the run waits for (its index in the file).
    pub index: usize,
    /// What that step is asked: its rendered prompt and the answer it must end with.
    pub prompt: String,
    /// Answers the step gave so far that its schema refused.
    pub refused: u32,
    /// When the run must be done (seconds since the Unix epoch).
    pub deadline: u64,
    pub run: Run,
}

/// Where a run goes next.
#[derive(Debug)]
pub enum Next {
    /// A step to run.
    Step(Box<Pending>),
    /// Done: the run with its result, or with its error.
    Done(Run),
}

/// The step at `index` of `wf`.
pub fn step_at(wf: &Workflow, index: usize) -> Option<(&String, &Step)> {
    wf.steps.get_index(index)
}

/// What a step is asked: its prompt, then the answer it must end with.
fn with_contract(prompt: &str, schema: &Value) -> String {
    format!(
        "{}\n\nEnd with your answer: one JSON object, alone in a ```json block, that matches this JSON Schema:\n{schema}",
        prompt.trim_end()
    )
}

/// The context templates see after the steps of `run`.
fn context(inputs: &Map<String, Value>, repo: &Path, run: &Run) -> Map<String, Value> {
    let mut ctx = inputs.clone();
    ctx.insert("repo".into(), json!(repo.display().to_string()));
    let steps: Map<String, Value> = run
        .steps
        .iter()
        .map(|s| (s.step.clone(), json!({ "output": s.output })))
        .collect();
    ctx.insert("steps".into(), Value::Object(steps));
    ctx
}

fn render(src: &str, ctx: &Map<String, Value>) -> Result<String, String> {
    env()
        .template_from_str(src)
        .and_then(|t| t.render(ctx))
        .map_err(|e| e.to_string())
}

/// The run from step `index` on: the first step to run (skipped ones recorded), or the result.
fn from_step(
    wf: &Workflow,
    name: &str,
    inputs: Map<String, Value>,
    repo: &Path,
    deadline: u64,
    mut run: Run,
    mut index: usize,
) -> Next {
    while let Some((step_name, step)) = step_at(wf, index) {
        let ctx = context(&inputs, repo, &run);
        let skip = match &step.condition {
            Some(c) => match render(&condition_template(c), &ctx) {
                Ok(v) => v != "true",
                Err(e) => return Next::Done(failed(run, step_name, &e)),
            },
            None => false,
        };
        if !skip {
            return match render(&step.prompt, &ctx) {
                Ok(p) => Next::Step(Box::new(Pending {
                    name: name.to_string(),
                    inputs,
                    repo: repo.to_path_buf(),
                    index,
                    prompt: with_contract(&p, &step.output_schema),
                    refused: 0,
                    deadline,
                    run,
                })),
                Err(e) => Next::Done(failed(run, step_name, &e)),
            };
        }
        run.steps.push(StepRun {
            step: step_name.clone(),
            skipped: true,
            prompt: None,
            output: Value::Null,
            seconds: 0.0,
            attempts: None,
            transcript: None,
            meta: None,
        });
        index += 1;
    }
    let ctx = context(&inputs, repo, &run);
    match render(&wf.result, &ctx) {
        Ok(r) => run.result = Some(r.trim().to_string()),
        Err(e) => run.error = Some(format!("result: {e}")),
    }
    Next::Done(run)
}

fn failed(mut run: Run, step: &str, why: &str) -> Run {
    run.error = Some(format!("step `{step}`: {why}"));
    run
}

/// Starts `wf` (named `name`) in `repo` with `inputs`, to be done by `deadline`.
pub fn start(
    wf: &Workflow,
    name: &str,
    inputs: &Map<String, Value>,
    repo: &Path,
    deadline: u64,
) -> Next {
    let run = Run {
        workflow: wf.id.clone(),
        steps: Vec::new(),
        result: None,
        error: None,
    };
    if let Some(missing) = wf.inputs.iter().find(|i| !inputs.contains_key(*i)) {
        let mut run = run;
        run.error = Some(format!("input `{missing}` is missing"));
        return Next::Done(run);
    }
    from_step(wf, name, inputs.clone(), repo, deadline, run, 0)
}

/// What a step did besides its answer, for its log entry.
#[derive(Debug, Clone, Default)]
pub struct Done {
    pub seconds: f64,
    pub transcript: Option<String>,
    pub meta: Option<Value>,
}

/// The pending step answered `output` (already checked against its schema): the run goes on.
pub fn accept(wf: &Workflow, p: Pending, output: Value, done: Done) -> Next {
    let Some((step, _)) = step_at(wf, p.index) else {
        return Next::Done(failed(
            p.run,
            "?",
            "the workflow file changed during the run",
        ));
    };
    let mut run = p.run;
    run.steps.push(StepRun {
        step: step.clone(),
        skipped: false,
        prompt: Some(p.prompt),
        output,
        seconds: done.seconds,
        attempts: Some(p.refused + 1),
        transcript: done.transcript,
        meta: done.meta,
    });
    from_step(wf, &p.name, p.inputs, &p.repo, p.deadline, run, p.index + 1)
}

/// The pending step failed (`why`): the run ends without a result.
pub fn fail(wf: &Workflow, p: Pending, why: &str, done: Done) -> Run {
    let step = step_at(wf, p.index).map_or("?".to_string(), |(s, _)| s.clone());
    let mut run = p.run;
    run.steps.push(StepRun {
        step: step.clone(),
        skipped: false,
        prompt: Some(p.prompt),
        output: Value::Null,
        seconds: done.seconds,
        attempts: Some(p.refused + 1),
        transcript: done.transcript,
        meta: done.meta,
    });
    failed(run, &step, why)
}

/// A step's answer: the last ```json block of its final message (else the whole message), checked
/// against its schema. The error says what to correct.
pub fn answer(step: &Step, message: &str) -> Result<Value, String> {
    let block = message
        .rsplit("```json")
        .next()
        .filter(|_| message.contains("```json"))
        .and_then(|rest| rest.split("```").next());
    let text = block.unwrap_or(message).trim();
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("your answer is not one JSON object in a ```json block ({e})"))?;
    let validator = jsonschema::validator_for(&step.output_schema).map_err(|e| e.to_string())?;
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

/// The workflows Forge ships, the fallback when no file replaces them.
pub const BUILTIN: [(&str, &str); 1] =
    [("spec-answer", include_str!("../workflows/spec-answer.yaml"))];

/// Where workflow files are read from at run time: `FORGE_WORKFLOWS_DIR`.
pub const DIR_VAR: &str = "FORGE_WORKFLOWS_DIR";

/// The workflows available, by name, in name order: the built-in ones, then each `<name>.yaml` in
/// `dir`, a file replacing the built-in of the same name. Changing a workflow is editing a file.
pub fn available(dir: Option<&Path>) -> Vec<(String, Result<String, String>)> {
    let mut all: std::collections::BTreeMap<String, Result<String, String>> = BUILTIN
        .iter()
        .map(|(n, t)| ((*n).to_string(), Ok((*t).to_string())))
        .collect();
    if let Some(entries) = dir.and_then(|d| std::fs::read_dir(d).ok()) {
        for e in entries.flatten() {
            let path = e.path();
            let Some(name) = path
                .extension()
                .is_some_and(|x| x == "yaml" || x == "yml")
                .then(|| path.file_stem())
                .flatten()
                .and_then(|s| s.to_str())
            else {
                continue;
            };
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()));
            all.insert(name.to_string(), text);
        }
    }
    all.into_iter().collect()
}

/// The workflow named `name`, loaded; an error when it is missing or invalid.
pub fn named(dir: Option<&Path>, name: &str) -> Result<Workflow, String> {
    let text = available(dir)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, t)| t)
        .ok_or_else(|| format!("no workflow `{name}`"))??;
    load(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const TWO_STEPS: &str = r#"id: t
inputs: [request]
steps:
  ask:
    output_schema: {type: object, properties: {qs: {type: array, items: {type: string}}}, required: [qs]}
    prompt: "Request: {{ request }}"
  answer:
    if: steps.ask.output.qs | length > 0
    max_turns: 1
    output_schema: {type: object, properties: {a: {type: string}}, required: [a]}
    prompt: "{% for q in steps.ask.output.qs %}{{ q }};{% endfor %}"
result: "{{ steps.ask.output.qs | length }} {% if steps.answer.output %}{{ steps.answer.output.a }}{% else %}none{% endif %}"
"#;

    fn inputs() -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("request".into(), json!("fix it"));
        m
    }

    fn pending(n: Next) -> Pending {
        match n {
            Next::Step(p) => *p,
            Next::Done(r) => panic!("done: {r:?}"),
        }
    }

    fn done(n: Next) -> Run {
        match n {
            Next::Done(r) => r,
            Next::Step(p) => panic!("pending step {}", p.index),
        }
    }

    /// Drives a run, answering each step with the next of `outputs`, and returns the prompts.
    fn drive(wf: &Workflow, outputs: Vec<Value>) -> (Vec<String>, Run) {
        let mut prompts = Vec::new();
        let mut next = start(wf, "t", &inputs(), Path::new("/r"), u64::MAX);
        for out in outputs {
            let p = pending(next);
            prompts.push(
                p.prompt
                    .split("\n\nEnd with your answer")
                    .next()
                    .unwrap()
                    .to_string(),
            );
            next = accept(wf, p, out, Done::default());
        }
        (prompts, done(next))
    }

    #[test]
    fn steps_run_in_order_and_see_earlier_outputs() {
        let wf = load(TWO_STEPS).unwrap();
        let (prompts, run) = drive(&wf, vec![json!({"qs": ["q1", "q2"]}), json!({"a": "yes"})]);
        assert_eq!(run.error, None);
        assert_eq!(prompts, ["Request: fix it", "q1;q2;"]);
        assert_eq!(run.result.as_deref(), Some("2 yes"));
        assert_eq!(run.steps[0].attempts, Some(1));
    }

    #[test]
    fn a_step_is_told_the_answer_it_must_end_with() {
        let wf = load(TWO_STEPS).unwrap();
        let p = pending(start(&wf, "t", &inputs(), Path::new("/r"), 0));
        assert!(
            p.prompt
                .starts_with("Request: fix it\n\nEnd with your answer: one JSON object")
        );
        assert!(
            p.prompt
                .ends_with(&wf.steps["ask"].output_schema.to_string())
        );
    }

    #[test]
    fn a_false_condition_skips_the_step() {
        let wf = load(TWO_STEPS).unwrap();
        let (prompts, run) = drive(&wf, vec![json!({"qs": []})]);
        assert!(run.steps[1].skipped);
        assert_eq!(prompts.len(), 1);
        assert_eq!(run.result.as_deref(), Some("0 none"));
    }

    #[test]
    fn an_answer_is_its_last_json_block_checked_against_its_schema() {
        let wf = load(TWO_STEPS).unwrap();
        let step = &wf.steps["ask"];
        let msg = "Done.\n```json\n{\"qs\": [\"x\"]}\n```";
        assert_eq!(answer(step, msg).unwrap(), json!({"qs": ["x"]}));
        assert_eq!(answer(step, "{\"qs\": []}").unwrap(), json!({"qs": []}));
        let e = answer(step, "```json\n{\"other\": 1}\n```").unwrap_err();
        assert!(e.contains("does not match its JSON Schema"), "{e}");
        assert!(
            answer(step, "no json here")
                .unwrap_err()
                .contains("not one JSON object")
        );
    }

    #[test]
    fn a_failed_step_ends_the_run_with_what_it_did() {
        let wf = load(TWO_STEPS).unwrap();
        let p = pending(start(&wf, "t", &inputs(), Path::new("/r"), 0));
        let meta = json!({"turns": 7});
        let run = fail(
            &wf,
            p,
            "out of time",
            Done {
                seconds: 3.0,
                transcript: Some("/t.jsonl".into()),
                meta: Some(meta),
            },
        );
        assert_eq!(run.error.as_deref(), Some("step `ask`: out of time"));
        assert_eq!(run.result, None);
        assert_eq!(run.steps[0].meta.as_ref().unwrap()["turns"], 7);
        assert_eq!(run.steps[0].transcript.as_deref(), Some("/t.jsonl"));
    }

    #[test]
    fn a_missing_input_ends_the_run() {
        let wf = load(TWO_STEPS).unwrap();
        let run = done(start(&wf, "t", &Map::new(), Path::new("/r"), 0));
        assert_eq!(run.error.as_deref(), Some("input `request` is missing"));
    }

    #[test]
    fn a_template_naming_a_later_step_is_refused_at_load() {
        let text = TWO_STEPS.replace("Request: {{ request }}", "{{ steps.answer.output.a }}");
        assert!(load(&text).unwrap_err().contains("not an earlier step"));
    }

    #[test]
    fn an_unknown_name_is_refused_at_load() {
        let text = TWO_STEPS.replace("Request: {{ request }}", "{{ reqest }}");
        assert!(load(&text).unwrap_err().contains("`reqest`"));
    }

    #[test]
    fn an_unknown_field_is_refused_and_tools_are_accepted_at_load() {
        assert!(load(&TWO_STEPS.replace("max_turns: 1", "max_turn: 1")).is_err());
        // older files' tool lists load; a step has every tool of the session
        assert!(
            load(&TWO_STEPS.replacen(
                "prompt: \"Request",
                "tools: [Write]\n    prompt: \"Request",
                1
            ))
            .is_ok()
        );
        assert!(
            load(&TWO_STEPS.replacen(
                "prompt: \"Request",
                "forge: {guards: true}\n    prompt: \"Request",
                1
            ))
            .is_err()
        );
    }

    #[test]
    fn a_step_declares_which_of_forges_hooks_apply_to_its_calls() {
        let wf = load(TWO_STEPS).unwrap();
        assert_eq!(wf.steps["ask"].forge, StepHooks::default());
        assert!(wf.steps["ask"].forge.guard && wf.steps["ask"].forge.internals);
        assert!(!wf.steps["ask"].forge.checks);
        let text = TWO_STEPS.replacen(
            "prompt: \"Request",
            "forge: {guard: false, checks: true}\n    prompt: \"Request",
            1,
        );
        let f = load(&text).unwrap().steps["ask"].forge;
        assert_eq!(
            f,
            StepHooks {
                guard: false,
                internals: true,
                checks: true
            }
        );
    }

    /// An agent's transcript as Claude Code writes it: two model messages (the first written
    /// twice, as a message with several blocks is), a tool result between them.
    const TRANSCRIPT: &str = r#"{"type":"user","message":{"content":"task"}}
{"type":"assistant","message":{"id":"m1","model":"claude-sonnet-x","content":[{"type":"text","text":"Looking."}],"usage":{"input_tokens":2,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":3000}}}
{"type":"assistant","message":{"id":"m1","model":"claude-sonnet-x","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}],"usage":{"input_tokens":2,"output_tokens":16,"cache_read_input_tokens":0,"cache_creation_input_tokens":3000}}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}
{"type":"assistant","message":{"id":"m2","model":"claude-sonnet-x","content":[{"type":"tool_use","id":"t2","name":"Read","input":{}}],"usage":{"input_tokens":2,"output_tokens":24,"cache_read_input_tokens":3000,"cache_creation_input_tokens":100}}}
{"type":"assistant","message":{"id":"m3","mod"#;

    #[test]
    fn a_step_records_its_model_turns_tools_and_tokens() {
        let m = meta(TRANSCRIPT.as_bytes());
        assert_eq!(m["model"], "claude-sonnet-x");
        assert_eq!(m["turns"], 2);
        assert_eq!(m["tool_calls"], json!({"Bash": 1, "Read": 1}));
        assert_eq!(m["last_tool"], "Read");
        // each message counted once, at its last usage; a line cut short is skipped
        assert_eq!(
            m["tokens"],
            json!({"input": 4, "output": 40, "cache_read": 3000, "cache_write": 3100})
        );
    }

    fn git_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().canonicalize().unwrap();
        (dir, repo)
    }

    fn sh(repo: &Path, script: &str) {
        let ok = Command::new("sh")
            .args(["-c", script])
            .current_dir(repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "{script}");
    }

    #[test]
    fn a_step_writes_in_the_repository_and_leaves_it_as_it_was() {
        let (_d, repo) = git_repo();
        sh(
            &repo,
            "git init -q && printf '*.log\\ncache/\\n' > .gitignore && echo a > a.txt && echo b > b.txt \
            && mkdir cache && echo c > cache/c && git add -A && git commit -qm base \
            && echo staged > a.txt && git add a.txt && echo mine > a.txt && echo u > untracked.txt \
            && mkdir output && printf '__pycache__/\\n' >> .git/info/exclude",
        );
        let index = std::fs::read(repo.join(".git/index")).unwrap();
        let state = tempfile::tempdir().unwrap();
        // taken when the step starts, saved, read back when it ends (separate hook calls)
        let saved =
            serde_json::to_string(&PutBack::take(&repo, &state.path().join("snap")).unwrap())
                .unwrap();
        // the step: changes a tracked, an untracked and a deleted file, adds files, ignored files
        // (one in an empty directory that was there) and directories of its own
        sh(
            &repo,
            "echo changed > a.txt; echo changed > untracked.txt; rm b.txt; \
             mkdir -p out && echo log > out/audit.log; echo log > output/audit.log; \
             echo new > new.txt; echo x > run.log; mkdir __pycache__; echo p > __pycache__/m.pyc",
        );
        let p: PutBack = serde_json::from_str(&saved).unwrap();
        p.put_back().unwrap();
        let read = |f: &str| std::fs::read_to_string(repo.join(f)).ok();
        assert_eq!(read("a.txt").as_deref(), Some("mine\n"));
        assert_eq!(read("untracked.txt").as_deref(), Some("u\n"));
        assert_eq!(read("b.txt").as_deref(), Some("b\n"));
        assert_eq!(read("cache/c").as_deref(), Some("c\n"));
        for gone in [
            "out",
            "output/audit.log",
            "new.txt",
            "run.log",
            "__pycache__",
        ] {
            assert!(!repo.join(gone).exists(), "{gone} is left");
        }
        assert!(
            repo.join("output").is_dir(),
            "a directory that was there is removed"
        );
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    }

    #[test]
    fn a_step_writing_in_what_was_ignored_cannot_be_put_back() {
        let (_d, repo) = git_repo();
        sh(
            &repo,
            "git init -q && printf 'cache/\\n' > .gitignore && mkdir cache && echo c > cache/c",
        );
        let state = tempfile::tempdir().unwrap();
        let p = PutBack::take(&repo, &state.path().join("snap")).unwrap();
        // the ignored directory that was there is not the step's: what it writes there stays,
        // and the walk does not go into it
        assert!(p.skip.contains(&repo.join("cache")) && p.skip.contains(&repo.join(".git")));
        let outside = tempfile::tempdir().unwrap();
        assert!(PutBack::take(outside.path(), &state.path().join("x")).is_none());
    }

    #[test]
    fn a_file_replaces_the_builtin_of_its_name_and_adds_new_ones() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("spec-answer.yaml"), TWO_STEPS).unwrap();
        std::fs::write(dir.path().join("extra.yaml"), TWO_STEPS).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a workflow").unwrap();
        let all = available(Some(dir.path()));
        let names: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["extra", "spec-answer"]);
        assert_eq!(all[1].1.as_deref(), Ok(TWO_STEPS));
        assert_eq!(named(Some(dir.path()), "extra").unwrap().id, "t");
        assert!(named(None, "extra").is_err());
        // without a directory: the built-in ones
        let builtin: Vec<String> = available(None).into_iter().map(|(n, _)| n).collect();
        assert_eq!(builtin, BUILTIN.map(|(n, _)| n.to_string()));
    }

    /// A value its schema accepts: required properties, one array item, the first enum value.
    fn sample(schema: &Value) -> Value {
        if let Some(first) = schema["enum"].as_array().and_then(|e| e.first()) {
            return first.clone();
        }
        match schema["type"].as_str() {
            Some("object") => {
                let mut m = Map::new();
                for key in schema["required"].as_array().into_iter().flatten() {
                    let k = key.as_str().unwrap_or_default();
                    m.insert(k.into(), sample(&schema["properties"][k]));
                }
                Value::Object(m)
            }
            Some("array") => json!([sample(&schema["items"])]),
            Some("boolean") => json!(true),
            Some("integer") => json!(1),
            Some("number") => json!(1.0),
            _ => json!("x"),
        }
    }

    /// Whatever a shipped workflow says, it loads and runs to a result on answers its own schemas
    /// accept: no test depends on a workflow's wording, so changing one changes no test.
    #[test]
    fn every_shipped_workflow_loads_and_runs_to_a_result() {
        for (name, text) in BUILTIN {
            let wf = load(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            let inputs: Map<String, Value> =
                wf.inputs.iter().map(|i| (i.clone(), json!("x"))).collect();
            let mut next = start(&wf, name, &inputs, Path::new("/r"), u64::MAX);
            let run = loop {
                match next {
                    Next::Step(p) => {
                        let (_, step) = step_at(&wf, p.index).unwrap();
                        let a = format!("```json\n{}\n```", sample(&step.output_schema));
                        let v = answer(step, &a).unwrap_or_else(|e| panic!("{name}: {e}"));
                        next = accept(&wf, *p, v, Done::default());
                    }
                    Next::Done(run) => break run,
                }
            };
            assert_eq!(run.error, None, "{name}");
            assert!(run.result.is_some_and(|r| !r.is_empty()), "{name}");
        }
    }

    /// The built-in spec-answer's result: no "Open" heading when the verify step settled every
    /// conflict the solve step found; the heading with the conflict when it did not.
    #[test]
    fn spec_answer_shows_open_items_only_when_there_are_some() {
        let wf = load(BUILTIN[0].1).unwrap();
        let solve = json!({
            "coverage": [], "results": [], "must_hold": ["mean([]) returns 0.0"],
            "must_stay_unchanged": [],
            "conflicts": [{"case": "The unused helper the request asks for", "requirements": "r",
                "resolutions": [{"resolution": "keep it", "gives": "kept"}]}],
        });
        let verdict = |v: &str| {
            json!({"verdicts": [{"item": "Conflict: The unused helper the request asks for",
            "request_words": "add a function", "check": "the request asks for it", "verdict": v, "value": "keep it"}]})
        };
        let (_, settled) = drive(&wf, vec![solve.clone(), verdict("confirmed")]);
        let text = settled.result.unwrap();
        assert!(!text.contains("Open, to settle"), "{text}");
        assert!(
            text.contains("Settled from the request and the project"),
            "{text}"
        );
        let (_, open) = drive(&wf, vec![solve, verdict("undecided")]);
        let text = open.result.unwrap();
        assert!(text.contains("Open, to settle"), "{text}");
        assert!(
            text.contains("- Conflict: The unused helper the request asks for"),
            "{text}"
        );
    }
}
