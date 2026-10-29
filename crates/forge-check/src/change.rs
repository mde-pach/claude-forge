//! The change a run is about: which files changed against which base, their base copies and
//! the lines the change added (`forge_change_set`, `forge_added_lines` in the engine).
//!
//! Bases:
//! - a turn snapshot (Stop): the tree at the start of the turn. The snapshot directory holds
//!   `head` (the commit then, possibly empty), `dirty` (`<path><TAB><blob hash>` per path that
//!   was uncommitted then, `-` when it was deleted) and `files/<path>` (a copy of each dirty file).
//! - `HEAD` (`--changed`, `--all`, and the edit-time checks before the first prompt);
//! - `git merge-base <ref> HEAD` (`--since <ref>`).
//!
//! Git is asked through its CLI with one process per question for the whole change
//! (`status -z`, `ls-tree`, `hash-object --stdin-paths`, `cat-file --batch`): spawning git is
//! cheaper than opening the repository with a library for the few questions a run asks.
//! Added lines are computed in-process (Myers with git's slider heuristics, `imara-diff`),
//! which is what `git diff --no-index -U0` reports.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use forge_core::builtin::Scope;

use crate::scope::is_excluded;

/// What a run compares the change with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Uncommitted changes against HEAD (pre-commit).
    Changed,
    /// Everything since `git merge-base <ref> HEAD` (CI on a pull request).
    Since(String),
    /// The whole project (CI on the default branch); HEAD is the base of the counts.
    All,
    /// The files changed during the agent's turn, against the turn snapshot.
    Turn(PathBuf),
}

impl Mode {
    pub fn scope(&self) -> Scope {
        match self {
            Mode::All => Scope::All,
            Mode::Turn(_) => Scope::Turn,
            _ => Scope::Changed,
        }
    }
}

/// The lines of a file the change added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    /// The file is new since the base: every line.
    Whole,
    /// 1-based line numbers.
    Lines(BTreeSet<u32>),
}

impl Added {
    pub fn contains(&self, line: u32) -> bool {
        match self {
            Added::Whole => true,
            Added::Lines(l) => l.contains(&line),
        }
    }

    /// Whether a match spanning `start..=end` overlaps an added line.
    pub fn overlaps(&self, start: u32, end: u32) -> bool {
        match self {
            Added::Whole => true,
            Added::Lines(l) => l.range(start..=end.max(start)).next().is_some(),
        }
    }
}

#[derive(Debug)]
pub enum ChangeError {
    /// `--since <ref>`: git has no merge base for it and HEAD.
    NoMergeBase(String),
    NotARepository(PathBuf),
    Io(String),
}

