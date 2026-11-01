//! The spec step (§8.5): on the session's first prompt, an independent agent (a fresh, read-only
//! `claude -p`, as the reviewer) turns the request into statements the code and the tests are
//! built from: what must hold, what must stay unchanged, and the open questions, each with a
//! default reading. File edits wait until the statements are validated through AskUserQuestion,
//! which the person answers, or Forge's reviewer when nobody is there; after each answer the
//! independent agent updates the statements. Everything lives in the session's state; nothing is
//! written to the repository.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use forge_core::{Note, NoteKind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::input::HookInput;
use crate::{Output, Project, Runtime, pre_decision, proc, review, state};

/// The longest the first prompt waits for the statements (the hook's timeout is longer).
const TRANSFORM_CAP: Duration = Duration::from_secs(840);
/// The longest an answer or an update takes (their hooks' timeouts are longer).
const FOLLOW_CAP: Duration = Duration::from_secs(540);

const TRANSFORMER: &str = "You turn a software request into a precise specification that a separate coding agent will use to write both the code and the tests. You do not write code.

You can read the project (Read, Grep, Glob). Read what the request depends on: the code it touches, and every input the code works from (data files, configuration, parameters, existing helpers). Ground each statement in what you find.

Return three lists:
- must_hold: observable, testable behaviours of the result.
- must_stay_unchanged: existing behaviour the change must not break.
- open_questions: the ambiguities in the request, each with the reading you would choose (default) and one line on why.

Rules:
- Each statement can be checked from outside the code: an output, a value, a file, an error.
- Name concrete files, fields and functions where they matter.
- Inputs the code is given but does not use, and helpers nobody calls, are likely part of the request: state what they should do, or raise them as open questions.
- No design, no implementation steps, no restating the request in vaguer words.
- Short. Only what matters for correctness.";

const UPDATE: &str = "The specification below was validated with the user, who answered the questions shown. Rewrite the specification so it reflects the answers: same three lists; an open question that is answered becomes a statement; a statement an answer contradicts changes. Keep everything else as it is.";

const ANSWERER: &str = "An autonomous coding agent asks the user the questions below before it changes anything. Nobody is available, so you answer in the user's place, as a careful senior developer who knows the project would.

Be sceptical. Read the code and the data the questions are about before answering. For each question, choose the option the request and the project support best, not the easiest one or the first one; a default reading is a suggestion, not an answer. When the evidence points elsewhere, answer with your own short reading instead of an option. When the request does not settle it, choose what the user most likely meant and say what makes it uncertain. Answer every question, with its exact text; give one sentence of reason per answer.";

const SPEC_SCHEMA: &str = r#"{"type":"object","properties":{"must_hold":{"type":"array","items":{"type":"string"}},"must_stay_unchanged":{"type":"array","items":{"type":"string"}},"open_questions":{"type":"array","items":{"type":"object","properties":{"question":{"type":"string"},"default":{"type":"string"},"why":{"type":"string"}},"required":["question","default","why"],"additionalProperties":false}}},"required":["must_hold","must_stay_unchanged","open_questions"],"additionalProperties":false}"#;

const ANSWERS_SCHEMA: &str = r#"{"type":"object","properties":{"answers":{"type":"array","items":{"type":"object","properties":{"question":{"type":"string"},"answer":{"type":"string"},"reason":{"type":"string"}},"required":["question","answer","reason"],"additionalProperties":false}}},"required":["answers"],"additionalProperties":false}"#;

/// What the agent is told with the statements, in every session (a reviewer answers when nobody
/// is there).
const INSTRUCTIONS: &str = "An independent reading of this request produced the statements below. Before changing any file, validate them with the user through AskUserQuestion (at most 4 questions per call; use another call for the rest): first \"Do these statements match what you want?\" (options \"Approve\" and \"Needs changes\"), then one question per open question, with its default reading as the first option. File edits are refused until a question is answered. Then build both the code and the tests from the validated statements; each test names the statements it covers (e.g. S2).";

/// The refusal of a file edit before validation.
pub const GATE_REASON: &str =
    "Validate the statements with the user first (AskUserQuestion), then edit.";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    pub default: String,
    pub why: String,
}

