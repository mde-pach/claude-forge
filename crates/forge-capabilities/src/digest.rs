//! The digest of the task (§20.4): what the capability engine reads about it. Built from the
//! hook input's prompt and the session's transcript, it holds the request, the agent's last answer
//! and what the agent did in the previous turn, never an argument, a file's content or a tool's
//! output.
//!
//! The transcript is Claude Code's session file (JSON lines). Only its `user` and `assistant`
//! records are read: a `user` record whose content is text is a prompt; an `assistant` record's
//! `text` blocks are the agent's message and its `tool_use` blocks its calls (repeated rows are
//! counted once, by id; a subagent's rows, `isSidechain`, are not the session's). When
//! UserPromptSubmit runs, the transcript does not hold the new prompt yet; on a session's first
//! prompt it does not exist.

use std::path::Path;

use forge_guard::parse::{self, Dir, Start};
use serde::Serialize;
use serde_json::Value;

/// The request's first characters sent.
pub const REQUEST_MAX: usize = 8000;
/// The previous answer's first characters sent.
pub const ANSWER_MAX: usize = 2000;
/// The newest tool calls of the previous turn sent.
pub const ACTIONS_MAX: usize = 30;
/// Commands of one Bash call named in its target.
const COMMANDS_MAX: usize = 5;

/// One tool call: the tool, and what it acted on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Action {
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// What the engine reads about the task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Digest {
    pub request: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_answer: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recent_actions: Vec<Action>,
}

fn first(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Where the paths of a digest are shown from: the repository (relative paths) and the home
/// directory (`~/…`).
#[derive(Debug, Clone, Copy)]
pub struct Places<'a> {
    pub root: &'a Path,
    pub home: Option<&'a Path>,
}

impl Places<'_> {
    /// A path as the digest shows it: relative to the repository inside it, `~/…` under the home
    /// directory, else as written.
    fn show(&self, path: &str) -> String {
        let p = Path::new(path);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        };
        if let Ok(rel) = p.strip_prefix(self.root) {
            let r = rel.to_string_lossy();
            return if r.is_empty() {
                ".".into()
            } else {
                r.into_owned()
            };
        }
        if let Some(rel) = self.home.and_then(|h| p.strip_prefix(h).ok()) {
            return format!("~/{}", rel.to_string_lossy());
        }
        p.to_string_lossy().into_owned()
    }
}

/// Shell words that only move or set up the shell: not what the call did.
const SHELL_SETUP: &[&str] = &[
    "cd", "pushd", "popd", "export", "set", "true", "source", ".",
];

/// A plain word that can be a subcommand: lowercase letters, digits and `-`, unquoted and known
/// before the command runs. A path, a file name, a flag or quoted text is never one.
fn subcommand(w: &parse::Word) -> Option<&str> {
    let t = w.text.as_str();
    let ok = !w.quoted
        && !w.dynamic
        && (1..=32).contains(&t.len())
        && t.starts_with(|c: char| c.is_ascii_lowercase())
        && t.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    ok.then_some(t)
}

/// A Bash call's target: each command it runs, as the guard parses it, by its program and
/// subcommand (`git commit; uv run pytest` reads `git commit; pytest`).
fn bash_target(command: &str, places: &Places) -> Option<String> {
    let parsed = parse::parse(
        command,
        &Start {
            cwd: Dir::Known(places.root.to_path_buf()),
            home: places.home.map(Path::to_path_buf),
        },
    );
    let mut out: Vec<String> = Vec::new();
    for c in parsed.commands.iter().filter(|c| !c.nested) {
        let Some(&start) = c.starts.last() else {
            continue;
        };
        let name = c.name_at(start);
        if SHELL_SETUP.contains(&name) || name.is_empty() {
            continue;
        }
        let mut one = name.to_string();
        if let Some(sub) = c.words.get(start + 1).and_then(subcommand) {
            one.push(' ');
            one.push_str(sub);
        }
        if !out.contains(&one) {
            out.push(one);
        }
        if out.len() == COMMANDS_MAX {
            break;
        }
    }
    (!out.is_empty()).then(|| out.join("; "))
}

/// What a tool call acted on: its file, or for Bash its commands. Nothing else of its input.
fn target(tool: &str, input: &Value, places: &Places) -> Option<String> {
    let s = |k: &str| input.get(k).and_then(Value::as_str);
    if tool == "Bash" {
        return s("command").and_then(|c| bash_target(c, places));
    }
    s("file_path")
        .or_else(|| s("notebook_path"))
        .or_else(|| s("relative_path"))
        .or_else(|| s("path"))
        .map(|p| places.show(p))
}

/// The text of a `user` record that is a prompt (not a tool result, not Claude Code's own
/// messages).
fn prompt_text(msg: &Value) -> Option<String> {
    let text = match msg.get("content")? {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            if blocks
                .iter()
                .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            {
                return None;
            }
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => return None,
    };
    let t = text.trim_start();
    // Claude Code's own messages, recorded as the user's
    let own = [
        "<local-command",
        "<command-name>",
        "<command-message>",
        "<task-notification>",
        "<agent-message",
        "<system-reminder>",
    ];
    (!t.is_empty() && !own.iter().any(|o| t.starts_with(o))).then_some(text)
}

