//! `pre-turn-changes`: a `git checkout` / `git restore` that discards working-tree changes,
//! judged against the turn snapshot (§10.2). Discarding a path that held uncommitted changes
//! before the turn loses the person's own work, which nothing can bring back: the check finds
//! it (the rule's deny). Paths changed only during the turn are the agent's own work: the check
//! finds nothing, and the rule's ask decides (a reviewer, given the user's request, when nobody
//! is there). When the turn is not known (no snapshot, or a command whose paths cannot be
//! known), the check finds it, as the rule decided before turns were known.

use std::path::PathBuf;

use crate::args::Args;
use crate::ctx::{GuardCtx, pre_turn_changes};
use crate::parse::{self, SimpleCommand};

/// git's options that take a value, besides its global ones, for `checkout` and `restore`.
const VALUE: &[&str] = &[
    "-s",
    "--source",
    "--conflict",
    "--pathspec-from-file",
    "-b",
    "-B",
    "--orphan",
    "--unified",
    "-U",
    "--inter-hunk-context",
];

/// What a matched command discards: the directories and files its pathspecs name, relative to
/// the repository root ("" is the whole repository). None when that cannot be known (a
/// directory not known in advance, `--pathspec-from-file`, a path outside the repository).
fn discarded(cmd: &SimpleCommand, ctx: &GuardCtx) -> Option<Vec<String>> {
    let p = *cmd.starts.iter().find(|&&i| cmd.name_at(i) == "git")?;
    let mut dir: PathBuf = cmd.dir.known()?.to_path_buf();
    let words = &cmd.words[p + 1..];
    let args = Args::read(
        "git",
        words,
        &VALUE.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    if args
        .options
        .iter()
        .any(|(o, _)| *o == "--pathspec-from-file")
    {
        return None;
    }
    // `git -C <dir>` before the subcommand
    for (o, v) in &args.options {
        if *o == "-C" {
            dir = dir.join((*v)?);
        }
    }
    let sub = *args.positionals.first()?;
    let specs: Vec<&str> = match words.iter().position(|w| w.text == "--") {
        Some(i) => words[i + 1..].iter().map(|w| w.text.as_str()).collect(),
        None => {
            let rest = &args.positionals[1..];
            match sub {
                // `git checkout <ref> <path>…`, `git checkout .`
                "checkout" if rest.len() > 1 => rest[1..].to_vec(),
                _ => rest.to_vec(),
            }
        }
    };
    let root = parse::normalize(&ctx.root);
    let mut out = Vec::new();
    for spec in specs {
        // pathspec magic (`:/`, `:(glob)…`): the whole repository
        if spec.starts_with(':') {
            out.push(String::new());
            continue;
        }
        // a glob: the directory before its first wildcard
        let literal = match spec.find(['*', '?', '[']) {
            Some(i) => spec[..i].rsplit_once('/').map_or("", |(d, _)| d),
            None => spec,
        };
        let abs = parse::normalize(&dir.join(literal));
        let rel = abs.strip_prefix(&root).ok()?;
        out.push(rel.to_string_lossy().into_owned());
    }
    Some(out)
}

/// Whether `path` is `spec` or below it ("" is everything).
fn covers(spec: &str, path: &str) -> bool {
    spec.is_empty()
        || path == spec
        || path
            .strip_prefix(spec)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The detail of a deny: the first pre-turn path a matched command discards, or "" when the turn
/// or the paths are not known. None: nothing from before the turn is discarded.
pub fn check(matched: &[&SimpleCommand], ctx: &GuardCtx) -> Option<String> {
    let Some(before) = ctx.turn.as_deref().and_then(pre_turn_changes) else {
        return Some(String::new());
    };
    for cmd in matched {
        let Some(specs) = discarded(cmd, ctx) else {
            return Some(String::new());
        };
        if let Some(p) = before.iter().find(|p| specs.iter().any(|s| covers(s, p))) {
            return Some(format!(
                "{p} has uncommitted changes from before this task, which this would discard"
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_covers_itself_and_what_is_below_it() {
        assert!(covers("", "a.py"));
        assert!(covers("src", "src/a.py"));
        assert!(covers("src/a.py", "src/a.py"));
        assert!(!covers("src", "srcx/a.py"));
        assert!(!covers("src/a.py", "src/a.pyc"));
    }
}
