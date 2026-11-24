//! PreToolUse: the refusal of Forge's internal files, the Bash guard and snapshot, the test-edit
//! ask.

use std::path::{Path, PathBuf};

use forge_core::{Note, NoteKind};
use forge_guard::parse::{self, Dir, Parsed, Start};
use forge_guard::{GuardCtx, Outcome, PythonPm};

use crate::agents::{self, Who};
use crate::input::HookInput;
use crate::review::{self, Ask};
use crate::rules::{self, HookCache};
use crate::snapshot::BashSnapshots;
use crate::workflow::StepHooks;
use crate::{INTERNAL_REASON, Output, RUN_REASON, Runtime, pre_decision, testedit};
use forge_core::serena;

pub fn pre_tool_use(rt: &Runtime, input: &HookInput, user: bool) -> Output {
    let cwd = rt.cwd(input);
    let tool = input.tool();
    // Forge's own agents: a step gets the hooks its workflow declares; the reviewer, the internals
    // refusal; neither edits, so the test-edit ask never applies
    let me = agents::who(input);
    let hooks = match me {
        Who::Step => rt
            .project(input)
            .and_then(|p| agents::step_hooks(rt, &p))
            .unwrap_or_default(),
        // the reviewer has every tool of the session: its commands go through the guard
        Who::Reviewer => StepHooks {
            guard: true,
            internals: true,
            checks: false,
        },
        _ => StepHooks {
            guard: true,
            internals: true,
            checks: true,
        },
    };
    let mine = matches!(me, Who::Step | Who::Reviewer);
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
    if hooks.internals && names_internal(rt, input, &cwd, parsed.as_ref()) {
        return pre_decision("deny", INTERNAL_REASON);
    }
    if hooks.internals && parsed.as_ref().is_some_and(|p| runs_forge(rt, p)) {
        return pre_decision("deny", RUN_REASON);
    }
    // a run of searches and reads of code without Serena: reminded once (§17.8)
    if !user {
        if let Some(out) = rt
            .project(input)
            .and_then(|p| crate::serena_remind::check(&p, input, parsed.as_ref()))
        {
            return out;
        }
    }
    if let Some(st) = serena::tool_of(tool) {
        return serena_call(rt, input, &cwd, st, user, mine, hooks.checks);
    }
    match (tool, parsed) {
        ("Bash", Some(parsed)) if hooks.guard => bash(rt, input, &cwd, &parsed, user, hooks.checks),
        ("Edit" | "Write" | "MultiEdit", _) if !user && !mine => testedit::pre(rt, input, &cwd),
        _ => Output::silent(),
    }
}

/// A Serena tool (§17.8), under the rules of the built-in tool it stands next to: its shell and
/// configuration refused; an edit of one file gets the test-edit ask; every edit, the working tree
/// snapshotted before it (`snapshot`), for the checks after it, as for a Bash command.
fn serena_call(
    rt: &Runtime,
    input: &HookInput,
    cwd: &Path,
    tool: &str,
    user: bool,
    mine: bool,
    snapshot: bool,
) -> Output {
    let kind = serena::kind(tool);
    match kind {
        serena::Kind::Shell => return pre_decision("deny", serena::SHELL_REASON),
        serena::Kind::Config => return pre_decision("deny", serena::CONFIG_REASON),
        serena::Kind::Read | serena::Kind::Memory => return Output::silent(),
        serena::Kind::Edit | serena::Kind::MultiEdit => {}
    }
    if user || mine {
        return Output::silent();
    }
    let answer = if kind == serena::Kind::Edit {
        testedit::pre_serena(rt, input, cwd, tool)
    } else {
        Output::silent()
    };
    if snapshot {
        if let Some(p) = rt.project(input) {
            bash_snapshot(rt, input, &p);
        }
    }
    answer
}

/// The guard on a Bash command, then (`snapshot`) the working tree before it runs, for the
/// checks after it.
fn bash(
    rt: &Runtime,
    input: &HookInput,
    cwd: &Path,
    parsed: &Parsed,
    user: bool,
    snapshot: bool,
) -> Output {
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
                ))
                // what was uncommitted at the start of the turn: the person's work
                .with_turn(project.as_ref().and_then(|p| p.snapshot()));
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
    if !user && snapshot {
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

/// Forge's locks: committed files the agent may read, but never change (the version of Forge
/// that runs is the lock's).
const LOCKS: &[&str] = &["forge.lock", "forge-user.lock"];

/// Whether a Bash command runs Forge's command line: `claude-forge`, or a `forge` that is
/// Forge (not Foundry's). The agent never operates its harness; `forge test` (a project's local
/// rules' examples) stays open, as in the CLI.
fn runs_forge(rt: &Runtime, parsed: &Parsed) -> bool {
    parsed.commands.iter().any(|c| {
        c.starts.iter().any(|&i| {
            let forge = match c.name_at(i) {
                "claude-forge" => true,
                "forge" => is_forge(rt, &c.words[i].text),
                _ => false,
            };
            forge && c.words.get(i + 1).is_none_or(|w| w.text != "test")
        })
    })
}

/// Whether `forge` (a name or a path) is this binary: it resolves to a `claude-forge`, or to the
/// executable answering the hook.
fn is_forge(rt: &Runtime, word: &str) -> bool {
    let found = if word.contains('/') {
        Some(PathBuf::from(word))
    } else {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|d| d.join(word))
                .find(|p| p.is_file())
        })
    };
    let Some(target) = found.and_then(|p| p.canonicalize().ok()) else {
        return false;
    };
    target.file_name().is_some_and(|n| n == "claude-forge")
        || rt.exe.canonicalize().is_ok_and(|e| e == target)
}

