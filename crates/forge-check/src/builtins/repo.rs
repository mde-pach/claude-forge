//! Repository hygiene builtins of the checker: large files, lock files left behind.

use std::io::Write;
use std::process::{Command, Stdio};

use super::machinery::package_deps;
use super::{Invocation, Registry};
use crate::change::diff_lines;

pub(super) fn register(r: &mut Registry) {
    r.register_raw("large_files", large_files);
    r.register_raw("lockfile_updated", lockfile_updated);
}

/// `git check-attr <attr>` of a path.
fn attr(root: &std::path::Path, name: &str, path: &str) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(["check-attr", name, "--", path])
        .stderr(Stdio::null())
        .output();
    out.ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .and_then(|s| s.rsplit_once(": ").map(|(_, v)| v.trim().to_string()))
        .unwrap_or_default()
}

/// `forge_large_files <bytes>`: each in-scope file over the limit that is new or grew in the
/// change, and that Git LFS does not store.
fn large_files(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let max: u64 = inv
        .args
        .first()
        .and_then(|a| a.parse().ok())
        .unwrap_or(1 << 20);
    let mut rc = 0;
    for f in inv.scope_files() {
        let Ok(m) = std::fs::metadata(inv.root().join(&f)) else {
            continue;
        };
        if !m.is_file() || m.len() <= max {
            continue;
        }
        if let Some(b) = inv.base_copy(&f)
            && std::fs::metadata(b).map(|b| b.len()).unwrap_or(0) >= m.len()
        {
            continue;
        }
        if attr(inv.root(), "filter", &f) == "lfs" {
            continue;
        }
        let _ = writeln!(
            out,
            "{f}:0 is {} KiB, over the {} KiB limit, and Git LFS does not store it: keep it out of the repository or track it with git lfs track",
            m.len() / 1024,
            max / 1024
        );
        rc = 1;
    }
    rc
}

/// The lock files of the JavaScript package managers, in the order they are looked for.
const JS_LOCKS: &[&str] = &[
    "pnpm-lock.yaml",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
];

/// `forge_lockfile_updated`: a package.json whose dependencies changed since the base needs a
/// lock file whose changed lines name each changed dependency, when the project has a pnpm,
/// npm or Yarn lock (next to it or at the root). bun.lock is left to the bun lock-sync check
/// when it is active.
fn lockfile_updated(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let root = inv.root();
    let mut rc = 0;
    for f in inv.existing() {
        if !(f == "package.json" || f.ends_with("/package.json"))
            || format!("/{f}").contains("/node_modules/")
        {
            continue;
        }
        let dir = std::path::Path::new(&f)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut lock = None;
        for l in JS_LOCKS {
            let near = if dir.is_empty() {
                l.to_string()
            } else {
                format!("{dir}/{l}")
            };
            if root.join(&near).is_file() {
                lock = Some(near);
                break;
            }
            if root.join(l).is_file() {
                lock = Some(l.to_string());
                break;
            }
        }
        let Some(lock) = lock else { continue };
        if (lock.ends_with("bun.lock") || lock.ends_with("bun.lockb"))
            && inv.checks.iter().any(|c| c == "typescript/bun/lock-sync")
        {
            continue;
        }
        let then: Vec<String> = inv
            .base_copy(&f)
            .map(|b| package_deps(&b))
            .unwrap_or_default();
        let now = package_deps(&root.join(&f));
        if then == now {
            continue;
        }
        let parse = |v: &[String]| -> std::collections::BTreeMap<String, (String, String)> {
            v.iter()
                .filter_map(|l| {
                    let mut p = l.splitn(3, '\t');
                    let (s, n, spec) = (p.next()?, p.next()?, p.next()?);
                    Some((format!("{s} {n}"), (n.to_string(), spec.to_string())))
                })
                .collect()
        };
        let (t, n) = (parse(&then), parse(&now));
        let lock_then = inv
            .base_copy(&lock)
            .and_then(|b| std::fs::read_to_string(b).ok())
            .unwrap_or_default();
        let lock_now = std::fs::read_to_string(root.join(&lock)).unwrap_or_default();
        let diff = if inv.ctx.changed.contains(&lock) {
            diff_lines(&lock_then, &lock_now).join("\n")
        } else {
            String::new()
        };
        let mut missing = Vec::new();
        for (k, (name, spec)) in &n {
            let w = match t.get(k) {
                None => format!("{k} {spec} added"),
                Some((_, old)) if old != spec => format!("{k} {old} -> {spec}"),
                _ => continue,
            };
            if !diff.contains(name.as_str()) {
                missing.push(w);
            }
        }
        for (k, (name, _)) in &t {
            if !n.contains_key(k) && !diff.contains(name.as_str()) {
                missing.push(format!("{k} removed"));
            }
        }
        missing.sort();
        if missing.is_empty() {
            continue;
        }
        let what = missing
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        let _ = writeln!(out, "{f}: {what}, but {lock} has no changed line for it");
        rc = 1;
    }
    if rc == 1 {
        let _ = writeln!(
            out,
            "Run the package manager (bun install, pnpm install, npm install, yarn) to update the lock file; it is not edited by hand. If it cannot run here, tell the user."
        );
    }
    rc
}
