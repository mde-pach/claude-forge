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
