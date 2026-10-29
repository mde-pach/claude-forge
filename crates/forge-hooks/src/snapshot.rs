//! Snapshots of the working tree, in the state directory:
//!
//! - the **turn snapshot** (UserPromptSubmit): `head` (the commit), `tree` (a hash of the
//!   uncommitted state), `dirty` (`path<TAB>blob hash`, `-` for a deleted file) and `files/<path>`
//!   (copies of the dirty files up to 1 MiB), the layout forge-check reads as the turn's base;
//! - **Bash snapshots** (PreToolUse Bash): the working tree as a git tree, written with a copy of
//!   the index and a separate object directory (the repository's objects read as alternates), so
//!   the user's index and object store are never touched. PostToolUse compares it with the tree
//!   after the command to find the files the command changed.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::proc::{GIT_CAP, git, git_env};
use crate::state;

/// The largest dirty file the turn snapshot copies.
pub const COPY_MAX: u64 = 1 << 20;

/// The uncommitted state: HEAD and each changed path's blob hash (`-`: deleted).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub head: Option<String>,
    pub branch: Option<String>,
    pub dirty: BTreeMap<String, String>,
}

impl Status {
    /// A hash of the state: equal when HEAD and every uncommitted file are the same.
    pub fn tree_hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.head.as_deref().unwrap_or("").as_bytes());
        h.update(b"\n");
        for (p, b) in &self.dirty {
            h.update(p.as_bytes());
            h.update(b"\t");
            h.update(b.as_bytes());
            h.update(b"\n");
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Paths a snapshot leaves out: Forge's, Claude Code's worktrees, git's own.
pub fn skipped(path: &str) -> bool {
    path.ends_with('/') || path.starts_with(".claude/worktrees/") || path.starts_with(".git/")
}

/// The changed paths and HEAD, from one `git status` (porcelain v2), with their blob hashes from
/// one `git hash-object`.
pub fn status(root: &Path) -> Option<Status> {
    let (branch, head, paths) = status_paths_only(root)?;
    let mut st = Status {
        head,
        branch,
        dirty: BTreeMap::new(),
    };
    let present: Vec<&String> = paths.iter().filter(|p| root.join(p).is_file()).collect();
    let hashes: Vec<String> = if present.is_empty() {
        Vec::new()
    } else {
        let mut input = String::new();
        for p in &present {
            input.push_str(p);
            input.push('\n');
        }
        let out = git_env(
            root,
            &["hash-object", "--stdin-paths"],
            &[],
            Some(input.as_bytes()),
            GIT_CAP,
        )?;
        if !out.ok {
            return None;
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()
    };
    if hashes.len() != present.len() {
        return None;
    }
    let mut hashed = present.into_iter().zip(hashes);
    let mut next = hashed.next();
    for p in paths.iter().cloned() {
        match &next {
            Some((q, h)) if **q == p => {
                st.dirty.insert(p, h.clone());
                next = hashed.next();
            }
            _ => {
                st.dirty.insert(p, "-".into());
            }
        }
    }
    Some(st)
}

/// The branch, HEAD and the changed paths (sorted), from one `git status`.
#[allow(clippy::type_complexity)]
pub fn status_paths_only(root: &Path) -> Option<(Option<String>, Option<String>, Vec<String>)> {
    let out = git(
        root,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--no-renames",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
        GIT_CAP,
    )?;
    let mut st = Status::default();
    let mut paths: Vec<String> = Vec::new();
    for rec in out.split(|b| *b == 0) {
        let rec = String::from_utf8_lossy(rec);
        if let Some(rest) = rec.strip_prefix("# branch.oid ") {
            if rest != "(initial)" {
                st.head = Some(rest.to_string());
            }
        } else if let Some(rest) = rec.strip_prefix("# branch.head ") {
            if rest != "(detached)" {
                st.branch = Some(rest.to_string());
            }
        } else if let Some(p) = rec.strip_prefix("? ") {
            paths.push(p.to_string());
        } else if rec.starts_with("1 ") {
            if let Some(p) = rec.splitn(9, ' ').nth(8) {
                paths.push(p.to_string());
            }
        } else if rec.starts_with("u ") {
            if let Some(p) = rec.splitn(11, ' ').nth(10) {
                paths.push(p.to_string());
            }
        }
    }
    paths.retain(|p| !p.is_empty() && !skipped(p) && !p.contains('\n'));
    paths.sort();
    paths.dedup();
    Some((st.branch, st.head, paths))
}

/// Writes the turn snapshot to `dir` (replacing it), from `st`.
pub fn write_turn(root: &Path, dir: &Path, st: &Status) -> bool {
    let Some(parent) = dir.parent() else {
        return false;
    };
    let tmp = parent.join(format!(".turn.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    if !state::ensure_dir(&tmp.join("files")) {
        return false;
    }
    let mut dirty = String::new();
    for (p, h) in &st.dirty {
        dirty.push_str(&format!("{p}\t{h}\n"));
        if h == "-" {
            continue;
        }
        let src = root.join(p);
        if std::fs::metadata(&src).is_ok_and(|m| m.len() <= COPY_MAX) {
            let dst = tmp.join("files").join(p);
            if let Some(d) = dst.parent() {
                state::ensure_dir(d);
            }
            let _ = std::fs::copy(&src, &dst);
        }
    }
    let ok = std::fs::write(
        tmp.join("head"),
        format!("{}\n", st.head.as_deref().unwrap_or("")),
    )
    .is_ok()
        && std::fs::write(tmp.join("tree"), format!("{}\n", st.tree_hash())).is_ok()
        && std::fs::write(tmp.join("dirty"), dirty).is_ok();
    if !ok {
        let _ = std::fs::remove_dir_all(&tmp);
        return false;
    }
    let old = parent.join(format!(".turn.old.{}", std::process::id()));
    let had = std::fs::rename(dir, &old).is_ok();
    let moved = std::fs::rename(&tmp, dir).is_ok();
    if had {
        let _ = std::fs::remove_dir_all(&old);
    }
    moved
}

/// The turn snapshot's `head` and `tree`.
pub fn turn_ids(dir: &Path) -> Option<(String, String)> {
    let head = std::fs::read_to_string(dir.join("head")).ok()?;
    let tree = state::read_trim(&dir.join("tree"))?;
    Some((head.trim().to_string(), tree))
}

/// The repository's object directory and index (`git rev-parse --git-path`), absolute; cached in
/// `cache` (one line each).
fn git_paths(root: &Path, cache: &Path) -> Option<(PathBuf, PathBuf)> {
    let parse = |s: &str| -> Option<(PathBuf, PathBuf)> {
        let mut l = s.lines();
        let o = root.join(l.next()?);
        let i = root.join(l.next()?);
        Some((o, i))
    };
    if let Some(p) = std::fs::read_to_string(cache)
        .ok()
        .as_deref()
        .and_then(parse)
    {
        if p.0.is_dir() {
            return Some(p);
        }
    }
    let out = git(
        root,
        &["rev-parse", "--git-path", "objects", "--git-path", "index"],
        GIT_CAP,
    )?;
    let text = String::from_utf8_lossy(&out).into_owned();
    let p = parse(&text)?;
    state::write_atomic(cache, text.as_bytes());
    Some(p)
}

/// The Bash snapshots of a session.
pub struct BashSnapshots {
    pub dir: PathBuf,
    root: PathBuf,
    paths_cache: PathBuf,
}

impl BashSnapshots {
    pub fn new(root: &Path, session_dir: &Path, repo_state: &Path) -> BashSnapshots {
        BashSnapshots {
            dir: session_dir.join("bash"),
            root: root.to_path_buf(),
            paths_cache: repo_state.join("git-paths"),
        }
    }

    pub fn tree_file(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.tree"))
    }

    /// The files a command changed, written by PostToolUse for the typecheck hook.
    pub fn files_file(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.files"))
    }

    fn index(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.index"))
    }

    fn objects(&self) -> PathBuf {
        self.dir.join("objects")
    }

    /// The working tree as a git tree id, within `cap`; the index copy `key` is kept for the
    /// snapshot after the command (it then only re-hashes what changed).
    pub fn tree(&self, key: &str, fresh_index: bool, cap: Duration) -> Option<String> {
        if !state::ensure_dir(&self.objects()) {
            return None;
        }
        let (objects, index) = git_paths(&self.root, &self.paths_cache)?;
        let tmp_index = self.index(key);
        if fresh_index || !tmp_index.is_file() {
            if index.is_file() {
                std::fs::copy(&index, &tmp_index).ok()?;
            } else {
                let _ = std::fs::remove_file(&tmp_index);
            }
        }
        let started = std::time::Instant::now();
        let objs = self.objects();
        let env: [(&str, &OsStr); 3] = [
            ("GIT_INDEX_FILE", tmp_index.as_os_str()),
            ("GIT_OBJECT_DIRECTORY", objs.as_os_str()),
            ("GIT_ALTERNATE_OBJECT_DIRECTORIES", objects.as_os_str()),
        ];
        let add = git_env(
            &self.root,
            &[
                "add",
                "-A",
                "--ignore-errors",
                "--",
                ".",
                ":(exclude).claude/worktrees",
            ],
            &env,
            None,
            cap,
        )?;
        if add.timed_out {
            return None;
        }
        let left = cap.saturating_sub(started.elapsed());
        let w = git_env(&self.root, &["write-tree"], &env, None, left)?;
        if !w.ok {
            return None;
        }
        let tree = String::from_utf8_lossy(&w.stdout).trim().to_string();
        (!tree.is_empty()).then_some(tree)
    }

    /// Added or modified files between two trees, outside Forge's, Claude's, git's and
    /// dependency directories.
    pub fn changed(&self, before: &str, after: &str) -> Vec<String> {
        let Some((objects, _)) = git_paths(&self.root, &self.paths_cache) else {
            return Vec::new();
        };
        let objs = self.objects();
        let env: [(&str, &OsStr); 2] = [
            ("GIT_OBJECT_DIRECTORY", objs.as_os_str()),
            ("GIT_ALTERNATE_OBJECT_DIRECTORIES", objects.as_os_str()),
        ];
        let Some(out) = git_env(
            &self.root,
            &[
                "diff-tree",
                "-r",
                "-z",
                "--no-renames",
                "--name-status",
                before,
                after,
            ],
            &env,
            None,
            GIT_CAP,
        )
        .filter(|r| r.ok) else {
            return Vec::new();
        };
        let parts: Vec<String> = out
            .stdout
            .split(|b| *b == 0)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        let mut files = Vec::new();
        for pair in parts.chunks(2) {
            if let [status, path] = pair {
                if (status == "A" || status == "M") && !excluded(path) {
                    files.push(path.clone());
                }
            }
        }
        files
    }

    /// Done with a command's snapshot.
    pub fn drop_key(&self, key: &str) {
        let _ = std::fs::remove_file(self.tree_file(key));
        let _ = std::fs::remove_file(self.index(key));
    }

    /// Drops the snapshots beyond the 50 newest (a denied command never reaches PostToolUse),
    /// with `max_age` also older ones, and the objects once none is pending.
    pub fn cleanup(&self, max_age: Option<Duration>) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut entries: Vec<(std::time::SystemTime, PathBuf)> = rd
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.ends_with(".tree") || n.ends_with(".files"))
            })
            .map(|e| {
                (
                    e.metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH),
                    e.path(),
                )
            })
            .collect();
        entries.sort_by(|a, b| b.0.cmp(&a.0));
        let now = std::time::SystemTime::now();
        let mut kept_trees = 0;
        for (i, (t, p)) in entries.iter().enumerate() {
            let old = max_age.is_some_and(|a| now.duration_since(*t).unwrap_or_default() > a);
            if i >= 50 || old {
                let _ = std::fs::remove_file(p);
                if let Some(k) = p.file_stem().and_then(|s| s.to_str()) {
                    let _ = std::fs::remove_file(self.index(k));
                }
            } else if p.extension().is_some_and(|e| e == "tree") {
                kept_trees += 1;
            }
        }
        if kept_trees == 0 {
            let _ = std::fs::remove_dir_all(self.objects());
        }
    }
}

