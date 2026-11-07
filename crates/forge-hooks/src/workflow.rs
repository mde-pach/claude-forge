//! Workflows: a chain of read-only `claude -p` steps described in a YAML file, and the text the
//! chain ends with. The spec step is one workflow; the engine knows nothing of specs.
//!
//! A workflow file (strict YAML, as packs are) has an optional trigger `on` (when a session runs
//! it), `inputs` (names given by the caller), `steps`
//! (run in the file's order) and `result` (a template: the text the caller gets). Each step has a
//! `prompt` template, the `tools` it may use, `max_turns`, an `output_schema` (JSON Schema, which
//! `claude --json-schema` fills and the engine checks again) and an optional `if` (a Jinja
//! expression; false skips the step, whose `output` is then `none`).
//!
//! Templates are Jinja ([minijinja]) with strict undefined values. They see each input by its
//! name, `repo` (the directory the steps read) and `steps.<name>.output`. A template may only
//! name earlier steps: that is checked when the file is loaded, so a run never fails on a typo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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
    /// When a session runs it on its own; without one, only when asked
    /// (`forge internal workflow run`).
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

/// One `claude -p` call.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// A Jinja expression; when false the step is skipped and its output is `none`.
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Claude Code tools the step may use: Read, Grep, Glob, and Bash, which runs in Claude
    /// Code's sandbox with the repository read-only (see [`settings`]). Empty: none.
    #[serde(default)]
    pub tools: Vec<String>,
    pub max_turns: u32,
    /// The JSON Schema the step's output follows.
    pub output_schema: Value,
    /// A Jinja template: what the step is asked.
    pub prompt: String,
}

