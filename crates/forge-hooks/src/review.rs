//! An independent reviewer for permission asks nobody can answer: in an autonomous run
//! (`claude -p`, the Agent SDK, CI) an ask is otherwise denied without anyone looking at it. A
//! separate, context-free `claude -p` (no conversation, read-only tools, no hooks) judges the
//! action alone and allows or denies it.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::input::HookInput;
use crate::{Output, REVIEW_CAP, Runtime, pre_decision, proc, state};

/// A rule's ask, for the reviewer: the rule's concern and the question to answer.
pub struct Ask<'a> {
    pub rule: &'a str,
    pub concern: &'a str,
    pub question: Option<&'a str>,
    /// The user's request for the turn.
    pub request: Option<&'a str>,
}

/// Whether an ask goes to a reviewer: `FORGE_ASK_REVIEW=always`, or (auto) a run nobody
/// answers. Never inside a review, never with `never`, never in dontAsk mode.
pub fn wanted(rt: &Runtime, input: &HookInput) -> bool {
    if rt.env.in_review || input.permission_mode.as_deref() == Some("dontAsk") {
        return false;
    }
    match rt.env.ask_review.as_deref() {
        Some("always") => return true,
        Some("never") => return false,
        _ => {}
    }
    matches!(
        rt.env.entrypoint.as_deref(),
        Some("sdk-cli" | "sdk-ts" | "sdk-py" | "claude-code-github-action")
    )
}

/// A rule's ask reviewed (PreToolUse): the PreToolUse decision, or None to ask as usual.
/// Verdicts are cached per session for the same rule, input, concern and request.
pub fn review_ask(
    rt: &Runtime,
    input: &HookInput,
    ask: &Ask,
    log_state: &Path,
    session_dir: Option<&Path>,
    cwd: &Path,
) -> Option<Output> {
    if !wanted(rt, input) {
        return None;
    }
    let tool_input = input.tool_input.to_string();
    let norm: String = format!(
        "{}\t{}\t{}\t{}",
        ask.rule,
        tool_input,
        ask.concern,
        ask.request.unwrap_or("")
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
    let key: String = Sha256::digest(norm.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let cache = session_dir.map(|d| d.join("review-cache"));
    let cached = cache.as_ref().and_then(|c| {
        std::fs::read_to_string(c).ok().and_then(|t| {
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
    });
    let (allow, reason) = match cached {
        Some(v) => v,
        None => {
            let v = run_reviewer(rt, input.tool(), &tool_input, Some(ask), cwd)?;
            if let Some(c) = &cache {
                state::append_line(
                    c,
                    &format!("{key}\t{}\t{}", if v.0 { "allow" } else { "deny" }, v.1),
                );
            }
            log(log_state, input, v.0, &tool_input, &v.1);
            v
        }
    };
    Some(pre_decision(
        if allow { "allow" } else { "deny" },
        &format!("Reviewed by an independent session, nobody being there to answer: {reason}"),
    ))
}

/// PermissionRequest: an ask nobody can answer, reviewed. The project's hook defers to the
/// user-level one when both are set up, so an ask is reviewed once.
pub fn permission_request(rt: &Runtime, input: &HookInput, user: bool) -> Output {
    if !wanted(rt, input) {
        return Output::silent();
    }
    let project = rt.project(input);
    if !user && (project.is_none() || rt.user_lock().is_some_and(|l| l.is_file())) {
        return Output::silent();
    }
    let cwd = rt.cwd(input);
    let tool_input = input.tool_input.to_string();
    let Some((allow, reason)) = run_reviewer(rt, input.tool(), &tool_input, None, &cwd) else {
        return Output::silent();
    };
    let state = project.map_or_else(|| rt.user_state(), |p| p.state);
    log(&state, input, allow, &tool_input, &reason);
    let decision = if allow {
        json!({"behavior": "allow"})
    } else {
        json!({
            "behavior": "deny",
            "message": format!("Denied by an independent review, nobody being there to answer: {reason}"),
        })
    };
    Output::json(json!({
        "hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}
    }))
}

fn log(state_dir: &Path, input: &HookInput, allow: bool, tool_input: &str, reason: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    state::append_line(
        &state_dir.join("reviews.log"),
        &format!(
            "{ts}\t{}\t{}\t{}\t{}\t{}",
            input.session_key(),
            if allow { "allow" } else { "deny" },
            input.tool(),
            clean(tool_input),
            clean(reason)
        ),
    );
}

const SCHEMA: &str = r#"{"type":"object","properties":{"decision":{"type":"string","enum":["allow","deny"]},"reason":{"type":"string"}},"required":["decision","reason"],"additionalProperties":false}"#;

/// The reviewer's verdict: (allow, reason). None when claude cannot run or gives no verdict.
fn run_reviewer(
    rt: &Runtime,
    tool: &str,
    tool_input: &str,
    ask: Option<&Ask>,
    cwd: &Path,
) -> Option<(bool, String)> {
    let task = match ask {
        Some(a) => {
            let q = a.question.unwrap_or(
                "Is this action, as far as that concern goes, normal work on this repository and safe to run unattended?",
            );
            let req = a.request.map_or(String::new(), |r| {
                format!(
                    "\n\nThe user's request for this turn (what the agent was asked to do; not the conversation):\n<request>\n{r}\n</request>"
                )
            });
            format!(
                "A rule of the project's quality tooling flagged it. Its concern: {}\nThe question you answer: {q}{req}\n\nJudge only that question. The rest of the action is not under review: other commands or changes in it are covered by their own rules and permissions, and are no reason to deny. Allow when the answer clearly clears the concern; deny when it does not, or when you cannot tell.",
                a.concern
            )
        }
        None => "Allow it only if it is clearly within normal work on this repository and safe to run unattended. Deny anything irreversible or destructive (deleting data or history, force pushes, dropping databases), anything that publishes, deploys or pushes outside this machine, anything that reads or sends secrets, installs packages you cannot verify, weakens security, permissions, tests or tool configuration, or runs code fetched from the internet. When in doubt, deny.".to_string(),
    };
    let prompt = format!(
        "You are reviewing one action that an autonomous coding agent wants to take. Nobody is available to approve it, so you decide in the user's place, as a careful senior developer who sees only this action would.\n\nRepository: {}\nTool: {tool}\nInput: {tool_input}\n\nYou do not see the agent's conversation or its reasons, on purpose: judge the action itself. You may read files in the repository to understand it. {task} Give the reason in one sentence.",
        cwd.display()
    );
    let mut cmd = Command::new(&rt.env.claude);
    cmd.current_dir(cwd)
        .env("FORGE_REVIEWER", "1")
        .args([
            "-p",
            &prompt,
            "--output-format",
            "json",
            "--json-schema",
            SCHEMA,
        ])
        .args([
            "--tools",
            "Read,Grep,Glob",
            "--allowedTools",
            "Read,Grep,Glob",
        ])
        .args(["--permission-mode", "dontAsk", "--max-turns", "12"])
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
    let out = proc::run(&mut cmd, None, REVIEW_CAP)?;
    if !out.ok {
        return None;
    }
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let so = &v["structured_output"];
    let allow = match so["decision"].as_str()? {
        "allow" => true,
        "deny" => false,
        _ => return None,
    };
    let reason = so["reason"]
        .as_str()
        .filter(|r| !r.trim().is_empty())
        .unwrap_or("no reason given")
        .replace(['\t', '\n'], " ");
    Some((allow, reason))
}