/// Paths the edit-time checks never look at.
pub fn excluded(path: &str) -> bool {
    [".forge/", ".claude/", ".git/", "node_modules/", ".venv/"]
        .iter()
        .any(|d| path.starts_with(d) || path.contains(&format!("/{d}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sh(dir: &Path, script: &str) {
        let ok = Command::new("sh")
            .args(["-c", script])
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "{script}");
    }

    #[test]
    fn turn_and_bash_snapshots() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("r");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        sh(
            &root,
            "git init -q && echo a > a.txt && echo b > b.txt && git add . && git commit -qm init && echo A > a.txt && rm b.txt && echo n > new.txt",
        );
        let st = status(&root).unwrap();
        assert!(st.head.is_some());
        assert_eq!(st.dirty.len(), 3);
        assert_eq!(st.dirty["b.txt"], "-");
        let snap = t.path().join("s/turn");
        assert!(write_turn(&root, &snap, &st));
        assert_eq!(
            std::fs::read_to_string(snap.join("files/a.txt")).unwrap(),
            "A\n"
        );
        assert!(!snap.join("files/b.txt").exists());
        assert_eq!(turn_ids(&snap).unwrap().1, st.tree_hash());
        // unchanged: same hash; a change: another
        assert_eq!(status(&root).unwrap().tree_hash(), st.tree_hash());
        std::fs::write(root.join("a.txt"), "AA\n").unwrap();
        assert_ne!(status(&root).unwrap().tree_hash(), st.tree_hash());

        let index_before = std::fs::read(root.join(".git/index")).unwrap();
        let b = BashSnapshots::new(&root, &t.path().join("s"), &t.path().join("st"));
        let before = b.tree("k1", true, Duration::from_secs(10)).unwrap();
        std::fs::write(root.join("made.txt"), "m\n").unwrap();
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("node_modules/x.js"), "x\n").unwrap();
        let after = b.tree("k1", false, Duration::from_secs(10)).unwrap();
        assert_eq!(b.changed(&before, &after), ["a.txt", "made.txt"]);
        assert_eq!(
            std::fs::read(root.join(".git/index")).unwrap(),
            index_before
        );
        b.drop_key("k1");
        b.cleanup(None);
        assert!(!b.dir.join("objects").exists());
    }
}
