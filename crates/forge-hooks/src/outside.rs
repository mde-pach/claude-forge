//! Files the agent writes outside the repository (a script in `/tmp`, a scratch test): the
//! project's rules apply to them too, since they follow the agent, not the repository's root.
//!
//! The checker works on a git repository and repository-relative paths, so these files are
//! checked in a mirror: a session-owned git repository whose top-level entries link to the
//! project's (its configuration, its code), committed once, with a copy of each outside file at
//! `_outside/<absolute path>`. A check there sees the copies as new files and the project's
//! configuration as its own; findings are given back under the real path, and what a fixer or
//! formatter changed in a copy is written back to the real file, as Forge does in the repository.

use std::path::{Path, PathBuf};

use forge_core::Report;

use crate::checker::StopOutcome;
use crate::proc::{GIT_CAP, git, git_env};
use crate::{Project, Runtime, snapshot, state};

/// The mirror's directory of copies; a copy's path is this, then the real absolute path.
const PREFIX: &str = forge_detect::facts::OUTSIDE_DIR;

/// The session's mirror (`<session>/outside`).
fn dir(p: &Project) -> PathBuf {
    p.session_dir.join("outside")
}

/// Whether `abs`, outside the repository, is the agent's own work: not Claude Code's or Forge's
/// files, nor a directory the rules never look in.
pub(crate) fn applies(rt: &Runtime, p: &Project, abs: &Path) -> bool {
    // a large output (a log) is not code the rules are about, and is not copied
    let small = std::fs::metadata(abs).is_ok_and(|m| m.is_file() && m.len() <= snapshot::COPY_MAX);
    if !abs.is_absolute() || abs.starts_with(&p.root) || !small {
        return false;
    }
    let s = abs.to_string_lossy();
    if s.contains('\n') || snapshot::excluded(s.trim_start_matches('/')) {
        return false;
    }
    if rt.home.is_internal_path(abs) {
        return false;
    }
    let mut own: Vec<PathBuf> = vec![
        rt.home.cache_root().to_path_buf(),
        rt.home.state_root().to_path_buf(),
        p.state.clone(),
    ];
    if let Some(d) = forge_render::lock::claude_config_dir(
        rt.env.claude_config_dir.clone(),
        rt.env.home_dir.clone(),
    ) {
        own.push(d);
    }
    !own.iter().any(|d| abs.starts_with(d))
}

