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
    fn call(
        &self,
        name: &str,
        step: &Step,
        prompt: &str,
        cwd: &Path,
        cap: Duration,
    ) -> Result<Value, String>;
}

/// The settings of a step's session: no hooks; with Bash, Claude Code's sandbox, required
/// (`failIfUnavailable`, no unsandboxed retry), with `repo` not writable (scripts go in
/// `$TMPDIR`) and no network. Inside a container the sandbox is the nested, weaker one, the
/// container being the outer isolation.
fn settings(step: &Step, repo: &Path, in_container: bool) -> Value {
    let mut s = json!({ "disableAllHooks": true });
    if step.tools.iter().any(|t| t == "Bash") {
        s["sandbox"] = json!({
            "enabled": true,
            "failIfUnavailable": true,
            "autoAllowBashIfSandboxed": true,
            "allowUnsandboxedCommands": false,
            "enableWeakerNestedSandbox": in_container,
            "filesystem": { "denyWrite": [repo.display().to_string()] },
            "network": { "allowedDomains": [], "strictAllowlist": true },
        });
    }
    s
}

/// Running inside a container (Docker, Podman).
fn in_container() -> bool {
    Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists()
}

/// `claude -p` as Forge's own agents run it: no hooks, no MCP, no settings but [`settings`], the
/// reviewer's environment (`FORGE_REVIEWER=1`: Forge's hooks stay out), structured output.
pub struct Claude {
    pub exe: PathBuf,
    pub model: Option<String>,
}