/// The statements of a session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    pub must_hold: Vec<String>,
    pub must_stay_unchanged: Vec<String>,
    pub open_questions: Vec<Question>,
}

impl Spec {
    /// As the agent reads it: `## Must hold` / `S1. …`, `## Must stay unchanged` / `U1. …`,
    /// `## Open questions` / `Q1. … Default reading: …. Why: ….`
    pub fn render(&self) -> String {
        let mut out = String::from("## Must hold\n");
        for (i, s) in self.must_hold.iter().enumerate() {
            out.push_str(&format!("S{}. {}\n", i + 1, s.trim()));
        }
        out.push_str("## Must stay unchanged\n");
        for (i, s) in self.must_stay_unchanged.iter().enumerate() {
            out.push_str(&format!("U{}. {}\n", i + 1, s.trim()));
        }
        if !self.open_questions.is_empty() {
            out.push_str("## Open questions\n");
            for (i, q) in self.open_questions.iter().enumerate() {
                out.push_str(&format!(
                    "Q{}. {} Default reading: {} Why: {}\n",
                    i + 1,
                    q.question.trim(),
                    q.default.trim(),
                    q.why.trim()
                ));
            }
        }
        out
    }

    fn from_value(v: &Value) -> Option<Spec> {
        let spec: Spec = serde_json::from_value(v.clone()).ok()?;
        (!spec.must_hold.is_empty()).then_some(spec)
    }
}

fn spec_file(p: &Project) -> std::path::PathBuf {
    p.session_dir.join("spec.json")
}

/// The session's statements, when the spec step produced them.
pub fn load(p: &Project) -> Option<Spec> {
    serde_json::from_slice(&std::fs::read(spec_file(p)).ok()?).ok()
}

fn validated(p: &Project) -> bool {
    p.session_dir.join("spec-validated").exists()
}

/// A fresh, read-only `claude -p` (the reviewer's setup) with `schema`: its structured output.
fn run_agent(
    rt: &Runtime,
    prompt: &str,
    schema: &str,
    cwd: &Path,
    cap: Duration,
    turns: u32,
) -> Result<Value, String> {
    let mut cmd = Command::new(&rt.env.claude);
    cmd.current_dir(cwd)
        .env("FORGE_REVIEWER", "1")
        .args([
            "-p",
            prompt,
            "--output-format",
            "json",
            "--json-schema",
            schema,
        ])
        .args([
            "--tools",
            "Read,Grep,Glob",
            "--allowedTools",
            "Read,Grep,Glob",
        ])
        .args([
            "--permission-mode",
            "dontAsk",
            "--max-turns",
            &turns.to_string(),
        ])
        .args(["--no-session-persistence", "--strict-mcp-config"])
        .args([
            "--setting-sources",
            "",
            "--settings",
            r#"{"disableAllHooks":true}"#,
        ]);
    if let Some(m) = &rt.env.review_model {
        cmd.args(["--model", m]);
    }
    let out = proc::run(&mut cmd, None, cap).ok_or("claude could not run or was stopped")?;
    if out.timed_out {
        return Err(format!("claude took longer than {}s", cap.as_secs()));
    }
    if !out.ok {
        return Err("claude failed".into());
    }
    let v: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("claude's output is not JSON: {e}"))?;
    match &v["structured_output"] {
        Value::Null => Err("claude gave no structured answer".into()),
        so => Ok(so.clone()),
    }
}

fn log(p: &Project, input: &HookInput, what: &str, detail: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    state::append_line(
        &p.state.join("spec.log"),
        &format!(
            "{ts}\t{}\t{what}\t{}",
            input.session_key(),
            detail.replace(['\t', '\n'], " ")
        ),
    );
}