/// The files a Bash command writes through a redirection (`cat > /tmp/x.py <<EOF`, `>>`), as
/// absolute paths; a target or directory known only when the command runs is left out.
pub(crate) fn bash_targets(rt: &Runtime, command: &str, cwd: &Path) -> Vec<PathBuf> {
    use forge_guard::parse::{self, Dir, Start};
    let parsed = parse::parse(
        command,
        &Start {
            cwd: Dir::Known(parse::normalize(cwd)),
            home: rt.env.home_dir.clone(),
        },
    );
    let mut out: Vec<PathBuf> = Vec::new();
    for c in &parsed.commands {
        for r in c.redirects.iter().filter(|r| r.writes_file()) {
            if r.target.dynamic || r.target.text.is_empty() {
                continue;
            }
            let t = Path::new(&r.target.text);
            let abs = if t.is_absolute() {
                t.to_path_buf()
            } else {
                match c.dir.known() {
                    Some(d) => d.join(t),
                    None => continue,
                }
            };
            out.push(parse::normalize(&abs));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The copy's repository-relative path in the mirror.
fn virtual_path(abs: &Path) -> String {
    format!("{PREFIX}{}", abs.display())
}

/// The real path of a mirror path, when it is a copy.
fn real_path(virt: &str) -> Option<PathBuf> {
    virt.strip_prefix(PREFIX)
        .filter(|r| r.starts_with('/'))
        .map(PathBuf::from)
}

/// The mirror, made on first use: a git repository linking each top-level entry of the project
/// (but `.git`), committed, so that only the copies are a change.
fn mirror(p: &Project) -> Option<PathBuf> {
    let m = dir(p);
    if m.join(".git").is_dir() {
        return Some(m);
    }
    let _ = std::fs::remove_dir_all(&m);
    if !state::ensure_dir(&m) {
        return None;
    }
    for e in std::fs::read_dir(&p.root).ok()?.flatten() {
        let name = e.file_name();
        if name == ".git" || name == PREFIX {
            continue;
        }
        std::os::unix::fs::symlink(e.path(), m.join(&name)).ok()?;
    }
    let who: [(&str, &std::ffi::OsStr); 4] = [
        ("GIT_AUTHOR_NAME", "forge".as_ref()),
        ("GIT_AUTHOR_EMAIL", "forge@localhost".as_ref()),
        ("GIT_COMMITTER_NAME", "forge".as_ref()),
        ("GIT_COMMITTER_EMAIL", "forge@localhost".as_ref()),
    ];
    let ok = git(&m, &["init", "-q"], GIT_CAP).is_some()
        && git(&m, &["add", "-A"], GIT_CAP).is_some()
        && git_env(
            &m,
            &[
                "commit",
                "-q",
                "--no-verify",
                "--allow-empty",
                "-m",
                "project",
            ],
            &who,
            None,
            GIT_CAP,
        )
        .is_some_and(|r| r.ok);
    if !ok {
        let _ = std::fs::remove_dir_all(&m);
        return None;
    }
    Some(m)
}

/// A new turn: the copies of the last one are dropped.
pub(crate) fn new_turn(p: &Project) {
    let _ = std::fs::remove_dir_all(dir(p).join(PREFIX));
}

/// Copies `abs` into the mirror; its mirror path and the bytes copied.
fn copy_in(m: &Path, abs: &Path) -> Option<(String, Vec<u8>)> {
    let virt = virtual_path(abs);
    let bytes = std::fs::read(abs).ok()?;
    let dst = m.join(&virt);
    state::ensure_dir(dst.parent()?);
    std::fs::write(&dst, &bytes).ok()?;
    Some((virt, bytes))
}

/// The copies of this turn, refreshed from their real files (a deleted one is dropped).
fn refresh(m: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![m.join(PREFIX)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path);
                continue;
            }
            let Some(virt) = path
                .strip_prefix(m)
                .ok()
                .map(|r| r.to_string_lossy().into_owned())
            else {
                continue;
            };
            match real_path(&virt).filter(|r| r.is_file()) {
                Some(real) => {
                    if let Some(c) = copy_in(m, &real) {
                        out.push(c);
                    }
                }
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    out.sort();
    out
}

/// What the checks changed in the copies goes back to the real files.
fn write_back(m: &Path, copies: &[(String, Vec<u8>)]) {
    for (virt, before) in copies {
        let Ok(now) = std::fs::read(m.join(virt)) else {
            continue;
        };
        if now != *before
            && let Some(real) = real_path(virt)
        {
            let _ = std::fs::write(real, now);
        }
    }
}

/// Findings and notes under the real paths.
fn to_real(mut report: Report) -> Report {
    let fix = |s: &str| s.replace(&format!("{PREFIX}/"), "/");
    for f in &mut report.findings {
        if let Some(path) = &f.path {
            f.path = Some(fix(path));
        }
        f.message = fix(&f.message);
    }
    for n in &mut report.notes {
        n.text = fix(&n.text);
    }
    report
}

/// The edit-time checks of the outside files the agent just wrote.
pub(crate) fn post_edit(rt: &Runtime, p: &Project, files: &[PathBuf]) -> Report {
    let Some(m) = mirror(p) else {
        return Report::default();
    };
    let copies: Vec<(String, Vec<u8>)> = files.iter().filter_map(|f| copy_in(&m, f)).collect();
    if copies.is_empty() {
        return Report::default();
    }
    let virt: Vec<String> = copies.iter().map(|(v, _)| v.clone()).collect();
    let report = rt.checker.post_edit(&m, &virt, None);
    write_back(&m, &copies);
    to_real(report)
}

/// The Stop checks of the turn's outside files; None when the turn wrote none.
pub(crate) fn stop(rt: &Runtime, p: &Project) -> Option<StopOutcome> {
    let m = dir(p);
    if !m.join(PREFIX).is_dir() || !m.join(".git").is_dir() {
        return None;
    }
    let copies = refresh(&m);
    if copies.is_empty() {
        return None;
    }
    let mut outcome = rt.checker.stop(&m, None);
    write_back(&m, &copies);
    outcome.report = to_real(outcome.report);
    Some(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_maps_back_to_its_real_path() {
        let v = virtual_path(Path::new("/tmp/suite/stress.py"));
        assert_eq!(v, "_outside/tmp/suite/stress.py");
        assert_eq!(real_path(&v), Some(PathBuf::from("/tmp/suite/stress.py")));
        assert_eq!(real_path("app/main.py"), None);
        assert_eq!(real_path("_outsider/x.py"), None);
    }
}
