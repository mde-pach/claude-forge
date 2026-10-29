//! PreToolUse: the refusal of Forge's internal files, the Bash guard and snapshot, the test-edit
//! ask.

use std::path::{Path, PathBuf};

use forge_core::{Note, NoteKind};
use forge_guard::parse::{self, Dir, Parsed, Start};
use forge_guard::{GuardCtx, Outcome, PythonPm};

use crate::input::HookInput;
use crate::review::{self, Ask};
use crate::rules::{self, HookCache};
use crate::snapshot::BashSnapshots;
use crate::{INTERNAL_REASON, Output, Runtime, pre_decision, testedit};

pub fn pre_tool_use(rt: &Runtime, input: &HookInput, user: bool) -> Output {
    let cwd = rt.cwd(input);
    let tool = input.tool();
    let parsed = match (tool, input.command()) {
        ("Bash", Some(cmd)) => Some(parse::parse(
            cmd,
            &Start {
                cwd: Dir::Known(parse::normalize(&cwd)),
                home: rt.env.home_dir.clone(),
            },
        )),
        _ => None,
    };
    if names_internal(rt, input, &cwd, parsed.as_ref()) {
        return pre_decision("deny", INTERNAL_REASON);
    }
    match (tool, parsed) {
        ("Bash", Some(parsed)) => bash(rt, input, &cwd, &parsed, user),
        ("Edit" | "Write" | "MultiEdit", _) if !user => testedit::pre(rt, input, &cwd),
        _ => Output::silent(),
    }
}

fn bash(rt: &Runtime, input: &HookInput, cwd: &Path, parsed: &Parsed, user: bool) -> Output {
    let command = input.command().unwrap_or("");
    let project = rt.project(input);
    let (cache, root) = if user {
        let Some(lock) = rt.user_lock() else {
            return Output::silent();
        };
        let cache = rules::load(
            &rt.user_state(),
            &rules::Source {
                lock,
                repo: None,
                packs_cache: Some(rt.home.packs_dir()),
            },
        );
        let root = project
            .as_ref()
            .map(|p| p.root.clone())
            .or_else(|| git_root(cwd))
            .unwrap_or_else(|| cwd.to_path_buf());
        (cache, root)
    } else {
        let Some(p) = project.as_ref() else {
            return Output::silent();
        };
        (p.rules(&rt.home), p.root.clone())
    };
    // the project's facts (package managers) also serve the user-level rules
    let facts: Option<HookCache> = if user {
        project.as_ref().and_then(|p| {
            std::fs::read(p.state.join("hook-cache.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
        })
    } else {
        cache.clone()
    };
    let outcome = match cache.as_ref().and_then(HookCache::guard_set) {
        Some(set) => {
            let ctx = GuardCtx::new(&root, cwd)
                .with_home(rt.env.home_dir.clone())
                .with_python_pm(
                    facts
                        .as_ref()
                        .and_then(|f| f.python_pm.as_deref())
                        .and_then(PythonPm::from_fact),
                )
                .with_js_pm(rules::js_pm(
                    facts.as_ref().and_then(|f| f.js_pm.as_deref()),
                ));
            set.evaluate_parsed(parsed, command, &ctx)
        }
        None => Outcome::None,
    };
    let answer = match &outcome {
        Outcome::Deny(v) => return pre_decision("deny", &v.reason),
        Outcome::Ask(v) => {
            let ask = Ask {
                rule: &v.rule,
                concern: &v.reason,
                question: v.review.as_deref(),
                request: None,
            };
            let state = project
                .as_ref()
                .map_or_else(|| rt.user_state(), |p| p.state.clone());
            let session_dir = project.as_ref().map(|p| p.session_dir.clone());
            match review::review_ask(rt, input, &ask, &state, session_dir.as_deref(), cwd) {
                Some(out) => out,
                None => pre_decision("ask", &v.reason),
            }
        }
        Outcome::None => Output::silent(),
    };
    if !user {
        if let Some(p) = &project {
            bash_snapshot(rt, input, p);
        }
    }
    answer
}

/// Snapshots the working tree before a Bash command, for the PostToolUse checks.
fn bash_snapshot(rt: &Runtime, input: &HookInput, p: &crate::Project) {
    let Some(key) = input.tool_key() else { return };
    let snaps = BashSnapshots::new(&p.root, &p.session_dir, &p.state);
    if !crate::state::ensure_dir(&snaps.dir) {
        return;
    }
    snaps.cleanup(None);
    let tree = snaps.tree(&key, true, rt.env.snapshot_cap);
    let text = match &tree {
        Some(t) => t.clone(),
        None => {
            p.note(&[Note::new(
                NoteKind::Environment,
                format!(
                    "The working tree could not be snapshotted within {}s, so the files Bash commands change are not checked after each command; the Stop check still covers them. Set FORGE_SNAPSHOT_TIMEOUT to allow more time.",
                    rt.env.snapshot_cap.as_secs()
                ),
            )]);
            "skipped".into()
        }
    };
    crate::state::write_atomic(&snaps.tree_file(&key), format!("{text}\n").as_bytes());
}

fn git_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Whether the tool call names a file in Forge's cache or state.
fn names_internal(rt: &Runtime, input: &HookInput, cwd: &Path, parsed: Option<&Parsed>) -> bool {
    let check = |s: &str, base: &Path| -> bool {
        match expand(s, base, rt.env.home_dir.as_deref()) {
            Some(p) => rt.home.is_internal_path(&p),
            None => false,
        }
    };
    match input.tool() {
        "Read" | "Edit" | "Write" | "MultiEdit" => {
            input.input_str("file_path").is_some_and(|p| check(p, cwd))
        }
        "NotebookEdit" | "NotebookRead" => input
            .input_str("notebook_path")
            .is_some_and(|p| check(p, cwd)),
        "Grep" | "Glob" => {
            let base = input
                .input_str("path")
                .and_then(|p| expand(p, cwd, rt.env.home_dir.as_deref()))
                .unwrap_or_else(|| cwd.to_path_buf());
            rt.home.is_internal_path(&base)
                || input
                    .input_str("pattern")
                    .is_some_and(|p| (p.starts_with('/') || p.starts_with('~')) && check(p, &base))
        }
        "Bash" => {
            let Some(cmd) = input.command() else {
                return false;
            };
            let roots = [rt.home.cache_root(), rt.home.state_root()];
            let mentions = cmd.contains("forge")
                || roots
                    .iter()
                    .any(|r| r.to_str().is_some_and(|r| cmd.contains(r)));
            if !mentions {
                return false;
            }
            let Some(parsed) = parsed else { return false };
            parsed.commands.iter().any(|c| {
                let base = c.dir.known().unwrap_or(cwd);
                c.words
                    .iter()
                    .map(|w| w.text.as_str())
                    .chain(c.redirects.iter().map(|r| r.target.text.as_str()))
                    .any(|w| {
                        let w = w.rsplit_once('=').map_or(w, |(_, v)| v);
                        (w.contains('/') || w.starts_with('~')) && check(w, base)
                    })
            })
        }
        _ => false,
    }
}

/// `s` as an absolute path: `~` and `$HOME` expanded, relative to `base`.
fn expand(s: &str, base: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    for pre in ["~/", "$HOME/", "${HOME}/"] {
        if let Some(rest) = s.strip_prefix(pre) {
            return Some(home?.join(rest));
        }
    }
    if s == "~" || s == "$HOME" {
        return home.map(Path::to_path_buf);
    }
    if s.starts_with('$') || s.starts_with('~') {
        return None;
    }
    let p = Path::new(s);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    })
}