impl Model for Claude {
    fn call(
        &self,
        _name: &str,
        step: &Step,
        prompt: &str,
        cwd: &Path,
        cap: Duration,
    ) -> Result<Value, String> {
        let schema = step.output_schema.to_string();
        let tools = step.tools.join(",");
        let settings = settings(step, cwd, in_container()).to_string();
        let mut cmd = Command::new(&self.exe);
        cmd.current_dir(cwd)
            .env("FORGE_REVIEWER", "1")
            .args([
                "-p",
                prompt,
                "--output-format",
                "json",
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
        let out = proc::run(&mut cmd, None, cap).ok_or("claude could not run or was stopped")?;
        if out.timed_out {
            return Err(format!("claude took longer than {}s", cap.as_secs()));
        }
        if !out.ok {
            return Err(failure(&out.stdout));
        }
        let v: Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| format!("claude's output is not JSON: {e}"))?;
        match &v["structured_output"] {
            Value::Null => Err("claude gave no structured answer".into()),
            so => Ok(so.clone()),
        }
    }
}

/// Why a `claude -p` that exited with an error failed, from its JSON result: the result's subtype
/// (`error_max_turns`, …), the API error, and its message, cut short.
fn failure(stdout: &[u8]) -> String {
    let Ok(v) = serde_json::from_slice::<Value>(stdout) else {
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
        let (prompt, output) = if skip {
            (None, Value::Null)
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
            let result = model.call(name, step, &prompt, repo, left).and_then(|v| {
                let validator =
                    jsonschema::validator_for(&step.output_schema).map_err(|e| e.to_string())?;
                let refused = validator
                    .iter_errors(&v)
                    .next()
                    .map(|e| format!("output refused by its schema: {}: {e}", e.instance_path()));
                refused.map_or(Ok(v), Err)
            });
            match result {
                Ok(v) => (Some(prompt), v),
                Err(e) => {
                    out.steps.push(StepRun {
                        step: name.clone(),
                        skipped: false,
                        prompt: Some(prompt),
                        output: Value::Null,
                        seconds: t.elapsed().as_secs_f64(),
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
        });
    }
    match render(&wf.result, &ctx) {
        Ok(r) => out.result = Some(r.trim().to_string()),
        Err(e) => out.error = Some(format!("result: {e}")),
    }
    out
}

/// The workflows Forge ships.
pub const BUILTIN: [(&str, &str); 1] =
    [("spec-answer", include_str!("../workflows/spec-answer.yaml"))];

/// `forge internal workflow run <file|builtin name> --repo DIR [--input NAME=FILE]... [--cap SECS]
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
    let text = match BUILTIN.iter().find(|(n, _)| *n == which) {
        Some((_, t)) => (*t).to_string(),
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
    fn call(
        &self,
        name: &str,
        step: &Step,
        prompt: &str,
        cwd: &Path,
        cap: Duration,
    ) -> Result<Value, String> {
        match self.outputs.get(name) {
            Some(v) => Ok(v.clone()),
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
        fn call(
            &self,
            _: &str,
            _: &Step,
            prompt: &str,
            _: &Path,
            _: Duration,
        ) -> Result<Value, String> {
            self.prompts.borrow_mut().push(prompt.to_string());
            Ok(self.outputs.borrow_mut().remove(0))
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

    fn writer_output() -> Value {
        json!({
            "coverage": [],
            "conflicts": [{"rows": [3], "case": "Leg 2: weight against fuel.", "requirements": [],
                           "resolutions": [{"resolution": "Cap fuel", "gives": "145.7 gal"},
                                           {"resolution": "Flag it", "gives": "route_feasible false"}]}],
            "must_hold": ["A", "B"],
            "must_stay_unchanged": ["\"data files are correct\""],
            "open_questions": [{"row": 1, "question": "Q1?", "why": ""}, {"row": 2, "question": "Q2?", "why": ""},
                               {"row": 3, "question": "Q3, part of the conflict?", "why": ""}],
        })
    }

    fn answers() -> Value {
        json!({"answers": [
            {"question": "Q1?", "evidence": "", "decided": true, "answer": "yes"},
            {"question": "Q2?", "evidence": "", "decided": false, "answer": ""},
        ]})
    }

    fn builtin(name: &str) -> Workflow {
        load(BUILTIN.iter().find(|(n, _)| *n == name).unwrap().1).unwrap()
    }

    #[test]
    fn spec_answer_hands_over_decisions_and_leaves_conflicts_open() {
        let m = fake(vec![writer_output(), answers()]);
        let run = run(
            &builtin("spec-answer"),
            &inputs(),
            Path::new("/r"),
            &m,
            Duration::from_secs(60),
        );
        let text = run.result.unwrap();
        assert!(
            m.prompts.borrow()[1].contains("- Q1?\n- Q2?\n"),
            "{}",
            m.prompts.borrow()[1]
        );
        assert!(
            !m.prompts.borrow()[1].contains("Q3"),
            "a question on a row a conflict involves skips the answerer"
        );
        assert!(
            text.contains("- Q3, part of the conflict?\n"),
            "and is handed over open, never dropped: {text}"
        );
        assert!(
            !m.prompts.borrow()[1].contains("S1."),
            "the answerer never sees the statements"
        );
        for part in [
            "S1. A\nS2. B",
            "U1. \"data files are correct\"",
            "- Q1? -> yes",
            "- Q2?\n",
            "- Leg 2: weight against fuel.",
            "(1) Cap fuel -> 145.7 gal",
            "(2) Flag it",
        ] {
            assert!(text.contains(part), "{part:?} not in:\n{text}");
        }
    }

    #[test]
    fn bash_runs_in_a_required_sandbox_with_the_repository_read_only() {
        let wf = builtin("spec-answer");
        let s = settings(&wf.steps["writer"], Path::new("/repo"), true);
        assert_eq!(s["disableAllHooks"], true);
        let sb = &s["sandbox"];
        assert_eq!(sb["enabled"], true);
        assert_eq!(sb["failIfUnavailable"], true);
        assert_eq!(sb["allowUnsandboxedCommands"], false);
        assert_eq!(sb["enableWeakerNestedSandbox"], true);
        assert_eq!(sb["filesystem"]["denyWrite"], json!(["/repo"]));
        assert_eq!(sb["network"]["strictAllowlist"], true);
        let mut read_only = wf.steps["writer"].clone();
        read_only.tools = vec!["Read".into()];
        assert_eq!(
            settings(&read_only, Path::new("/repo"), false),
            json!({"disableAllHooks": true})
        );
    }

    #[test]
    fn a_failed_claude_says_why() {
        let out = br#"{"type":"result","subtype":"error_max_turns","is_error":true,"result":""}"#;
        assert_eq!(failure(out), "claude failed: error_max_turns");
        let out = br#"{"subtype":"success","is_error":true,"api_error_status":529,"api_error":"overloaded_error","result":"Overloaded"}"#;
        assert_eq!(
            failure(out),
            "claude failed: overloaded_error, 529, Overloaded"
        );
        assert_eq!(failure(b"not json"), "claude failed");
    }

    #[test]
    fn the_builtin_workflows_load() {
        for (name, text) in BUILTIN {
            load(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
