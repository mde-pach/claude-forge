//! The hooks' files in the state root (never in the repository): per repository, per session,
//! and the person's notes.
//!
//! ```text
//! <state>/<repo-id>/hook-cache.json          guard rules, facts, typed languages (rules.rs)
//! <state>/<repo-id>/notes/hooks.jsonl        notes for the person (forge status)
//! <state>/<repo-id>/reviews.log              reviewer verdicts
//! <state>/<repo-id>/sessions/<id>/turn/      the turn snapshot (head, tree, dirty, files/)
//! <state>/<repo-id>/sessions/<id>/bash/      Bash snapshots (<tool-use-id>.tree / .files)
//! <state>/<repo-id>/sessions/<id>/blocks     Stop blocks this turn
//! <state>/user/…                             the same for the user-level hooks outside a project
//! ```

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use forge_core::Note;

/// Creates `dir` (and its parents) readable by the user only.
pub fn ensure_dir(dir: &Path) -> bool {
    if dir.is_dir() {
        return true;
    }
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .is_ok()
}

/// Writes `path` through a temporary file and a rename, so readers never see half of it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    if !ensure_dir(dir) {
        return false;
    }
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("f"),
        std::process::id()
    ));
    if fs::write(&tmp, bytes).is_err() {
        return false;
    }
    fs::rename(&tmp, path).is_ok()
}

/// The file's text, trimmed; None when absent or empty.
pub fn read_trim(path: &Path) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

pub fn append_line(path: &Path, line: &str) -> bool {
    if let Some(d) = path.parent() {
        ensure_dir(d);
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| writeln!(f, "{line}"))
        .is_ok()
}

/// Where the hooks keep the person's notes under a state directory.
pub fn notes_file(state: &Path) -> PathBuf {
    state.join("notes").join("hooks.jsonl")
}

/// The most notes kept: the oldest go first.
const MAX_NOTES: usize = 50;

/// Records notes for `forge status`: a note already recorded (same text) is not repeated.
pub fn record_notes(state: &Path, notes: &[Note]) {
    if notes.is_empty() {
        return;
    }
    let file = notes_file(state);
    let mut lines: Vec<String> = fs::read_to_string(&file)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    let mut changed = false;
    for n in notes {
        let Ok(line) = serde_json::to_string(n) else {
            continue;
        };
        if !lines.contains(&line) {
            lines.push(line);
            changed = true;
        }
    }
    if !changed {
        return;
    }
    let skip = lines.len().saturating_sub(MAX_NOTES);
    let mut text = lines[skip..].join("\n");
    text.push('\n');
    write_atomic(&file, text.as_bytes());
}

/// The recorded notes, oldest first, removed (what `forge status` prints).
pub fn take_notes(state: &Path) -> Vec<Note> {
    let file = notes_file(state);
    let out = read_notes(state);
    let _ = fs::remove_file(file);
    out
}

/// The recorded notes, oldest first.
pub fn read_notes(state: &Path) -> Vec<Note> {
    fs::read_to_string(notes_file(state))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The hooks' notes of the repository at `root`, removed, as `forge status` prints them.
pub fn take_repo_notes(home: &forge_home::Home, root: &Path) -> Vec<String> {
    let Ok(id) = forge_home::RepoId::for_root(root) else {
        return Vec::new();
    };
    take_notes(&home.repo_state_dir(&id))
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::NoteKind;

    #[test]
    fn notes_dedup_and_take() {
        let t = tempfile::tempdir().unwrap();
        let n = Note::new(NoteKind::Environment, "large tree");
        record_notes(t.path(), &[n.clone(), n.clone()]);
        record_notes(t.path(), std::slice::from_ref(&n));
        assert_eq!(read_notes(t.path()), vec![n.clone()]);
        assert_eq!(take_notes(t.path()), vec![n]);
        assert!(read_notes(t.path()).is_empty());
    }
}