/// The names templates may use besides the inputs.
const BUILTIN_NAMES: [&str; 2] = ["repo", "steps"];
/// Jinja's own global functions, which templates may call.
const JINJA_GLOBALS: [&str; 4] = ["namespace", "range", "dict", "debug"];
const TOOLS: [&str; 4] = ["Read", "Grep", "Glob", "Bash"];

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
        if let Some(t) = step.tools.iter().find(|t| !TOOLS.contains(&t.as_str())) {
            return Err(format!(
                "step `{name}`: tool `{t}` is not one of {}",
                TOOLS.join(", ")
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

/// How a step's `claude -p` call is made (a fake one in tests).
pub trait Model {
    fn call(&self, name: &str, step: &Step, prompt: &str, cwd: &Path, cap: Duration) -> Called;
}

/// What a step's call gave: its output, and what the call did when one was made ([`meta`]).
pub struct Called {
    pub output: Result<Value, String>,
    pub meta: Option<Value>,
}

impl From<Result<Value, String>> for Called {
    fn from(output: Result<Value, String>) -> Self {
        Self { output, meta: None }
    }
}

/// The settings of a step's session: no hooks; with Bash, Claude Code's sandbox, required
/// (`failIfUnavailable`, no unsandboxed retry), with no network and `deny` not writable: what
/// [`PutBack`] cannot put back, or the whole of `repo` when there is no [`PutBack`] (scripts go
/// in `$TMPDIR`, the step's own: [`StepTmp`]). Inside a container the sandbox is the nested,
/// weaker one, the container being the outer isolation.
fn settings(step: &Step, repo: &Path, deny: Option<&[PathBuf]>, in_container: bool) -> Value {
    let mut s = json!({ "disableAllHooks": true });
    if step.tools.iter().any(|t| t == "Bash") {
        s["sandbox"] = json!({
            "enabled": true,
            "failIfUnavailable": true,
            "autoAllowBashIfSandboxed": true,
            "allowUnsandboxedCommands": false,
            "enableWeakerNestedSandbox": in_container,
            "filesystem": { "denyWrite": match deny {
                Some(paths) => paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                None => vec![repo.display().to_string()],
            } },
            "network": { "allowedDomains": [], "strictAllowlist": true },
        });
    }
    s
}

/// How long one snapshot of the repository may take.
const SNAPSHOT_CAP: Duration = Duration::from_secs(60);

/// The repository around a step with Bash: writable while the step runs, so the project's own
/// programs work as they would for the agent (one that logs what it does, a database that keeps
/// a journal), then put back as it was. Before the step, the working tree is a git tree in a
/// private object store ([`BashSnapshots`]: the user's index and objects are never touched), and
/// its directories are listed; after it, the files the step added are removed (ignored ones
/// too), the ones it changed or deleted restored, and the directories it made removed. What the
/// tree does not hold stays not writable (`deny`): `.git`, Claude's worktrees, the ignored
/// paths that were there.
struct PutBack {
    root: PathBuf,
    snaps: BashSnapshots,
    before: String,
    ignored: Vec<String>,
    dirs: BTreeSet<PathBuf>,
    deny: Vec<PathBuf>,
    _dir: StepTmp,
}

impl PutBack {
    /// None outside a git repository, or when the snapshot fails: the step then runs with the
    /// repository not writable.
    fn take(repo: &Path) -> Option<Self> {
        let out = proc::git(
            repo,
            &["rev-parse", "--show-toplevel", "--absolute-git-dir"],
            proc::GIT_CAP,
        )?;
        let out = String::from_utf8_lossy(&out).into_owned();
        let mut lines = out.lines();
        let (root, git_dir) = (PathBuf::from(lines.next()?), PathBuf::from(lines.next()?));
        let dir = StepTmp::new().ok()?;
        let snaps = BashSnapshots::new(&root, &dir.0, &dir.0);
        let before = snaps.tree("before", true, SNAPSHOT_CAP)?;
        let ignored = ignored(&root, &[], true)?;
        let mut deny = vec![
            git_dir,
            root.join(".git"),
            root.join(".claude/worktrees"),
            dir.0.clone(),
        ];
        deny.extend(ignored.iter().map(|p| root.join(p)));
        let mut dirs = BTreeSet::new();
        walk_dirs(&root, &deny, &mut dirs);
        Some(Self {
            root,
            snaps,
            before,
            ignored,
            dirs,
            deny,
            _dir: dir,
        })
    }

    /// The repository as it was before the step; an error naming what could not be put back.
    fn put_back(&self) -> Result<(), String> {
        let not = |what: &str| {
            format!("the step's changes to the repository could not be undone: {what}")
        };
        let after = self
            .snaps
            .tree("after", true, SNAPSHOT_CAP)
            .ok_or_else(|| not("no snapshot after the step"))?;
        let mut ok = self.snaps.put_back(&self.before, &after, SNAPSHOT_CAP);
        // ignored files the step wrote, outside the ignored paths it could not write
        for path in ignored(&self.root, &self.ignored, false).unwrap_or_default() {
            ok &= std::fs::remove_file(self.root.join(path)).is_ok();
        }
        let mut now = BTreeSet::new();
        walk_dirs(&self.root, &self.deny, &mut now);
        // a directory the step made holds only what it wrote (a parent comes first and takes
        // the ones it holds with it)
        for made in now.difference(&self.dirs) {
            if made.exists() {
                ok &= std::fs::remove_dir_all(made).is_ok();
            }
        }
        let check = self
            .snaps
            .tree("check", true, SNAPSHOT_CAP)
            .ok_or_else(|| not("no snapshot after putting it back"))?;
        let left: Vec<String> = self
            .snaps
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

/// Running inside a container (Docker, Podman).
fn in_container() -> bool {
    Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists()
}

/// A step's own temp directory, given to its session as `CLAUDE_CODE_TMPDIR`: the sandbox's
/// writable `$TMPDIR` is then inside it, not the per-user one other Claude sessions share. Under
/// `/tmp` and short, since Claude Code falls back to the shared one for a long path; private to
/// the user; removed when it is dropped.
struct StepTmp(PathBuf);

impl StepTmp {
    fn new() -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let base = Path::new("/tmp");
        let base = if base.is_dir() {
            base.to_path_buf()
        } else {
            std::env::temp_dir()
        };
        // pid and a counter: unique among this process's steps, which may run at the same instant;
        // one left by an earlier process of the same pid is skipped
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        loop {
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = base.join(format!("fw-{}-{n}", std::process::id()));
            match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => return Ok(Self(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => {}
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for StepTmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `claude -p` as Forge's own agents run it: no hooks, no MCP, no settings but [`settings`], the
/// reviewer's environment (`FORGE_REVIEWER=1`: Forge's hooks stay out), structured output.
pub struct Claude {
    pub exe: PathBuf,
    pub model: Option<String>,
}

impl Model for Claude {
    fn call(&self, _name: &str, step: &Step, prompt: &str, cwd: &Path, cap: Duration) -> Called {
        match self.run(step, prompt, cwd, cap) {
            Ok(called) => called,
            Err(e) => Err(e).into(),
        }
    }
}

impl Claude {
    fn run(&self, step: &Step, prompt: &str, cwd: &Path, cap: Duration) -> Result<Called, String> {
        let schema = step.output_schema.to_string();
        let tools = step.tools.join(",");
        let put_back = step
            .tools
            .iter()
            .any(|t| t == "Bash")
            .then(|| PutBack::take(cwd))
            .flatten();
        let deny = put_back.as_ref().map(|p| p.deny.as_slice());
        let settings = settings(step, cwd, deny, in_container()).to_string();
        let tmp = StepTmp::new().map_err(|e| format!("no temp directory for the step: {e}"))?;
        let mut cmd = Command::new(&self.exe);
        cmd.current_dir(cwd)
            .env("FORGE_REVIEWER", "1")
            .env("CLAUDE_CODE_TMPDIR", &tmp.0)
            .args([
                "-p",
                prompt,
                "--output-format",
                "stream-json",
                "--verbose",
                "--json-schema",
                &schema,
            ])
            .args(["--tools", &tools, "--allowedTools", &tools])
            .args([
                "--permission-mode",
                "dontAsk",
                "--max-turns",
                &step.max_turns.to_string(),
            ])
            .args(["--no-session-persistence", "--strict-mcp-config"])
            .args(["--setting-sources", "", "--settings", &settings]);
        if let Some(m) = &self.model {
            cmd.args(["--model", m]);
        }
        let out = proc::run(&mut cmd, None, cap);
        let undone = put_back.map_or(Ok(()), |p| p.put_back());
        let out = out.ok_or("claude could not run or was stopped")?;
        let (result, meta) = meta(&out.stdout, out.timed_out);
        let output = if out.timed_out {
            Err(format!("claude took longer than {}s", cap.as_secs()))
        } else if !out.ok {
            Err(failure(result.as_ref()))
        } else {
            match result.as_ref().map(|r| &r["structured_output"]) {
                None => Err("claude gave no result".into()),
                Some(Value::Null) => Err("claude gave no structured answer".into()),
                Some(so) => Ok(so.clone()),
            }
        };
        let output = undone.and(output);
        Ok(Called {
            output,
            meta: Some(meta),
        })
    }
}

/// What a step's `claude -p` did, read from its stream (`--output-format stream-json`), with the
/// stream's `result` event when it got that far. Read as far as the stream goes, so a step killed
/// at its cap still says what it had done: `model`, `session_id`, `turns` (the result's, else the
/// model's messages), `api_calls` (the model's messages), `tool_calls` by tool, `last_tool`,
/// `tokens` (`input`, `output`, `cache_read`, `cache_write`) with `tokens_from` (`result`, or
/// `messages` when there is no result: each message's usage as streamed, its output tokens lower
/// than the final count), `cost_usd` and `api_seconds` (from the result only), and `end` (the
/// result's subtype, `killed at its cap` or `no result`).
pub fn meta(stream: &[u8], timed_out: bool) -> (Option<Value>, Value) {
    let mut init = Value::Null;
    let mut messages: Vec<(String, Value)> = Vec::new();
    let mut tools: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut last_tool = Value::Null;
    let mut result = None;
    for line in stream.split(|b| *b == b'\n') {
        let Ok(e) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        match e["type"].as_str() {
            Some("system") if e["subtype"] == "init" => init = e,
            Some("assistant") => {
                let m = &e["message"];
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
            Some("result") => result = Some(e),
            _ => {}
        }
    }
    let tokens = |u: &dyn Fn(&str) -> u64| {
        json!({
            "input": u("input_tokens"),
            "output": u("output_tokens"),
            "cache_read": u("cache_read_input_tokens"),
            "cache_write": u("cache_creation_input_tokens"),
        })
    };
    let r = result.clone().unwrap_or(Value::Null);
    let (tokens, from) = if r["usage"].is_object() {
        (tokens(&|k| r["usage"][k].as_u64().unwrap_or(0)), "result")
    } else {
        let sum = |k: &str| {
            messages
                .iter()
                .map(|(_, u)| u[k].as_u64().unwrap_or(0))
                .sum()
        };
        (tokens(&sum), "messages")
    };
    let end = if timed_out {
        json!("killed at its cap")
    } else if result.is_some() {
        r["subtype"].clone()
    } else {
        json!("no result")
    };
    let turns = match &r["num_turns"] {
        Value::Null => json!(messages.len()),
        n => n.clone(),
    };
    let mut m = Map::new();
    for (k, v) in [
        ("model", init["model"].clone()),
        ("session_id", init["session_id"].clone()),
        ("turns", turns),
        ("api_calls", json!(messages.len())),
        ("tool_calls", json!(tools)),
        ("last_tool", last_tool),
        ("tokens", tokens),
        ("tokens_from", json!(from)),
        ("cost_usd", r["total_cost_usd"].clone()),
        (
            "api_seconds",
            r["duration_api_ms"]
                .as_f64()
                .map_or(Value::Null, |ms| json!(ms / 1000.0)),
        ),
        ("end", end),
    ] {
        if !v.is_null() {
            m.insert(k.into(), v);
        }
    }
    (result, Value::Object(m))
}

/// Why a `claude -p` that exited with an error failed, from its result: the result's subtype
/// (`error_max_turns`, …), the API error, and its message, cut short.
fn failure(result: Option<&Value>) -> String {
    let Some(v) = result else {
        return "claude failed".into();
    };
    let mut why: Vec<String> = Vec::new();
    for key in [
        "subtype",
        "terminal_reason",
        "api_error",
        "api_error_status",
    ] {
        match &v[key] {
            Value::String(s) if !s.is_empty() && s != "success" => why.push(s.clone()),
            Value::Number(n) => why.push(n.to_string()),
            _ => {}
        }
    }
    if let Some(r) = v["result"].as_str().filter(|r| !r.is_empty()) {
        why.push(r.chars().take(200).collect());
    }
    if why.is_empty() {
        "claude failed".into()
    } else {
        format!("claude failed: {}", why.join(", "))
    }
}

/// One step of a run, for the log.
#[derive(Debug, Clone, Serialize)]
pub struct StepRun {
    pub step: String,
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub output: Value,
    pub seconds: f64,
    /// What the step's call did (turns, tokens, cost, …): [`meta`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A whole run: every step, then the result (absent when a step failed).
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub workflow: String,
    pub steps: Vec<StepRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Runs `wf` in `repo` with `inputs`, every step within what is left of `cap`. A failed step
/// (no answer, out of time, an output its schema refuses) ends the run without a result.
pub fn run(
    wf: &Workflow,
    inputs: &Map<String, Value>,
    repo: &Path,
    model: &dyn Model,
    cap: Duration,
) -> Run {
    let started = Instant::now();
    let mut out = Run {
        workflow: wf.id.clone(),
        steps: Vec::new(),
        result: None,
        error: None,
    };
    if let Some(missing) = wf.inputs.iter().find(|i| !inputs.contains_key(*i)) {
        out.error = Some(format!("input `{missing}` is missing"));
        return out;
    }
    let mut ctx = inputs.clone();
    ctx.insert("repo".into(), json!(repo.display().to_string()));
    ctx.insert("steps".into(), json!({}));
    let env = env();
    let render = |src: &str, ctx: &Map<String, Value>| -> Result<String, String> {
        env.template_from_str(src)
            .and_then(|t| t.render(ctx))
            .map_err(|e| e.to_string())
    };
    for (name, step) in &wf.steps {
        let t = Instant::now();
        let fail = |out: &mut Run, e: String| out.error = Some(format!("step `{name}`: {e}"));
        let skip = match &step.condition {
            Some(c) => match render(&condition_template(c), &ctx) {
                Ok(v) => v != "true",
                Err(e) => {
                    fail(&mut out, e);
                    return out;
                }
            },
            None => false,
        };
        let (prompt, output, meta) = if skip {
            (None, Value::Null, None)
        } else {
            let prompt = match render(&step.prompt, &ctx) {
                Ok(p) => p,
                Err(e) => {
                    fail(&mut out, e);
                    return out;
                }
            };
            let Some(left) = cap.checked_sub(started.elapsed()) else {
                fail(&mut out, "no time left".into());
                return out;
            };
            let called = model.call(name, step, &prompt, repo, left);
            let result = called.output.and_then(|v| {
                let validator =
                    jsonschema::validator_for(&step.output_schema).map_err(|e| e.to_string())?;
                let refused = validator
                    .iter_errors(&v)
                    .next()
                    .map(|e| format!("output refused by its schema: {}: {e}", e.instance_path()));
                refused.map_or(Ok(v), Err)
            });
            match result {
                Ok(v) => (Some(prompt), v, called.meta),
                Err(e) => {
                    out.steps.push(StepRun {
                        step: name.clone(),
                        skipped: false,
                        prompt: Some(prompt),
                        output: Value::Null,
                        seconds: t.elapsed().as_secs_f64(),
                        meta: called.meta,
                    });
                    fail(&mut out, e);
                    return out;
                }
            }
        };
        if let Some(Value::Object(steps)) = ctx.get_mut("steps") {
            steps.insert(name.clone(), json!({ "output": output }));
        }
        out.steps.push(StepRun {
            step: name.clone(),
            skipped: skip,
            prompt,
            output,
            seconds: t.elapsed().as_secs_f64(),
            meta,
        });
    }
    match render(&wf.result, &ctx) {
        Ok(r) => out.result = Some(r.trim().to_string()),
        Err(e) => out.error = Some(format!("result: {e}")),
    }
    out
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

fn dir_from_env() -> Option<PathBuf> {
    std::env::var_os(DIR_VAR)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// `forge internal workflow run <file|name> --repo DIR [--input NAME=FILE]... [--cap SECS]
/// [--reuse RUN.json]`: runs a workflow outside a session and prints the run as JSON. With
/// `--reuse`, a step that ran in that earlier run (same name) takes its output from there instead
/// of calling claude again. For trying workflows; the hooks run them in place.
pub fn cli(args: &[String]) -> i32 {
    match cli_run(args) {
        Ok(run) => {
            println!("{}", serde_json::to_string_pretty(&run).unwrap_or_default());
            i32::from(run.error.is_some())
        }
        Err(e) => {
            eprintln!("forge internal workflow: {e}");
            2
        }
    }
}

fn cli_run(args: &[String]) -> Result<Run, String> {
    let usage = "usage: workflow run <file|name> --repo DIR [--input NAME=FILE]... [--cap SECS] [--reuse RUN.json]";
    let mut it = args.iter();
    if it.next().map(String::as_str) != Some("run") {
        return Err(usage.into());
    }
    let which = it.next().ok_or(usage)?;
    let (mut repo, mut inputs, mut cap) = (None, Map::new(), Duration::from_secs(1740));
    let mut reused = Map::new();
    while let Some(a) = it.next() {
        let v = it.next().ok_or(usage)?;
        match a.as_str() {
            "--repo" => repo = Some(PathBuf::from(v)),
            "--input" => {
                let (k, file) = v.split_once('=').ok_or("--input takes NAME=FILE")?;
                let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
                inputs.insert(k.to_string(), json!(text.trim()));
            }
            "--cap" => cap = Duration::from_secs(v.parse().map_err(|_| "--cap takes seconds")?),
            "--reuse" => {
                let text = std::fs::read_to_string(v).map_err(|e| format!("{v}: {e}"))?;
                let earlier: Value =
                    serde_json::from_str(&text).map_err(|e| format!("{v}: {e}"))?;
                for s in earlier["steps"].as_array().into_iter().flatten() {
                    if let (Some(name), false) =
                        (s["step"].as_str(), s["skipped"].as_bool().unwrap_or(true))
                    {
                        reused.insert(name.to_string(), s["output"].clone());
                    }
                }
            }
            _ => return Err(usage.into()),
        }
    }
    let named = available(dir_from_env().as_deref())
        .into_iter()
        .find(|(n, _)| n == which)
        .map(|(_, t)| t);
    let text = match named {
        Some(t) => t?,
        None => std::fs::read_to_string(which).map_err(|e| format!("{which}: {e}"))?,
    };
    let wf = load(&text)?;
    let model = Reused {
        outputs: reused,
        model: Claude {
            exe: PathBuf::from("claude"),
            model: std::env::var("FORGE_REVIEW_MODEL")
                .ok()
                .filter(|m| !m.is_empty()),
        },
    };
    Ok(run(&wf, &inputs, &repo.ok_or(usage)?, &model, cap))
}

/// Outputs of an earlier run, by step name, before the real model.
struct Reused {
    outputs: Map<String, Value>,
    model: Claude,
}

impl Model for Reused {
    fn call(&self, name: &str, step: &Step, prompt: &str, cwd: &Path, cap: Duration) -> Called {
        match self.outputs.get(name) {
            Some(v) => Ok(v.clone()).into(),
            None => self.model.call(name, step, prompt, cwd, cap),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Answers each step with the next canned output, and records the prompts.
    struct Fake {
        outputs: RefCell<Vec<Value>>,
        prompts: RefCell<Vec<String>>,
    }

    impl Model for Fake {
        fn call(&self, _: &str, _: &Step, prompt: &str, _: &Path, _: Duration) -> Called {
            self.prompts.borrow_mut().push(prompt.to_string());
            Ok(self.outputs.borrow_mut().remove(0)).into()
        }
    }

    fn fake(outputs: Vec<Value>) -> Fake {
        Fake {
            outputs: RefCell::new(outputs),
            prompts: RefCell::new(Vec::new()),
        }
    }

    const TWO_STEPS: &str = r#"id: t
inputs: [request]
steps:
  ask:
    max_turns: 1
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

    #[test]
    fn steps_run_in_order_and_see_earlier_outputs() {
        let wf = load(TWO_STEPS).unwrap();
        let m = fake(vec![json!({"qs": ["q1", "q2"]}), json!({"a": "yes"})]);
        let run = run(&wf, &inputs(), Path::new("/r"), &m, Duration::from_secs(60));
        assert_eq!(run.error, None);
        assert_eq!(*m.prompts.borrow(), vec!["Request: fix it", "q1;q2;"]);
        assert_eq!(run.result.as_deref(), Some("2 yes"));
    }

    #[test]
    fn a_false_condition_skips_the_step() {
        let wf = load(TWO_STEPS).unwrap();
        let m = fake(vec![json!({"qs": []})]);
        let run = run(&wf, &inputs(), Path::new("/r"), &m, Duration::from_secs(60));
        assert!(run.steps[1].skipped);
        assert_eq!(m.prompts.borrow().len(), 1);
        assert_eq!(run.result.as_deref(), Some("0 none"));
    }

    #[test]
    fn an_output_its_schema_refuses_ends_the_run() {
        let wf = load(TWO_STEPS).unwrap();
        let m = fake(vec![json!({"other": 1})]);
        let run = run(&wf, &inputs(), Path::new("/r"), &m, Duration::from_secs(60));
        assert!(run.error.unwrap().contains("refused by its schema"));
        assert_eq!(run.result, None);
    }

    #[test]
    fn a_missing_input_ends_the_run() {
        let wf = load(TWO_STEPS).unwrap();
        let run = run(
            &wf,
            &Map::new(),
            Path::new("/r"),
            &fake(vec![]),
            Duration::from_secs(60),
        );
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
    fn an_unknown_field_or_tool_is_refused_at_load() {
        assert!(
            load(&TWO_STEPS.replace(
                "max_turns: 1\n    output_schema: {type: object, properties: {a",
                "max_turn: 1\n    output_schema: {type: object, properties: {a"
            ))
            .is_err()
        );
        assert!(
            load(&TWO_STEPS.replacen("max_turns: 1", "tools: [Write]\n    max_turns: 1", 1))
                .unwrap_err()
                .contains("Write")
        );
    }

    #[test]
    fn bash_runs_in_a_required_sandbox_with_what_cannot_be_put_back_read_only() {
        let step = Step {
            condition: None,
            tools: vec!["Read".into(), "Bash".into()],
            max_turns: 1,
            output_schema: json!({"type": "object"}),
            prompt: "p".into(),
        };
        let s = settings(&step, Path::new("/repo"), None, true);
        assert_eq!(s["disableAllHooks"], true);
        let sb = &s["sandbox"];
        assert_eq!(sb["enabled"], true);
        assert_eq!(sb["failIfUnavailable"], true);
        assert_eq!(sb["allowUnsandboxedCommands"], false);
        assert_eq!(sb["enableWeakerNestedSandbox"], true);
        assert_eq!(sb["filesystem"]["denyWrite"], json!(["/repo"]));
        assert_eq!(sb["network"]["strictAllowlist"], true);
        let deny = [PathBuf::from("/repo/.git"), PathBuf::from("/repo/cache")];
        let s = settings(&step, Path::new("/repo"), Some(&deny), false);
        assert_eq!(
            s["sandbox"]["filesystem"]["denyWrite"],
            json!(["/repo/.git", "/repo/cache"])
        );
        let read_only = Step {
            tools: vec!["Read".into()],
            ..step
        };
        assert_eq!(
            settings(&read_only, Path::new("/repo"), None, false),
            json!({"disableAllHooks": true})
        );
    }

    /// A step's stream as `claude -p --output-format stream-json --verbose` writes it: init, two
    /// model messages (the first streamed twice, as a message with several blocks is), a result.
    const STREAM: &str = r#"{"type":"system","subtype":"init","model":"claude-sonnet-x","session_id":"s1"}
{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Looking."}],"usage":{"input_tokens":2,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":3000}}}
{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}],"usage":{"input_tokens":2,"output_tokens":16,"cache_read_input_tokens":0,"cache_creation_input_tokens":3000}}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}
{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t2","name":"Read","input":{}}],"usage":{"input_tokens":2,"output_tokens":24,"cache_read_input_tokens":3000,"cache_creation_input_tokens":100}}}
{"type":"result","subtype":"success","num_turns":3,"total_cost_usd":0.0113,"duration_api_ms":2826,"usage":{"input_tokens":4,"output_tokens":174,"cache_read_input_tokens":3000,"cache_creation_input_tokens":3100},"structured_output":{"a":"x"}}
"#;

    #[test]
    fn a_step_records_its_model_turns_tools_tokens_and_cost() {
        let (result, m) = meta(STREAM.as_bytes(), false);
        assert_eq!(result.unwrap()["structured_output"], json!({"a": "x"}));
        assert_eq!(m["model"], "claude-sonnet-x");
        assert_eq!(m["session_id"], "s1");
        assert_eq!(m["turns"], 3);
        assert_eq!(m["api_calls"], 2);
        assert_eq!(m["tool_calls"], json!({"Bash": 1, "Read": 1}));
        assert_eq!(m["last_tool"], "Read");
        assert_eq!(
            m["tokens"],
            json!({"input": 4, "output": 174, "cache_read": 3000, "cache_write": 3100})
        );
        assert_eq!(m["tokens_from"], "result");
        assert_eq!(m["cost_usd"], 0.0113);
        assert_eq!(m["api_seconds"], 2.826);
        assert_eq!(m["end"], "success");
    }

    #[test]
    fn a_step_killed_at_its_cap_still_says_what_it_had_done() {
        // the stream cut before the result, mid-line
        let cut = STREAM.split("{\"type\":\"result\"").next().unwrap();
        let (result, m) = meta(format!("{cut}{{\"type\":\"assi").as_bytes(), true);
        assert!(result.is_none());
        assert_eq!(m["end"], "killed at its cap");
        assert_eq!(m["turns"], 2);
        assert_eq!(m["tool_calls"], json!({"Bash": 1, "Read": 1}));
        // each message counted once, at its last streamed usage
        assert_eq!(
            m["tokens"],
            json!({"input": 4, "output": 40, "cache_read": 3000, "cache_write": 3100})
        );
        assert_eq!(m["tokens_from"], "messages");
        assert!(m.get("cost_usd").is_none());
    }

    #[test]
    fn a_failed_step_keeps_what_its_call_did_in_the_run() {
        struct Killed;
        impl Model for Killed {
            fn call(&self, _: &str, _: &Step, _: &str, _: &Path, _: Duration) -> Called {
                Called {
                    output: Err("claude took longer than 5s".into()),
                    meta: Some(json!({"end": "killed at its cap", "turns": 7})),
                }
            }
        }
        let wf = load(TWO_STEPS).unwrap();
        let run = run(
            &wf,
            &inputs(),
            Path::new("/r"),
            &Killed,
            Duration::from_secs(60),
        );
        assert!(run.error.unwrap().contains("longer than 5s"));
        assert_eq!(run.steps[0].meta.as_ref().unwrap()["turns"], 7);
    }

    #[test]
    fn a_step_with_bash_writes_in_the_repository_and_leaves_it_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().canonicalize().unwrap();
        let sh = |script: &str| {
            let ok = Command::new("sh")
                .args(["-c", script])
                .current_dir(&repo)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .unwrap()
                .success();
            assert!(ok, "{script}");
        };
        sh(
            "git init -q && printf '*.log\\ncache/\\n' > .gitignore && echo a > a.txt && echo b > b.txt \
            && mkdir cache && echo c > cache/c && git add -A && git commit -qm base \
            && echo staged > a.txt && git add a.txt && echo mine > a.txt && echo u > untracked.txt \
            && mkdir output",
        );
        let index = std::fs::read(repo.join(".git/index")).unwrap();
        // the step: changes a tracked, an untracked and a deleted file, adds files, ignored files
        // (one in an empty directory that was there) and directories of its own, then answers
        let claude = repo
            .join("..")
            .join(format!("claude-{}", std::process::id()));
        std::fs::write(
            &claude,
            "#!/bin/sh\necho changed > a.txt; echo changed > untracked.txt; rm b.txt\n\
             mkdir -p out && echo log > out/audit.log; echo log > output/audit.log\n\
             echo new > new.txt; echo x > run.log\n\
             mkdir __pycache__; echo p > __pycache__/m.pyc\n\
             echo '{\"type\":\"result\",\"subtype\":\"success\",\"structured_output\":{}}'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        sh("printf '__pycache__/\\n' >> .git/info/exclude");
        let step = Step {
            condition: None,
            tools: vec!["Bash".into()],
            max_turns: 1,
            output_schema: json!({"type": "object"}),
            prompt: "p".into(),
        };
        let model = Claude {
            exe: claude.clone(),
            model: None,
        };
        let called = model.call("s", &step, "p", &repo, Duration::from_secs(30));
        let _ = std::fs::remove_file(&claude);
        assert_eq!(called.output.unwrap(), json!({}));
        let read = |p: &str| std::fs::read_to_string(repo.join(p)).ok();
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
    fn a_step_with_bash_cannot_write_what_could_not_be_put_back() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().canonicalize().unwrap();
        let ok = Command::new("sh")
            .args([
                "-c",
                "git init -q && printf 'cache/\\n' > .gitignore && mkdir cache && echo c > cache/c",
            ])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let p = PutBack::take(&repo).unwrap();
        for denied in [repo.join(".git"), repo.join("cache"), p._dir.0.clone()] {
            assert!(p.deny.contains(&denied), "{} not denied", denied.display());
        }
        assert!(!p.deny.contains(&repo));
        let outside = tempfile::tempdir().unwrap();
        assert!(PutBack::take(outside.path()).is_none());
    }

    #[test]
    fn a_step_writes_in_its_own_short_private_temp_directory_removed_after() {
        use std::os::unix::fs::PermissionsExt;
        let (a, b) = (StepTmp::new().unwrap(), StepTmp::new().unwrap());
        assert_ne!(a.0, b.0);
        assert!(
            a.0.starts_with("/tmp") && a.0.as_os_str().len() < 40,
            "{}",
            a.0.display()
        );
        let mode = std::fs::metadata(&a.0).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::write(a.0.join("script.py"), "x").unwrap();
        let dir = a.0.clone();
        drop(a);
        assert!(!dir.exists());
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
        // without a directory: the built-in ones
        let builtin: Vec<String> = available(None).into_iter().map(|(n, _)| n).collect();
        assert_eq!(builtin, BUILTIN.map(|(n, _)| n.to_string()));
    }

    #[test]
    fn a_failed_claude_says_why() {
        let why = |stream: &[u8]| failure(meta(stream, false).0.as_ref());
        let out = br#"{"type":"result","subtype":"error_max_turns","is_error":true,"result":""}"#;
        assert_eq!(why(out), "claude failed: error_max_turns");
        let out = br#"{"type":"result","subtype":"success","is_error":true,"api_error_status":529,"api_error":"overloaded_error","result":"Overloaded"}"#;
        assert_eq!(why(out), "claude failed: overloaded_error, 529, Overloaded");
        assert_eq!(why(b"not json"), "claude failed");
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

    /// Answers each step with a value its output schema accepts.
    struct FromSchema;

    impl Model for FromSchema {
        fn call(&self, _: &str, step: &Step, _: &str, _: &Path, _: Duration) -> Called {
            Ok(sample(&step.output_schema)).into()
        }
    }

    /// Whatever a shipped workflow says, it loads and runs to a result on outputs its own schemas
    /// accept: no test depends on a workflow's wording, so changing one changes no test.
    #[test]
    fn every_shipped_workflow_loads_and_runs_to_a_result() {
        for (name, text) in BUILTIN {
            let wf = load(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            let inputs: Map<String, Value> =
                wf.inputs.iter().map(|i| (i.clone(), json!("x"))).collect();
            let run = run(
                &wf,
                &inputs,
                Path::new("/r"),
                &FromSchema,
                Duration::from_secs(60),
            );
            assert_eq!(run.error, None, "{name}");
            assert!(run.result.is_some_and(|r| !r.is_empty()), "{name}");
        }
    }
}