impl std::fmt::Display for ChangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeError::NoMergeBase(r) => write!(
                f,
                "no merge base for {r} and HEAD (fetch it: git fetch origin <branch>, or check out with fetch-depth: 0)"
            ),
            ChangeError::NotARepository(p) => {
                write!(f, "{} is not in a git repository", p.display())
            }
            ChangeError::Io(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for ChangeError {}

/// The change: changed paths, their base copies and added lines.
#[derive(Debug)]
pub struct ChangeSet {
    pub root: PathBuf,
    pub mode: Mode,
    /// The base commit, when there is one.
    pub base_rev: Option<String>,
    /// The turn snapshot, with `--turn` (and edit-time checks after a prompt).
    pub snapshot: Option<PathBuf>,
    /// Changed paths (sorted, repository-relative).
    pub changed: Vec<String>,
    /// Those that exist now.
    pub existing: Vec<String>,
    /// Those gone since the base.
    pub deleted: Vec<String>,
    /// Those that exist in the base (their copies are in `base_dir`).
    pub inbase: Vec<String>,
    /// Added lines per existing changed file (empty with `--all`).
    pub added: BTreeMap<String, Added>,
    /// With `--all`: every file of the project (tracked and untracked, not ignored).
    pub all: Option<Vec<String>>,
    base: tempfile::TempDir,
}

impl ChangeSet {
    /// The directory holding each changed file as it is in the base.
    pub fn base_dir(&self) -> &Path {
        self.base.path()
    }

    /// The base copy of `path`, when it exists in the base.
    pub fn base_copy(&self, path: &str) -> Option<PathBuf> {
        let p = self.base.path().join(path);
        p.is_file().then_some(p)
    }

    /// The files a whole-project scope covers: every project file with `--all`, else the
    /// existing changed files.
    pub fn scope_files(&self) -> &[String] {
        self.all.as_deref().unwrap_or(&self.existing)
    }

    /// Whether `path:line` is on a line the change added (always with `--all`).
    pub fn on_added(&self, path: &str, start: u32, end: u32) -> bool {
        if self.mode == Mode::All {
            return true;
        }
        self.added.get(path).is_some_and(|a| a.overlaps(start, end))
    }

    pub fn is_changed(&self, path: &str) -> bool {
        self.changed
            .binary_search_by(|p| p.as_str().cmp(path))
            .is_ok()
    }

    /// What the change is compared with, for messages to a person.
    pub fn base_name(&self) -> String {
        match &self.mode {
            Mode::Turn(_) => "the start of the turn".into(),
            Mode::Since(r) => format!("the merge base with {r}"),
            _ => "HEAD".into(),
        }
    }

    /// The change for `mode`.
    pub fn compute(root: &Path, mode: Mode) -> Result<ChangeSet, ChangeError> {
        let git = Git::new(root);
        let (base_rev, snapshot) = match &mode {
            Mode::Turn(snap) => {
                let head = std::fs::read_to_string(snap.join("head")).unwrap_or_default();
                let head = head.trim();
                (
                    (!head.is_empty()).then(|| head.to_string()),
                    Some(snap.clone()),
                )
            }
            Mode::Since(r) => {
                let out = git
                    .run(&["merge-base", r, "HEAD"])
                    .map_err(|_| ChangeError::NoMergeBase(r.clone()))?;
                let rev = String::from_utf8_lossy(&out).trim().to_string();
                if rev.is_empty() {
                    return Err(ChangeError::NoMergeBase(r.clone()));
                }
                (Some(rev), None)
            }
            _ => (git.head(), None),
        };
        let dirty = snapshot.as_deref().map(read_dirty).unwrap_or_default();
        let mut paths: BTreeSet<String> = BTreeSet::new();
        match &mode {
            Mode::Turn(_) => {
                paths.extend(git.status_paths()?);
                paths.extend(dirty.keys().cloned());
                if let Some(rev) = &base_rev {
                    paths.extend(split_z(
                        &git.run(&["diff", "--name-only", "-z", "--no-renames", rev, "HEAD"])
                            .unwrap_or_default(),
                    ));
                }
            }
            Mode::Since(_) => {
                let rev = base_rev.as_deref().unwrap_or("HEAD");
                paths.extend(split_z(
                    &git.run(&["diff", "--name-only", "-z", "--no-renames", rev])
                        .unwrap_or_default(),
                ));
                paths.extend(split_z(
                    &git.run(&["ls-files", "-z", "--others", "--exclude-standard"])
                        .unwrap_or_default(),
                ));
            }
            Mode::Changed | Mode::All => paths.extend(git.status_paths()?),
        }
        paths.retain(|p| !p.is_empty() && !p.ends_with('/') && !is_excluded(p));
        let mut paths: Vec<String> = paths.into_iter().collect();
        if let Mode::Turn(_) = mode {
            // only what differs from the start of the turn
            let now = git.hash_now(root, &paths);
            let base = base_hashes(&git, base_rev.as_deref(), &dirty, &paths);
            paths.retain(|p| now.get(p) != base.get(p));
        }
        let all = if mode == Mode::All {
            let mut files: Vec<String> = split_z(
                &git.run(&[
                    "ls-files",
                    "-z",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                ])
                .unwrap_or_default(),
            )
            .filter(|p| !is_excluded(p) && root.join(p).is_file())
            .collect();
            files.sort();
            files.dedup();
            Some(files)
        } else {
            None
        };
        ChangeSet::build(root, mode, base_rev, snapshot, &dirty, paths, all, true)
    }

    /// The change made to `files` (edit-time checks): against the turn snapshot when there is
    /// one, else HEAD.
    pub fn for_files(
        root: &Path,
        files: &[String],
        snapshot: Option<&Path>,
    ) -> Result<ChangeSet, ChangeError> {
        let git = Git::new(root);
        let snapshot = snapshot.filter(|s| s.join("head").is_file());
        let (base_rev, snapshot, mode) = match snapshot {
            Some(s) => {
                let head = std::fs::read_to_string(s.join("head")).unwrap_or_default();
                let head = head.trim().to_string();
                (
                    (!head.is_empty()).then_some(head),
                    Some(s.to_path_buf()),
                    Mode::Turn(s.to_path_buf()),
                )
            }
            None => (git.head(), None, Mode::Changed),
        };
        let dirty = snapshot.as_deref().map(read_dirty).unwrap_or_default();
        let mut paths: Vec<String> = files
            .iter()
            .map(|f| f.trim_start_matches("./").to_string())
            .filter(|p| !p.is_empty() && !is_excluded(p))
            .collect();
        paths.sort();
        paths.dedup();
        ChangeSet::build(root, mode, base_rev, snapshot, &dirty, paths, None, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        root: &Path,
        mode: Mode,
        base_rev: Option<String>,
        snapshot: Option<PathBuf>,
        dirty: &HashMap<String, String>,
        changed: Vec<String>,
        all: Option<Vec<String>>,
        lines: bool,
    ) -> Result<ChangeSet, ChangeError> {
        let git = Git::new(root);
        let base = tempfile::Builder::new()
            .prefix("forge-base.")
            .tempdir()
            .map_err(|e| ChangeError::Io(e.to_string()))?;
        // base copies: from the snapshot for paths dirty at the turn's start, else the base commit
        let mut from_rev = Vec::new();
        let mut inbase = Vec::new();
        for p in &changed {
            match (snapshot.as_ref(), dirty.get(p)) {
                (Some(snap), Some(h)) => {
                    if h != "-" {
                        let src = snap.join("files").join(p);
                        if src.is_file() && copy_to(&src, &base.path().join(p)).is_ok() {
                            inbase.push(p.clone());
                        }
                    }
                }
                _ => {
                    if base_rev.is_some() {
                        from_rev.push(p.clone());
                    }
                }
            }
        }
        if let Some(rev) = &base_rev
            && !from_rev.is_empty()
        {
            for (p, bytes) in git.cat_files(rev, &from_rev) {
                let dst = base.path().join(&p);
                if write_file(&dst, &bytes).is_ok() {
                    inbase.push(p);
                }
            }
        }
        inbase.sort();
        let mut existing = Vec::new();
        let mut deleted = Vec::new();
        let mut added = BTreeMap::new();
        for p in &changed {
            let now = root.join(p);
            if now.is_file() {
                existing.push(p.clone());
                if lines && mode != Mode::All {
                    let b = base.path().join(p);
                    let a = if b.is_file() {
                        added_lines_files(&b, &now)
                    } else {
                        Added::Whole
                    };
                    added.insert(p.clone(), a);
                }
            } else if inbase.binary_search(p).is_ok() {
                deleted.push(p.clone());
            }
        }
        Ok(ChangeSet {
            root: root.to_path_buf(),
            mode,
            base_rev,
            snapshot,
            changed,
            existing,
            deleted,
            inbase,
            added,
            all,
            base,
        })
    }

    /// Recomputes the added lines from the base copies (after formatters or fixes rewrote files).
    pub fn refresh_added(&mut self) {
        if self.mode == Mode::All {
            return;
        }
        for p in self.existing.clone() {
            let now = self.root.join(&p);
            let b = self.base.path().join(&p);
            let a = if b.is_file() {
                added_lines_files(&b, &now)
            } else {
                Added::Whole
            };
            self.added.insert(p, a);
        }
    }

    /// Replaces a file's added lines (after formatting the base copy: `forge_added_lines`).
    pub fn set_added(&mut self, path: &str, added: Added) {
        self.added.insert(path.to_string(), added);
    }
}

/// The lines of `after` that are added against `before` (1-based).
pub fn added_lines(before: &str, after: &str) -> BTreeSet<u32> {
    use imara_diff::{Algorithm, Diff, InternedInput};
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_lines(&input);
    let n = input.after.len() as u32;
    (0..n)
        .filter(|&i| diff.is_added(i))
        .map(|i| i + 1)
        .collect()
}

/// The lines of `after` that are removed or added against `before`, as `-text` / `+text`.
pub fn diff_lines(before: &str, after: &str) -> Vec<String> {
    use imara_diff::{Algorithm, Diff, InternedInput};
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_lines(&input);
    let mut out = Vec::new();
    for h in diff.hunks() {
        for i in h.before.clone() {
            let t = input.interner[input.before[i as usize]];
            out.push(format!("-{}", t.trim_end_matches(['\n', '\r'])));
        }
        for i in h.after.clone() {
            let t = input.interner[input.after[i as usize]];
            out.push(format!("+{}", t.trim_end_matches(['\n', '\r'])));
        }
    }
    out
}

fn added_lines_files(before: &Path, after: &Path) -> Added {
    let b = std::fs::read(before).unwrap_or_default();
    let a = std::fs::read(after).unwrap_or_default();
    Added::Lines(added_lines(
        &String::from_utf8_lossy(&b),
        &String::from_utf8_lossy(&a),
    ))
}

/// `dirty` of a turn snapshot: path → blob hash ("-": deleted then).
pub fn read_dirty(snap: &Path) -> HashMap<String, String> {
    let text = std::fs::read_to_string(snap.join("dirty")).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let (p, h) = l.split_once('\t')?;
            Some((
                p.to_string(),
                if h.is_empty() {
                    "-".into()
                } else {
                    h.to_string()
                },
            ))
        })
        .collect()
}

