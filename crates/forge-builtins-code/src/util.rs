//! Small shared pieces: git, notes, tool output.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use forge_core::builtin::BuiltinCtx;
use forge_core::{Note, NoteKind};

/// `git <args>` in `root`; None when it cannot run or fails.
pub fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The non-empty lines git prints.
pub fn git_lines(root: &Path, args: &[&str]) -> Vec<String> {
    git(root, args)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `git check-attr <attr>` of a path: its value ("unspecified" when unset).
pub fn attr(root: &Path, name: &str, path: &str) -> String {
    git(root, &["check-attr", name, "--", path])
        .and_then(|s| {
            s.lines()
                .next()
                .and_then(|l| l.rsplit_once(": ").map(|(_, v)| v.trim().to_string()))
        })
        .unwrap_or_default()
}

/// A note for the person about the running rule.
pub fn note(ctx: &BuiltinCtx, kind: NoteKind, text: impl Into<String>) -> Note {
    let n = Note::new(kind, text);
    if ctx.rule.is_empty() {
        n
    } else {
        n.for_rule(ctx.rule.clone())
    }
}

/// The last non-blank line of a tool's output (stderr first), without ANSI colours.
pub fn last_line(out: &Output) -> String {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").expect("regex");
    ansi.replace_all(&text, "")
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or("no output")
        .trim()
        .to_string()
}
