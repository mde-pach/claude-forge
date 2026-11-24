//! The edit-time findings, given when the agent's batch of tool calls ends (§8.4).
//!
//! The agent can make several calls in one answer; Claude Code runs them one after the other,
//! then hands all their results back at once. A check after one edit sees the file before the
//! batch's later edits (a name used before the edit that imports it), so its findings may be gone
//! by the time the agent reads them. Each check after an edit is therefore held, under its call's
//! id, and the PostToolBatch hook gives the batch's findings: for each file, those of the last
//! check that looked at it. Formatting and fixes still happen right after each edit, where Claude
//! Code takes the file's new content into account.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use forge_core::{Finding, Report};
use serde::{Deserialize, Serialize};

use crate::{Project, state};

/// A held check left behind (a batch whose hook never ran) is dropped after this.
const STALE: Duration = Duration::from_secs(3600);

/// One check after a call: the files it looked at and what it found.
#[derive(Default, Serialize, Deserialize)]
struct Held {
    checked: Vec<String>,
    findings: Vec<Finding>,
}

fn dir(p: &Project) -> PathBuf {
    p.session_dir.join("held")
}

/// A call id as a file name: Claude Code's ids are `toolu_` and letters and digits.
fn file_of(p: &Project, id: &str) -> Option<PathBuf> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then(|| dir(p).join(format!("{id}.json")))
}

/// Holds the findings of the check after call `id`, which looked at `checked`. False when they
/// cannot be held (no usable id, the state cannot be written): they are then given at once.
pub(crate) fn hold(p: &Project, id: &str, checked: Vec<String>, findings: Vec<Finding>) -> bool {
    let Some(file) = file_of(p, id) else {
        return false;
    };
    if !state::ensure_dir(&dir(p)) {
        return false;
    }
    let Ok(bytes) = serde_json::to_vec(&Held { checked, findings }) else {
        return false;
    };
    state::write_atomic(&file, &bytes);
    file.is_file()
}

/// The findings held for the batch's calls `ids` (in the order they ran), removed from the
/// state: per file, those of the last check that looked at it or found something in it.
pub(crate) fn release(p: &Project, ids: &[String]) -> Report {
    // (file, its findings), in the order the files first came up
    let mut per_file: Vec<(String, Vec<Finding>)> = Vec::new();
    for id in ids {
        let Some(file) = file_of(p, id) else {
            continue;
        };
        let Some(held) = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<Held>(&b).ok())
        else {
            continue;
        };
        let _ = std::fs::remove_file(&file);
        let mut fresh: Vec<(String, Vec<Finding>)> =
            held.checked.into_iter().map(|c| (c, Vec::new())).collect();
        for f in held.findings {
            let key = f.path.clone().unwrap_or_default();
            match fresh.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(f),
                None => fresh.push((key, vec![f])),
            }
        }
        for (key, findings) in fresh {
            match per_file.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => *v = findings,
                None => per_file.push((key, findings)),
            }
        }
    }
    sweep(p);
    Report {
        findings: per_file.into_iter().flat_map(|(_, v)| v).collect(),
        notes: Vec::new(),
    }
}

/// Drops held checks older than [`STALE`].
fn sweep(p: &Project) {
    let Ok(rd) = std::fs::read_dir(dir(p)) else {
        return;
    };
    let now = SystemTime::now();
    for e in rd.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > STALE);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
