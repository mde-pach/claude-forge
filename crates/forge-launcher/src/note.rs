//! Person-facing notes the launcher leaves when it could not run the locked
//! version. Hooks must stay quiet towards the agent, so notes are written to
//! the repo's state directory and shown to the person later (the CLI reads
//! them with [`take_notes`], e.g. at session start or in `forge status`).
//!
//! One file per kind, overwritten: a hook that fires hundreds of times while
//! a download is pending leaves one note, not hundreds.

use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use forge_home::{Home, RepoId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// The locked version is not in the cache; the installed one ran instead.
    VersionUnavailable {
        locked: String,
        running: String,
        /// A background download was started.
        fetching: bool,
        /// Why a download did not happen or failed, if it was attempted.
        fetch_error: Option<String>,
    },
    /// We exec'd `forge-<locked>` but that binary reports another version.
    ExecVersionMismatch { locked: String, running: String },
    /// `exec` of the cached binary failed.
    ExecFailed { path: PathBuf, error: String },
    /// `forge.lock` could not be read; the installed version ran.
    BadLock { error: String },
}

impl Note {
    /// Stable file-name-safe identifier of the note's kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Note::VersionUnavailable { .. } => "version-unavailable",
            Note::ExecVersionMismatch { .. } => "exec-version-mismatch",
            Note::ExecFailed { .. } => "exec-failed",
            Note::BadLock { .. } => "bad-lock",
        }
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Note::VersionUnavailable {
                locked,
                running,
                fetching,
                fetch_error,
            } => {
                write!(
                    f,
                    "This project pins Forge {locked}, which is not downloaded yet, so Forge {running} ran instead."
                )?;
                if *fetching {
                    write!(f, " Forge {locked} is downloading in the background.")?;
                } else if let Some(e) = fetch_error {
                    write!(f, " The download did not happen: {e}.")?;
                }
                write!(f, " Run `forge apply` to get it now.")
            }
            Note::ExecVersionMismatch { locked, running } => write!(
                f,
                "The cached Forge {locked} binary reports version {running}, so it ran as {running}. Delete it and run `forge apply` to download it again."
            ),
            Note::ExecFailed { path, error } => write!(
                f,
                "Could not start the cached Forge binary {} ({error}), so the installed version ran instead.",
                path.display()
            ),
            Note::BadLock { error } => write!(
                f,
                "{error}. The installed Forge version ran instead; fix forge.lock or run `forge apply`."
            ),
        }
    }
}

/// `<state>/<repo-id>/notes`.
pub fn notes_dir(home: &Home, repo: &RepoId) -> PathBuf {
    home.repo_state_dir(repo).join("notes")
}

/// Writes the note, replacing any earlier note of the same kind. Skips the
/// write when the same text is already there (the common repeated case).
pub fn record_note(home: &Home, repo: &RepoId, note: &Note) -> io::Result<()> {
    let dir = notes_dir(home, repo);
    let path = dir.join(format!("launcher-{}.txt", note.kind()));
    let text = note.to_string();
    if fs::read_to_string(&path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(
        ".launcher-{}.{}.tmp",
        note.kind(),
        std::process::id()
    ));
    fs::write(&tmp, &text)?;
    fs::rename(&tmp, &path)
}

/// Reads and removes the launcher notes for a repo, oldest kind first by name.
pub fn take_notes(home: &Home, repo: &RepoId) -> io::Result<Vec<String>> {
    let dir = notes_dir(home, repo);
    let mut paths: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("launcher-") && n.ends_with(".txt"))
            })
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        if let Ok(t) = fs::read_to_string(&p) {
            out.push(t);
        }
        let _ = fs::remove_file(&p);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_dedups_and_take_clears() {
        let t = tempfile::tempdir().unwrap();
        let home = Home::new(t.path().join("c"), t.path().join("s"));
        let repo = RepoId::from_physical_path(std::path::Path::new("/r"));
        let n = Note::VersionUnavailable {
            locked: "0.7.0".into(),
            running: "0.6.0".into(),
            fetching: false,
            fetch_error: None,
        };
        record_note(&home, &repo, &n).unwrap();
        record_note(&home, &repo, &n).unwrap();
        record_note(&home, &repo, &Note::BadLock { error: "x".into() }).unwrap();
        let notes = take_notes(&home, &repo).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes[1].contains("pins Forge 0.7.0"));
        assert!(take_notes(&home, &repo).unwrap().is_empty());
    }
}