fn base_hashes(
    git: &Git,
    rev: Option<&str>,
    dirty: &HashMap<String, String>,
    paths: &[String],
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut ask = Vec::new();
    for p in paths {
        match dirty.get(p) {
            Some(h) => {
                out.insert(p.clone(), h.clone());
            }
            None => ask.push(p.clone()),
        }
    }
    if let Some(rev) = rev
        && !ask.is_empty()
    {
        out.extend(git.tree_blobs(rev, &ask));
    }
    for p in paths {
        out.entry(p.clone()).or_insert_with(|| "-".into());
    }
    out
}

fn copy_to(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::copy(src, dst).map(|_| ())
}

fn write_file(dst: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(dst, bytes)
}

pub(crate) fn split_z(bytes: &[u8]) -> impl Iterator<Item = String> + '_ {
    bytes
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
}

/// The git executable. On macOS `/usr/bin/git` is a shim that looks up the developer tools at
/// every call (about 20 ms); the binary it runs is used instead, found once (`git --exec-path`)
/// and kept in Forge's state while it exists.
pub fn git_exe() -> &'static Path {
    static EXE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    EXE.get_or_init(|| {
        if let Some(p) = std::env::var_os("FORGE_GIT").filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
        let on_path = std::env::var("PATH")
            .unwrap_or_default()
            .split(':')
            .map(|d| Path::new(d).join("git"))
            .find(|p| crate::tools::is_exe(p));
        let Some(git) = on_path else {
            return PathBuf::from("git");
        };
        if git != Path::new("/usr/bin/git") {
            return git;
        }
        let fact = forge_home::Home::from_env()
            .ok()
            .map(|h| h.state_root().join("facts").join("git-exe"));
        if let Some(f) = &fact
            && let Ok(p) = std::fs::read_to_string(f)
            && crate::tools::is_exe(Path::new(p.trim()))
        {
            return PathBuf::from(p.trim());
        }
        let real = Command::new(&git)
            .arg("--exec-path")
            .stderr(Stdio::null())
            .output()
            .ok()
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
            .and_then(|d| {
                // <prefix>/libexec/git-core → <prefix>/bin/git
                let bin = d.parent()?.parent()?.join("bin/git");
                crate::tools::is_exe(&bin).then_some(bin)
            });
        match real {
            Some(r) => {
                if let Some(f) = &fact {
                    let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new("/")));
                    let _ = std::fs::write(f, r.display().to_string());
                }
                r
            }
            None => git,
        }
    })
}