/// UserPromptSubmit: on the session's first prompt, the statements, as the agent's context.
/// Never twice in a session, never inside Forge's own agents, nothing on failure but a note.
pub fn first_prompt(rt: &Runtime, input: &HookInput, p: &Project) -> Option<String> {
    if !rt.env.spec || rt.env.in_review {
        return None;
    }
    let done = p.session_dir.join("spec-done");
    if done.exists() {
        return None;
    }
    state::write_atomic(&done, b"");
    let request = input.prompt.as_deref()?.trim();
    if request.is_empty() {
        return None;
    }
    state::write_atomic(&p.session_dir.join("spec-request"), request.as_bytes());
    let cwd = rt.cwd(input);
    let prompt = format!(
        "{TRANSFORMER}\n\nRepository: {}\n\nREQUEST:\n<request>\n{request}\n</request>",
        cwd.display()
    );
    let started = std::time::Instant::now();
    let spec = run_agent(rt, &prompt, SPEC_SCHEMA, &cwd, TRANSFORM_CAP, 60)
        .and_then(|v| Spec::from_value(&v).ok_or_else(|| "the statements were empty".to_string()));
    match spec {
        Ok(spec) => {
            log(
                p,
                input,
                "statements",
                &format!(
                    "{}s, {} S, {} U, {} Q",
                    started.elapsed().as_secs(),
                    spec.must_hold.len(),
                    spec.must_stay_unchanged.len(),
                    spec.open_questions.len()
                ),
            );
            state::write_atomic(
                &spec_file(p),
                &serde_json::to_vec(&spec).unwrap_or_default(),
            );
            Some(format!("{INSTRUCTIONS}\n\n{}", spec.render()))
        }
        Err(why) => {
            log(p, input, "failed", &why);
            p.note(&[Note::new(
                NoteKind::ForgeFailure,
                format!("The request could not be turned into statements ({why}); the session went on without them."),
            )]);
            None
        }
    }
}

/// PreToolUse on a file edit: refused while the statements wait for validation.
pub fn gate(input: &HookInput, p: &Project) -> Option<Output> {
    if !matches!(
        input.tool(),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
    ) {
        return None;
    }
    if !spec_file(p).is_file() || validated(p) {
        return None;
    }
    Some(pre_decision("deny", GATE_REASON))
}

/// The questions of an AskUserQuestion input, by their text.
fn questions(input: &Value) -> Vec<&Value> {
    input["questions"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// PreToolUse on AskUserQuestion while the statements wait: nobody there, Forge's reviewer
/// answers, and Claude Code gets the question with its answers filled in.
pub fn answer(rt: &Runtime, input: &HookInput, p: &Project) -> Option<Output> {
    let spec = load(p)?;
    if !review::wanted(rt, input) {
        return None;
    }
    let qs = questions(&input.tool_input);
    if qs.is_empty() {
        return None;
    }
    let request = std::fs::read_to_string(p.session_dir.join("spec-request")).unwrap_or_default();
    let cwd = rt.cwd(input);
    let asked: Vec<Value> = qs
        .iter()
        .map(|q| {
            json!({
                "question": q["question"],
                "options": q["options"].as_array().map(|o| o.iter().map(|x| x["label"].clone()).collect::<Vec<_>>()).unwrap_or_default(),
            })
        })
        .collect();
    let prompt = format!(
        "{ANSWERER}\n\nRepository: {}\n\nThe user's request (the session's first prompt):\n<request>\n{}\n</request>\n\nWhat an independent reading of the request concluded:\n<statements>\n{}</statements>\n\nThe agent's questions, each with its options:\n{}",
        cwd.display(),
        request.trim(),
        spec.render(),
        serde_json::to_string_pretty(&asked).unwrap_or_default()
    );
    let v = match run_agent(rt, &prompt, ANSWERS_SCHEMA, &cwd, FOLLOW_CAP, 40) {
        Ok(v) => v,
        Err(why) => {
            log(p, input, "answer-failed", &why);
            return None;
        }
    };
    let given = v["answers"].as_array().cloned().unwrap_or_default();
    let mut answers = Map::new();
    for q in &qs {
        let Some(text) = q["question"].as_str() else {
            continue;
        };
        let a = given
            .iter()
            .find(|a| {
                a["question"]
                    .as_str()
                    .is_some_and(|t| t.trim() == text.trim())
            })
            .or_else(|| given.get(answers.len()));
        if let Some(a) = a.and_then(|a| a["answer"].as_str()) {
            answers.insert(text.to_string(), Value::String(a.to_string()));
        }
    }
    if answers.is_empty() {
        log(p, input, "answer-failed", "no answer matched the questions");
        return None;
    }
    let reasons: Vec<String> = given
        .iter()
        .map(|a| {
            format!(
                "{} -> {} ({})",
                a["question"].as_str().unwrap_or(""),
                a["answer"].as_str().unwrap_or(""),
                a["reason"].as_str().unwrap_or("")
            )
        })
        .collect();
    log(p, input, "answered", &reasons.join(" | "));
    let mut updated = input.tool_input.clone();
    updated["answers"] = Value::Object(answers);
    Some(Output::json(json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": "Answered by an independent review, nobody being there to answer.",
            "updatedInput": updated,
        }
    })))
}

