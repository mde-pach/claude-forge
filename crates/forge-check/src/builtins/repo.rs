//! Repository hygiene builtins of the checker: large files, lock files left behind, new files
//! beside the file they copy.

use std::io::Write;
use std::process::{Command, Stdio};

use super::machinery::package_deps;
use super::{Invocation, Registry};
use crate::change::diff_lines;

pub(super) fn register(r: &mut Registry) {
    r.register_raw("large_files", large_files);
    r.register_raw("lockfile_updated", lockfile_updated);
    r.register_raw("new_files", new_files);
}

/// `git check-attr <attr>` of a path.
fn attr(root: &std::path::Path, name: &str, path: &str) -> String {
    let out = Command::new(crate::change::git_exe())
        .current_dir(root)
        .args(["check-attr", name, "--", path])
        .stderr(Stdio::null())
        .output();
    out.ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .and_then(|s| s.rsplit_once(": ").map(|(_, v)| v.trim().to_string()))
        .unwrap_or_default()
}

/// `forge_large_files <bytes>`: each in-scope file over the limit that the change added or took
/// past it (its base copy was within it), that is not a lock file (the `lockfiles` role, or a
/// lock file's name anywhere) and that Git LFS does not store. A file already over the limit
/// that grows is not the change's.
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
            && std::fs::metadata(b).map(|b| b.len()).unwrap_or(0) > max
        {
            continue;
        }
        let name = f.rsplit('/').next().unwrap_or(&f);
        if inv.roles.matches("lockfiles", &f) || JS_LOCKS.contains(&name) || name == "uv.lock" {
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

/// The package manager a lock file is of, and the command that updates it.
fn lock_pm(lock: &str) -> (&'static str, &'static str) {
    match lock.rsplit('/').next().unwrap_or(lock) {
        "pnpm-lock.yaml" => ("pnpm", "pnpm install"),
        "yarn.lock" => ("yarn", "yarn install"),
        "bun.lock" | "bun.lockb" => ("bun", "bun install"),
        _ => ("npm", "npm install"),
    }
}

/// `forge_lockfile_updated`: a package.json whose dependencies changed since the base needs a
/// lock file whose changed lines name each changed dependency, when the project has a lock
/// (next to it or at the root). A lock its package manager's lock-sync check keeps in sync
/// (`typescript/<pm>/lock-sync`, §10.5) is left to that check.
fn lockfile_updated(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let root = inv.root();
    let mut rc = 0;
    let mut commands: Vec<&str> = Vec::new();
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
        let (pm, command) = lock_pm(&lock);
        let sync = format!("typescript/{pm}/lock-sync");
        if inv.checks.contains(&sync) {
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
        if !commands.contains(&command) {
            commands.push(command);
        }
        rc = 1;
    }
    if rc == 1 {
        let _ = writeln!(
            out,
            "Run `{}` to update the lock file; it is not edited by hand. If it cannot run here, tell the user.",
            commands.join("` or `")
        );
    }
    rc
}

/// The new files among `files` (`in_base` is false), each with the existing file in its
/// directory that its name names once `beside` (a regular expression and its replacement) is
/// applied to it; without `beside`, every new file. With `beside`, a file whose name the
/// expression does not rewrite, or rewrites to no existing file, is left out.
fn new_files_beside(
    files: &[String],
    in_base: impl Fn(&str) -> bool,
    exists: impl Fn(&str) -> bool,
    beside: Option<(&regex::Regex, &str)>,
) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    for f in files {
        if in_base(f) {
            continue;
        }
        let Some((re, rep)) = beside else {
            found.push((f.clone(), None));
            continue;
        };
        let (dir, name) = match f.rsplit_once('/') {
            Some((d, n)) => (format!("{d}/"), n),
            None => (String::new(), f.as_str()),
        };
        let other = re.replace(name, rep);
        if other.is_empty() || other == name {
            continue;
        }
        let other = format!("{dir}{other}");
        if exists(&other) {
            found.push((f.clone(), Some(other)));
        }
    }
    found
}

