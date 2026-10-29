//! Edit-time typecheck: the diagnostics a change added (spec §10.5).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A file's diagnostics now and with its base text.
pub(crate) type FileDiagnostics = (Vec<Diagnostic>, Vec<Diagnostic>);

/// One diagnostic of a file, as `ty check --output-format concise` prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Repository-relative path.
    pub path: String,
    /// 1-based.
    pub line: u32,
    /// 1-based.
    pub column: u32,
    /// `error`, `warning` or `info`.
    pub severity: String,
    pub code: Option<String>,
    pub message: String,
}

impl Diagnostic {
    /// `<path>:<line>:<column>: <severity>[<code>] <message>`.
    pub fn concise(&self) -> String {
        format!(
            "{}:{}:{}: {}",
            self.path,
            self.line,
            self.column,
            self.key()
        )
    }

    /// The diagnostic without its location: code moved within the file keeps its errors.
    pub fn key(&self) -> String {
        match &self.code {
            Some(c) => format!("{}[{c}] {}", self.severity, self.message),
            None => format!("{} {}", self.severity, self.message),
        }
    }

    /// From an LSP diagnostic.
    pub fn from_lsp(path: &str, d: &Value) -> Diagnostic {
        let start = &d["range"]["start"];
        let severity = match d.get("severity").and_then(Value::as_u64) {
            Some(2) => "warning",
            Some(3 | 4) => "info",
            _ => "error",
        };
        let code = match d.get("code") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        };
        let message = d
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .split('\n')
            .collect::<Vec<_>>()
            .join(" ");
        Diagnostic {
            path: path.to_owned(),
            line: start["line"].as_u64().map_or(0, |l| l as u32) + 1,
            column: start["character"].as_u64().map_or(0, |c| c as u32) + 1,
            severity: severity.to_owned(),
            code,
            message,
        }
    }
}

/// The diagnostics of `now` that its base did not have: an error counts only as many times as the
/// file now has more of it than its base. Warnings count too when `warnings` is set; nothing else.
pub fn added(now: &[Diagnostic], base: &[Diagnostic], warnings: bool) -> Vec<Diagnostic> {
    let counted = |d: &Diagnostic| d.severity == "error" || (warnings && d.severity == "warning");
    let mut left: HashMap<String, usize> = HashMap::new();
    for d in base.iter().filter(|d| counted(d)) {
        *left.entry(d.key()).or_default() += 1;
    }
    now.iter()
        .filter(|d| counted(d))
        .filter(|d| match left.get_mut(&d.key()) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(line: u32, msg: &str) -> Diagnostic {
        Diagnostic {
            path: "a.py".into(),
            line,
            column: 1,
            severity: "error".into(),
            code: Some("invalid-assignment".into()),
            message: msg.into(),
        }
    }

    #[test]
    fn only_added_errors_count() {
        let base = vec![e(3, "x")];
        let now = vec![e(5, "x"), e(6, "x"), e(7, "y")];
        let got = added(&now, &base, false);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].concise(), "a.py:6:1: error[invalid-assignment] x");
    }
}