/// The answers of an answered AskUserQuestion: question text → answer.
fn answers_of(input: &HookInput) -> Map<String, Value> {
    for v in [
        &input.tool_input["answers"],
        &input.tool_response["answers"],
    ] {
        if let Value::Object(m) = v
            && !m.is_empty()
        {
            return m.clone();
        }
    }
    Map::new()
}

/// PostToolUse on AskUserQuestion: the statements are validated; the independent agent updates
/// them with the answers, and the agent reads the new version.
pub fn after_answer(rt: &Runtime, input: &HookInput, p: &Project) -> Option<String> {
    let spec = load(p)?;
    state::write_atomic(&p.session_dir.join("spec-validated"), b"");
    let answers = answers_of(input);
    if answers.is_empty() {
        return None;
    }
    let qa: String = answers
        .iter()
        .map(|(q, a)| {
            format!(
                "- {q}\n  -> {}\n",
                a.as_str().map_or_else(|| a.to_string(), str::to_string)
            )
        })
        .collect();
    let cwd = rt.cwd(input);
    let prompt = format!(
        "{UPDATE}\n\nSPECIFICATION:\n{}\nQUESTIONS AND ANSWERS:\n{qa}",
        spec.render()
    );
    match run_agent(rt, &prompt, SPEC_SCHEMA, &cwd, FOLLOW_CAP, 20)
        .and_then(|v| Spec::from_value(&v).ok_or_else(|| "the statements were empty".to_string()))
    {
        Ok(new) => {
            log(
                p,
                input,
                "updated",
                &format!(
                    "{} S, {} U, {} Q",
                    new.must_hold.len(),
                    new.must_stay_unchanged.len(),
                    new.open_questions.len()
                ),
            );
            state::write_atomic(&spec_file(p), &serde_json::to_vec(&new).unwrap_or_default());
            Some(format!(
                "The statements were updated with the answers. Build the code and the tests from this version:\n\n{}",
                new.render()
            ))
        }
        Err(why) => {
            log(p, input, "update-failed", &why);
            None
        }
    }
}

/// SessionStart after a compaction: the statements again, so the agent keeps building from them.
pub fn after_compaction(input: &HookInput, p: &Project) -> Option<String> {
    if input.source.as_deref() != Some("compact") {
        return None;
    }
    let spec = load(p)?;
    let lead = if validated(p) {
        "The statements this work is built from (validated):"
    } else {
        INSTRUCTIONS
    };
    Some(format!("{lead}\n\n{}", spec.render()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_render_numbered() {
        let s = Spec {
            must_hold: vec!["mean([]) returns 0.0".into()],
            must_stay_unchanged: vec!["mean of a non-empty list".into()],
            open_questions: vec![Question {
                question: "Also for None?".into(),
                default: "no, only an empty list.".into(),
                why: "the request names an empty list.".into(),
            }],
        };
        assert_eq!(
            s.render(),
            "## Must hold\nS1. mean([]) returns 0.0\n## Must stay unchanged\nU1. mean of a non-empty list\n## Open questions\nQ1. Also for None? Default reading: no, only an empty list. Why: the request names an empty list.\n"
        );
        assert!(
            Spec::from_value(
                &json!({"must_hold": [], "must_stay_unchanged": [], "open_questions": []})
            )
            .is_none()
        );
    }
}