/// `forge_new_files [--beside <regex> <replacement>] [message]`: each of the check's files that
/// the change adds (it has no copy at the base), as `path:0 message`; with `--beside`, only one
/// whose name, rewritten by the expression, names a file that exists in the same directory
/// (`{beside}` in the message). Without a base (`--all`), nothing: files already in the
/// repository are not new.
fn new_files(inv: &Invocation<'_>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut args = inv.args.as_slice();
    let mut beside = None;
    if args.first().map(String::as_str) == Some("--beside") {
        let (Some(re), Some(rep)) = (args.get(1), args.get(2)) else {
            let _ = writeln!(
                err,
                "forge_new_files: --beside takes a regex and a replacement"
            );
            return 2;
        };
        match regex::Regex::new(re) {
            Ok(r) => beside = Some((r, rep.clone())),
            Err(e) => {
                let _ = writeln!(err, "forge_new_files: {e}");
                return 2;
            }
        }
        args = &args[3..];
    }
    let message = args.first().map_or("is a new file", String::as_str);
    if inv.ctx.scope == forge_core::builtin::Scope::All || inv.ctx.base.is_none() {
        return 0;
    }
    let files: Vec<String> = inv
        .scope_files()
        .into_iter()
        .filter(|f| inv.ctx.changed.contains(f))
        .collect();
    let root = inv.root();
    let found = new_files_beside(
        &files,
        |f| inv.base_copy(f).is_some(),
        |f| root.join(f).is_file(),
        beside.as_ref().map(|(r, s)| (r, s.as_str())),
    );
    for (f, other) in &found {
        let text = message.replace("{beside}", other.as_deref().unwrap_or(""));
        let _ = writeln!(out, "{f}:0 {text}");
    }
    i32::from(!found.is_empty())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use forge_core::builtin::{BuiltinCtx, Scope};

    use super::super::CheckCtx;
    use super::*;

    /// A file new over the limit, or taken past it, is reported; one already over it that
    /// grows, and a lock file, are not.
    #[test]
    fn large_files_reports_what_the_change_took_past_the_limit() {
        let root = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        let put = |d: &std::path::Path, p: &str, n: usize| {
            let f = d.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, "x".repeat(n)).unwrap();
        };
        put(root.path(), "new.bin", 200);
        put(root.path(), "grown.bin", 200);
        put(base.path(), "grown.bin", 50);
        put(root.path(), "big.json", 300);
        put(base.path(), "big.json", 200);
        put(root.path(), "web/package-lock.json", 300);
        put(root.path(), "pnpm-lock.yaml", 300);
        put(base.path(), "pnpm-lock.yaml", 50);
        let files: Vec<String> = [
            "new.bin",
            "grown.bin",
            "big.json",
            "web/package-lock.json",
            "pnpm-lock.yaml",
        ]
        .map(String::from)
        .to_vec();
        let registry = Registry::new();
        let inv = Invocation {
            ctx: BuiltinCtx {
                root: root.path().to_path_buf(),
                base: Some(base.path().to_path_buf()),
                changed: files.clone(),
                rule: "git/large-files".into(),
                scope: Scope::Changed,
                accepted: vec![],
                notes_file: None,
            },
            cx: CheckCtx {
                files,
                ..CheckCtx::default()
            },
            args: vec!["100".into()],
            registry: &registry,
            tools: Arc::new(crate::tools::LocalTools::default()),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(large_files(&inv, &mut out, &mut err), 1);
        let out = String::from_utf8(out).unwrap();
        let files: Vec<&str> = out.lines().filter_map(|l| l.split(':').next()).collect();
        assert_eq!(files, ["new.bin", "grown.bin"], "{out}");
    }

    // a versioned copy is reported only when the file it copies exists beside it
    #[test]
    fn new_files_beside_their_original() {
        let re = regex::Regex::new(
            r"^(.+?)(?:[-_ ](?:v[0-9]+|new|old|copy|final|backup)|[ ]copy[ ][0-9]+)((?:\.[A-Za-z0-9]+)*)$|^(.+)\.(?:bak|orig)$",
        )
        .unwrap();
        let rep = "${1}${2}${3}";
        let files: Vec<String> = [
            "src/api-v2.ts",
            "src/db_backup.py",
            "src/whats-new.tsx",
            "src/Button copy 2.tsx",
            "src/form.test.ts.bak",
            "src/old.ts",
            "src/user_v2.py",
            "src/oauth2.ts",
        ]
        .map(String::from)
        .to_vec();
        let exists = |p: &str| {
            [
                "src/api.ts",
                "src/Button.tsx",
                "src/form.test.ts",
                "src/user.py",
                "src/oauth.ts",
            ]
            .contains(&p)
        };
        let in_base = |p: &str| p == "src/user_v2.py";
        let found = new_files_beside(&files, in_base, exists, Some((&re, rep)));
        assert_eq!(
            found,
            vec![
                ("src/api-v2.ts".into(), Some("src/api.ts".into())),
                (
                    "src/Button copy 2.tsx".into(),
                    Some("src/Button.tsx".into())
                ),
                (
                    "src/form.test.ts.bak".into(),
                    Some("src/form.test.ts".into())
                ),
            ]
        );
        // without --beside, every new file
        assert_eq!(new_files_beside(&files, in_base, exists, None).len(), 7);
    }
}