/// The git CLI in a repository.
pub struct Git {
    root: PathBuf,
}

impl Git {
    pub fn new(root: &Path) -> Git {
        Git {
            root: root.to_path_buf(),
        }
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(git_exe());
        c.current_dir(&self.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["-c", "core.quotepath=off"]);
        c
    }

    pub fn run(&self, args: &[&str]) -> Result<Vec<u8>, ChangeError> {
        let out = self
            .cmd()
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| ChangeError::Io(format!("git: {e}")))?;
        if !out.status.success() {
            return Err(ChangeError::Io(format!("git {} failed", args.join(" "))));
        }
        Ok(out.stdout)
    }

    fn run_input(&self, args: &[&str], input: &[u8]) -> Option<Vec<u8>> {
        let mut child = self
            .cmd()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take()?;
        let input = input.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let mut out = Vec::new();
        child.stdout.take()?.read_to_end(&mut out).ok()?;
        let _ = writer.join();
        child.wait().ok()?;
        Some(out)
    }

    pub fn head(&self) -> Option<String> {
        self.run(&["rev-parse", "-q", "--verify", "HEAD"])
            .ok()
            .map(|o| String::from_utf8_lossy(&o).trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Changed paths: staged, unstaged, untracked; renames as delete + add.
    pub fn status_paths(&self) -> Result<Vec<String>, ChangeError> {
        let out = self.run(&[
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ]);
        let out = match out {
            Ok(o) => o,
            Err(_) => return Err(ChangeError::NotARepository(self.root.clone())),
        };
        Ok(split_z(&out)
            .filter(|l| l.len() > 3)
            .map(|l| l[3..].to_string())
            .collect())
    }

    /// The blob hash of each existing path's working-tree content ("-" when absent).
    pub fn hash_now(&self, root: &Path, paths: &[String]) -> HashMap<String, String> {
        let present: Vec<&String> = paths.iter().filter(|p| root.join(p).is_file()).collect();
        let mut out: HashMap<String, String> =
            paths.iter().map(|p| (p.clone(), "-".to_string())).collect();
        if present.is_empty() {
            return out;
        }
        let input: String = present.iter().map(|p| format!("{p}\n")).collect();
        if let Some(o) = self.run_input(&["hash-object", "--stdin-paths"], input.as_bytes()) {
            for (p, h) in present.iter().zip(String::from_utf8_lossy(&o).lines()) {
                out.insert((*p).clone(), h.to_string());
            }
        }
        out
    }

    /// The blob hash of each path in `rev`'s tree (absent paths left out).
    pub fn tree_blobs(&self, rev: &str, paths: &[String]) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let input: String = paths.iter().map(|p| format!("{rev}:{p}\n")).collect();
        let Some(o) = self.run_input(
            &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
            input.as_bytes(),
        ) else {
            return out;
        };
        for (p, line) in paths.iter().zip(String::from_utf8_lossy(&o).lines()) {
            if let Some((h, "blob")) = line.split_once(' ') {
                out.insert(p.clone(), h.to_string());
            }
        }
        out
    }

    /// The content of each path in `rev`'s tree (absent paths left out): one `cat-file --batch`.
    pub fn cat_files(&self, rev: &str, paths: &[String]) -> Vec<(String, Vec<u8>)> {
        let input: String = paths.iter().map(|p| format!("{rev}:{p}\n")).collect();
        let Some(o) = self.run_input(&["cat-file", "--batch"], input.as_bytes()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut i = 0;
        for p in paths {
            let Some(nl) = o[i..].iter().position(|b| *b == b'\n') else {
                break;
            };
            let header = String::from_utf8_lossy(&o[i..i + nl]).to_string();
            i += nl + 1;
            let parts: Vec<&str> = header.split(' ').collect();
            if parts.len() == 3 {
                let size: usize = parts[2].parse().unwrap_or(0);
                let body = o[i..(i + size).min(o.len())].to_vec();
                i += size + 1;
                if parts[1] == "blob" {
                    out.push((p.clone(), body));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_lines_like_git() {
        assert_eq!(
            added_lines("a\nb\nc\n", "a\nx\nb\nc\ny\n"),
            BTreeSet::from([2, 5])
        );
        // an edited line counts as added
        assert_eq!(added_lines("a\nb\n", "a\nB\n"), BTreeSet::from([2]));
        // a last line that gains its newline is changed (as git says)
        assert_eq!(added_lines("a\nb", "a\nb\n"), BTreeSet::from([2]));
        assert!(added_lines("a\n", "a\n").is_empty());
    }

    #[test]
    fn diff_lines_signs() {
        assert_eq!(diff_lines("a\nb\n", "a\nc\n"), vec!["-b", "+c"]);
    }
}
