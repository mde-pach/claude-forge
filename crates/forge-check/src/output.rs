//! How a run reads for a person (`forge check`) and for scripts (`--json`).

use forge_core::Severity;
use serde_json::json;

use crate::checker::Outcome;

/// Findings with their rule ids (errors first, then advice), then the notes.
pub fn person_text(o: &Outcome) -> String {
    let mut out = String::new();
    let errors = o
        .report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error);
    let advice = o
        .report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Advice);
    for f in errors.chain(advice) {
        out.push_str(&f.person_text());
        out.push('\n');
    }
    if !o.report.notes.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("Notes:\n");
        for n in &o.report.notes {
            out.push_str(&format!("  {n}\n"));
        }
    }
    out
}

/// The run as JSON: status, exit code, findings (with their agent text), notes, unverified tests.
pub fn json(o: &Outcome) -> serde_json::Value {
    json!({
        "status": o.status,
        "exit": o.status.exit_code(),
        "findings": o.report.findings.iter().map(|f| json!({
            "rule": f.rule, "severity": f.severity, "path": f.path, "line": f.line,
            "message": f.message, "agent_text": f.agent_text(),
        })).collect::<Vec<_>>(),
        "notes": o.report.notes,
        "unverified": o.unverified,
        "timings_ms": {"forge": o.timings.forge.as_millis() as u64, "tools": o.timings.tools.as_millis() as u64},
    })
}

/// For a person whose commit the commit-time check (`--changed`) refused: the forge.yaml
/// `accept:` entry that would accept each failing finding instead of fixing it (§10.5), by rule
/// and file, or by its text when it names no file. Empty when nothing failed or nothing can be
/// named precisely (a multi-line failure with no file). Never part of the agent's text.
pub fn accept_hint(o: &Outcome) -> String {
    let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
    let mut entries: Vec<String> = Vec::new();
    for f in o
        .report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
    {
        let entry = match &f.path {
            Some(p) => format!(
                "  - {{rule: {}, file: {}, reason: \"<why>\"}}",
                f.rule,
                q(p)
            ),
            None if !f.message.contains('\n') => format!(
                "  - {{rule: {}, change: {}, reason: \"<why>\"}}",
                f.rule,
                q(f.message.trim())
            ),
            None => continue,
        };
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    if entries.is_empty() {
        return String::new();
    }
    format!(
        "\nTo accept one of these instead of fixing it, add its entry to forge.yaml, with your reason (it then holds for that rule in that file):\naccept:\n{}\n",
        entries.join("\n")
    )
}