/// Commands that only read the files they are given (`sed` without `-i`, `git` with
/// [`GIT_READS`]).
const READERS: &[&str] = &[
    "cat", "head", "tail", "less", "more", "bat", "nl", "grep", "egrep", "fgrep", "rg", "wc",
    "diff", "cmp", "ls", "stat", "file", "sort", "uniq", "cut", "sed", "git",
];
const GIT_READS: &[&str] = &["diff", "show", "log", "blame", "status", "grep", "ls-files"];

/// A word naming a path: its value after any `NAME=`, when it has a `/`, starts with `~` or is a
/// lock's name.
fn named(w: &str) -> Option<&str> {
    let w = w.rsplit_once('=').map_or(w, |(_, v)| v);
    (w.contains('/') || w.starts_with('~') || LOCKS.contains(&w)).then_some(w)
}

/// Whether word `wi` of `c` is an argument of a command that only reads it.
fn only_read(c: &parse::SimpleCommand, wi: usize) -> bool {
    let Some(&s) = c.starts.iter().filter(|&&i| i < wi).max() else {
        return false;
    };
    let args = &c.words[s + 1..];
    match c.name_at(s) {
        "sed" => !args.iter().any(|w| {
            w.text.starts_with("-i") || w.text == "--in-place" || w.text.starts_with("--in-place=")
        }),
        "git" => args
            .first()
            .is_some_and(|w| GIT_READS.contains(&w.text.as_str())),
        name => READERS.contains(&name),
    }
}

/// Whether the tool call names a file in Forge's cache or state, or changes one of its locks.
fn names_internal(rt: &Runtime, input: &HookInput, cwd: &Path, parsed: Option<&Parsed>) -> bool {
    let home = rt.env.home_dir.as_deref();
    let internal = |s: &str, base: &Path| -> bool {
        expand(s, base, home).is_some_and(|p| rt.home.is_internal_path(&p))
    };
    let lock = |s: &str, base: &Path| -> bool {
        expand(s, base, home).is_some_and(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| LOCKS.contains(&n))
        })
    };
    // a call that changes the file it names
    let changes = |s: &str, base: &Path| internal(s, base) || lock(s, base);
    if let Some(tool) = serena::tool_of(input.tool()) {
        // Serena's paths are relative to the project root
        let root = git_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
        let reads = serena::kind(tool) == serena::Kind::Read;
        return ["relative_path", "file_path"]
            .iter()
            .filter_map(|k| input.input_str(k))
            .any(|p| {
                if reads {
                    internal(p, &root)
                } else {
                    changes(p, &root)
                }
            });
    }
    match input.tool() {
        "Read" => input
            .input_str("file_path")
            .is_some_and(|p| internal(p, cwd)),
        "Edit" | "Write" | "MultiEdit" => input
            .input_str("file_path")
            .is_some_and(|p| changes(p, cwd)),
        "NotebookRead" => input
            .input_str("notebook_path")
            .is_some_and(|p| internal(p, cwd)),
        "NotebookEdit" => input
            .input_str("notebook_path")
            .is_some_and(|p| changes(p, cwd)),
        "Grep" | "Glob" => {
            let base = input
                .input_str("path")
                .and_then(|p| expand(p, cwd, home))
                .unwrap_or_else(|| cwd.to_path_buf());
            rt.home.is_internal_path(&base)
                || input.input_str("pattern").is_some_and(|p| {
                    (p.starts_with('/') || p.starts_with('~')) && internal(p, &base)
                })
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
                let words = c.words.iter().enumerate().any(|(i, w)| {
                    named(&w.text)
                        .is_some_and(|w| internal(w, base) || (lock(w, base) && !only_read(c, i)))
                });
                let redirects = c.redirects.iter().any(|r| {
                    named(&r.target.text)
                        .is_some_and(|w| internal(w, base) || (lock(w, base) && r.writes_file()))
                });
                words || redirects
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