/// The previous turn, as the transcript shows it: the agent's last message and its calls since
/// the last prompt.
fn previous_turn(transcript: &str, places: &Places) -> (Option<String>, Vec<Action>) {
    let mut answer: Option<String> = None;
    let mut actions: Vec<Action> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for line in transcript.lines() {
        let Ok(rec) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if rec.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || rec.get("isMeta").and_then(Value::as_bool) == Some(true)
            || rec.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let Some(msg) = rec.get("message") else {
            continue;
        };
        match rec.get("type").and_then(Value::as_str) {
            Some("user") => {
                if prompt_text(msg).is_some() {
                    // a new turn: what came before it is not the previous turn's
                    answer = None;
                    actions.clear();
                }
            }
            Some("assistant") => {
                let blocks = msg.get("content").and_then(Value::as_array);
                let mut text = Vec::new();
                for b in blocks.into_iter().flatten() {
                    match b.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(Value::as_str) {
                                text.push(t);
                            }
                        }
                        Some("tool_use") => {
                            let id = b.get("id").and_then(Value::as_str).unwrap_or("");
                            if !id.is_empty() && seen.iter().any(|s| s == id) {
                                continue;
                            }
                            seen.push(id.to_string());
                            let tool = b.get("name").and_then(Value::as_str).unwrap_or("");
                            if tool.is_empty() {
                                continue;
                            }
                            let input = b.get("input").cloned().unwrap_or(Value::Null);
                            actions.push(Action {
                                tool: tool.to_string(),
                                target: target(tool, &input, places),
                            });
                        }
                        _ => {}
                    }
                }
                let text = text.join("\n");
                if !text.trim().is_empty() {
                    answer = Some(text);
                }
            }
            _ => {}
        }
    }
    (answer, actions)
}

/// The digest of a prompt: the request alone on a first prompt (no transcript yet), else with
/// the agent's last answer and the previous turn's newest calls.
pub fn digest(prompt: &str, transcript: Option<&Path>, places: &Places) -> Digest {
    let text = transcript.and_then(|t| std::fs::read_to_string(t).ok());
    let (answer, mut actions) = text
        .as_deref()
        .map(|t| previous_turn(t, places))
        .unwrap_or_default();
    if actions.len() > ACTIONS_MAX {
        actions.drain(..actions.len() - ACTIONS_MAX);
    }
    Digest {
        request: first(prompt, REQUEST_MAX),
        previous_answer: answer.map(|a| first(&a, ANSWER_MAX)),
        recent_actions: actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/transcript.jsonl");

    fn places() -> Places<'static> {
        Places {
            root: Path::new("/work/repo"),
            home: Some(Path::new("/home/me")),
        }
    }

    // The previous turn: the agent's last message, and its calls since the last prompt (repeated
    // rows once, a subagent's left out), each by tool and target; never an argument or an output.
    #[test]
    fn the_digest_reads_the_previous_turn() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("t.jsonl");
        std::fs::write(&t, FIXTURE).unwrap();
        let d = digest("Now add a page for the cart.", Some(&t), &places());
        assert_eq!(d.request, "Now add a page for the cart.");
        assert_eq!(d.previous_answer.as_deref(), Some("The total is fixed."));
        let shown: Vec<String> = d
            .recent_actions
            .iter()
            .map(|a| format!("{} {}", a.tool, a.target.as_deref().unwrap_or("-")))
            .collect();
        assert_eq!(
            shown,
            [
                "Read app/cart.py",
                "Bash git status; pytest",
                "Edit app/cart.py",
                "Write ~/notes/todo.md",
                "Bash grep",
                "WebFetch -",
            ]
        );
        let json = serde_json::to_string(&d).unwrap();
        for secret in [
            "SECRET",
            "-k",
            "total ==",
            "https://",
            "/work/repo",
            "/home/me",
            "first turn",
        ] {
            assert!(!json.contains(secret), "{secret} in {json}");
        }
    }

    // A first prompt has no transcript: the request alone. Long texts are cut; the newest 30
    // calls are kept.
    #[test]
    fn a_first_prompt_and_the_limits() {
        let d = digest(&"x".repeat(9000), None, &places());
        assert_eq!(d.request.len(), REQUEST_MAX);
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            serde_json::json!({"request": "x".repeat(REQUEST_MAX)})
        );
        let mut lines =
            vec![r#"{"type":"user","message":{"role":"user","content":"go"}}"#.to_string()];
        for i in 0..40 {
            lines.push(format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t{i}","name":"Read","input":{{"file_path":"/work/repo/f{i}.py"}}}}]}}}}"#
            ));
        }
        lines.push(format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}}}}"#,
            "y".repeat(3000)
        ));
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("t.jsonl");
        std::fs::write(&t, lines.join("\n")).unwrap();
        let d = digest("next", Some(&t), &places());
        assert_eq!(d.recent_actions.len(), ACTIONS_MAX);
        assert_eq!(d.recent_actions[0].target.as_deref(), Some("f10.py"));
        assert_eq!(d.previous_answer.unwrap().len(), ANSWER_MAX);
    }
}
