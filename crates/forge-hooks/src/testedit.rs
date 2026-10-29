//! Edits to tests that existed at the start of the turn.
//!
//! PreToolUse (Edit, Write, MultiEdit): when the edit removes a test, or changes it (formatting
//! aside), it asks; in an autonomous run the reviewer answers, given the user's request for the
//! turn. The tests asked about are pending for this tool call (`<turn>/tests-pending/<id>`).
//! PostToolUse: the call ran, so it was approved: its pending tests go to `<turn>/tests-approved`,
//! which the Stop check does not block on, and the file to `<turn>/touched`.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::input::HookInput;
use crate::proc::{GIT_CAP, git};
use crate::review::{self, Ask};
use crate::{Output, Project, Runtime, pre_decision, state};

pub fn pre(rt: &Runtime, input: &HookInput, cwd: &Path) -> Output {
    let Some(p) = rt.project(input) else {
        return Output::silent();
    };
    let Some(file) = input.input_str("file_path").and_then(|f| p.rel(f, cwd)) else {
        return Output::silent();
    };
    let Some(rule) = rt.checker.test_edit_rule(&p.root, &file) else {
        return Output::silent();
    };
    let Ok(before) = std::fs::read_to_string(p.root.join(&file)) else {
        return Output::silent();
    };
    let Some(base) = base_text(&p, &file) else {
        return Output::silent();
    };
    let Some(after) = apply(input, &before) else {
        return Output::silent();
    };
    let blocks = |s: &str| -> BTreeMap<String, String> {
        rt.checker
            .test_blocks(&file, s)
            .into_iter()
            .filter(|b| !b.key.starts_with("support:"))
            .map(|b| (b.key, b.print))
            .collect()
    };
    let (base, then, now) = (blocks(&base), blocks(&before), blocks(&after));
    let mut asked: Vec<(String, &str)> = Vec::new();
    for (k, print) in &then {
        if !base.contains_key(k) {
            continue;
        }
        match now.get(k) {
            None => asked.push((k.clone(), "removed")),
            Some(n) if n != print => asked.push((k.clone(), "changed")),
            _ => {}
        }
    }
    if asked.is_empty() {
        return Output::silent();
    }
    if let (Some(key), Some(snap)) = (input.tool_key(), p.snapshot()) {
        let pending: String = asked
            .iter()
            .map(|(k, _)| format!("{file}\t{k}\n"))
            .collect();
        state::write_atomic(&snap.join("tests-pending").join(key), pending.as_bytes());
    }
    let list = asked
        .iter()
        .map(|(k, how)| format!("\"{k}\" ({how})"))
        .collect::<Vec<_>>()
        .join(", ");
    let reason = format!(
        "This edit changes or removes tests in {file} that existed before this turn: {list}. Make it only if the task changes the behaviour these tests assert; otherwise leave the tests as they are and change the code instead."
    );
    let request = std::fs::read_to_string(p.session_dir.join("prompt")).ok();
    let ask = Ask {
        rule: &rule.id,
        concern: &reason,
        question: rule.review.as_deref(),
        request: Some(request.as_deref().unwrap_or("(not recorded)")),
    };
    review::review_ask(rt, input, &ask, &p.state, Some(&p.session_dir), cwd)
        .unwrap_or_else(|| pre_decision("ask", &reason))
}

/// The file at the start of the turn (the snapshot's copy, else HEAD's); None when it did not
/// exist then.
fn base_text(p: &Project, file: &str) -> Option<String> {
    let snap = p.snapshot();
    let mut rev = None;
    if let Some(s) = &snap {
        let dirty = std::fs::read_to_string(s.join("dirty")).unwrap_or_default();
        if let Some(h) = dirty
            .lines()
            .find_map(|l| l.split_once('\t').filter(|(q, _)| *q == file).map(|x| x.1))
        {
            if h == "-" {
                return None;
            }
            return std::fs::read_to_string(s.join("files").join(file)).ok();
        }
        rev = state::read_trim(&s.join("head"));
    }
    let rev = rev.unwrap_or_else(|| "HEAD".into());
    let out = git(
        &p.root,
        &["cat-file", "blob", &format!("{rev}:{file}")],
        GIT_CAP,
    )?;
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// The file after the edit, as Claude Code applies it.
fn apply(input: &HookInput, before: &str) -> Option<String> {
    let edit = |text: &str, e: &Value| -> Option<String> {
        let old = e.get("old_string")?.as_str()?;
        let new = e.get("new_string")?.as_str()?;
        if old.is_empty() || !text.contains(old) {
            return None;
        }
        Some(
            if e.get("replace_all").and_then(Value::as_bool) == Some(true) {
                text.replace(old, new)
            } else {
                text.replacen(old, new, 1)
            },
        )
    };
    match input.tool() {
        "Write" => input.input_str("content").map(str::to_owned),
        "Edit" => edit(before, &input.tool_input),
        "MultiEdit" => {
            let mut t = before.to_string();
            for e in input.tool_input.get("edits")?.as_array()? {
                t = edit(&t, e)?;
            }
            Some(t)
        }
        _ => None,
    }
}

/// PostToolUse of an Edit or Write: the call ran, so its pending tests were approved.
pub fn post(p: &Project, input: &HookInput, file: &str) {
    let Some(snap) = p.snapshot() else { return };
    let touched = snap.join("touched");
    let seen = std::fs::read_to_string(&touched).unwrap_or_default();
    if !seen.lines().any(|l| l == file) {
        state::append_line(&touched, file);
    }
    let Some(key) = input.tool_key() else { return };
    let pending = snap.join("tests-pending").join(key);
    if let Ok(text) = std::fs::read_to_string(&pending) {
        for l in text.lines().filter(|l| !l.is_empty()) {
            state::append_line(&snap.join("tests-approved"), l);
        }
        let _ = std::fs::remove_file(pending);
    }
}
