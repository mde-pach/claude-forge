//! The capability log (§20.7), `<state>/<repo-id>/capabilities.log`: one tab-separated line per
//! decision and per load, for the person and for `forge status`.
//!
//! ```text
//! <unix time>\t<session>\tdecision\t<capability id>\t<value>,<value>…\tselected|not-selected
//! <unix time>\t<session>\tload\t<capability id>
//! ```
//!
//! A decision's values are the engine's, one per `relevant` statement in its order (`-` for a
//! statement the answer left out). Writes are appends, best effort: a log that cannot be
//! written loses the line, never the prompt.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const LOG_FILE: &str = "capabilities.log";

/// The log of a repository's state directory.
pub fn log_path(repo_state: &Path) -> PathBuf {
    repo_state.join(LOG_FILE)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// One field of a line: no tab, no newline.
fn field(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn append(path: &Path, line: &str) {
    if let Some(dir) = path.parent() {
        crate::private_dir(dir);
    }
    let file = crate::private_file(path, true);
    if let Ok(mut f) = file {
        let _ = writeln!(f, "{line}");
    }
}

/// Logs the engine's decision on one capability.
pub fn decision(path: &Path, session: &str, id: &str, values: &[Option<f64>], selected: bool) {
    let values: Vec<String> = values
        .iter()
        .map(|v| v.map_or_else(|| "-".to_string(), |v| format!("{v:.3}")))
        .collect();
    append(
        path,
        &format!(
            "{}\t{}\tdecision\t{}\t{}\t{}",
            now(),
            field(session),
            field(id),
            values.join(","),
            if selected { "selected" } else { "not-selected" }
        ),
    );
}

/// Logs that the agent loaded an on-demand capability.
pub fn load(path: &Path, session: &str, id: &str) {
    append(
        path,
        &format!("{}\t{}\tload\t{}", now(), field(session), field(id)),
    );
}

/// How often each capability was selected and loaded, from the log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Counts {
    /// Prompts the engine decided on it.
    pub decided: u64,
    pub selected: u64,
    pub loaded: u64,
}

/// The counts per capability id; an absent or unreadable log counts nothing.
pub fn counts(path: &Path) -> BTreeMap<String, Counts> {
    let mut out: BTreeMap<String, Counts> = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            [_, _, "decision", id, _, verdict] => {
                let c = out.entry((*id).to_string()).or_default();
                c.decided += 1;
                c.selected += u64::from(*verdict == "selected");
            }
            [_, _, "load", id] => out.entry((*id).to_string()).or_default().loaded += 1,
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Decisions and loads are lines of the log; the counts read them back.
    #[test]
    fn decisions_and_loads_are_counted() {
        let dir = tempfile::tempdir().unwrap();
        let path = log_path(&dir.path().join("repo"));
        decision(&path, "s1", "acme/web", &[Some(0.91), None], true);
        decision(&path, "s1", "acme/db", &[Some(0.02)], false);
        decision(&path, "s2", "acme/web", &[Some(0.4)], true);
        load(&path, "s2", "acme/web");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\ts1\tdecision\tacme/web\t0.910,-\tselected\n"),
            "{text}"
        );
        let c = counts(&path);
        assert_eq!(
            c["acme/web"],
            Counts {
                decided: 2,
                selected: 2,
                loaded: 1
            }
        );
        assert_eq!(
            c["acme/db"],
            Counts {
                decided: 1,
                selected: 0,
                loaded: 0
            }
        );
        assert!(counts(&dir.path().join("none")).is_empty());
    }
}
