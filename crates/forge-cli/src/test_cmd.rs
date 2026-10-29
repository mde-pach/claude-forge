//! `forge test [--builtin] [--json]`: runs rules' examples through the machinery the built-in
//! packs' example tests use (`forge_guard::examples`, `forge_check::examples`): the guard, the
//! ast-grep and grep patterns, and the check stages on fixtures. Without `--builtin`, the
//! repository's local rules (its local packs and standalone rules); with it, the built-in packs.
//!
//! One line per example, then a count; exit 1 when an example fails. A skipped example is told
//! apart by its result's structure (it carries the note that says why), never by its text. Run
//! by Claude Code (`CLAUDECODE`), a skip's reason, a note for the person, is recorded for
//! `forge status` instead of printed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::Args;
use forge_check::scope::Roles;
use forge_check::tools::ToolResolver;
use forge_core::examples::{ExampleResult, ExampleVerdict};
use forge_packs::{PackSet, PackSource, Rule};

use crate::provisioning;

#[derive(Args)]
pub struct TestArgs {
    /// Run the built-in packs' examples instead of this repository's local rules.
    #[arg(long)]
    pub builtin: bool,
    /// Print one JSON object per example, then a summary object.
    #[arg(long)]
    pub json: bool,
}

/// Every result of one rule's examples, each target in turn.
pub fn rule_results(
    rule: &Rule,
    set: &PackSet,
    exe: &Path,
    tools: &Arc<dyn ToolResolver>,
) -> Vec<ExampleResult> {
    let roles = Roles::defaults();
    let mut out = forge_guard::examples::run(rule);
    out.extend(forge_check::examples::ast_grep(rule, &roles));
    out.extend(forge_check::examples::grep(rule, &roles));
    out.extend(forge_check::examples::check(rule, set, exe, tools));
    out
}

/// The results of `rules`, run on a few threads, in rule order.
pub fn run_rules(
    rules: &[&Rule],
    set: &PackSet,
    exe: &Path,
    tools: &Arc<dyn ToolResolver>,
) -> Vec<ExampleResult> {
    let queue = Mutex::new(rules.iter().enumerate().collect::<Vec<_>>());
    let done = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let next = queue.lock().map(|mut q| q.pop()).ok().flatten();
                    let Some((i, rule)) = next else { break };
                    let res = rule_results(rule, set, exe, tools);
                    if let Ok(mut d) = done.lock() {
                        d.push((i, res));
                    }
                }
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_default();
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().flat_map(|(_, r)| r).collect()
}

fn repo_root() -> Result<PathBuf, String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("not in a git repository".into());
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// The packs to test and the rules whose examples run.
fn load(builtin: bool) -> Result<(PackSet, Option<PathBuf>), String> {
    let cache = forge_home::Home::from_env().ok().map(|h| h.packs_dir());
    let opts = forge_packs::Options {
        cache_dir: cache.as_deref(),
    };
    if builtin {
        return forge_packs::load_builtin(opts)
            .map(|s| (s, None))
            .map_err(|e| e.to_string());
    }
    let root = repo_root()?;
    let set = forge_packs::load(Some(&root), opts).map_err(|e| e.to_string())?;
    Ok((set, Some(root)))
}

fn tested(set: &PackSet, builtin: bool) -> Vec<&Rule> {
    let want = if builtin {
        PackSource::Builtin
    } else {
        PackSource::Local
    };
    let mut rules: Vec<&Rule> = set
        .packs
        .values()
        .filter(|p| p.source == want)
        .flat_map(|p| p.rules.iter())
        .collect();
    if !builtin {
        rules.extend(set.local_rules.iter());
    }
    rules
}

pub fn run(args: TestArgs) -> ExitCode {
    let (set, root) = match load(args.builtin) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("forge test: {e}");
            return ExitCode::from(2);
        }
    };
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("forge test: {e}");
            return ExitCode::from(2);
        }
    };
    let tools = provisioning::tools(!provisioning::downloads_off());
    let rules = tested(&set, args.builtin);
    let results = run_rules(&rules, &set, &exe, &tools);
    let (mut pass, mut fail, mut skip) = (0, 0, 0);
    for r in &results {
        match r.verdict {
            ExampleVerdict::Pass => pass += 1,
            ExampleVerdict::Fail { .. } => fail += 1,
            ExampleVerdict::Skip { .. } => skip += 1,
        }
    }
    let agent = std::env::var_os("CLAUDECODE").is_some();
    let mut out = std::io::stdout().lock();
    if args.json {
        for r in &results {
            let _ = writeln!(out, "{}", serde_json::to_string(r).unwrap_or_default());
        }
        let summary = serde_json::json!({
            "rules": rules.len(), "examples": results.len(),
            "passed": pass, "failed": fail, "skipped": skip,
        });
        let _ = writeln!(out, "{summary}");
    } else {
        if rules.is_empty() {
            let _ = writeln!(
                out,
                "{}",
                if args.builtin {
                    "No built-in rules were loaded."
                } else {
                    "This repository has no local rules."
                }
            );
        }
        let mut notes = Vec::new();
        for r in &results {
            let line = match &r.verdict {
                ExampleVerdict::Pass => format!("PASS {}", r.label()),
                ExampleVerdict::Fail { why } => format!("FAIL {}: {why}", r.label()),
                // the reason is a note for the person: not Claude's to read
                ExampleVerdict::Skip { note } if agent => {
                    notes.push(note.clone());
                    format!("SKIP {}", r.label())
                }
                ExampleVerdict::Skip { note } => format!("SKIP {}: {}", r.label(), note.text),
            };
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(
            out,
            "{} examples of {} rules: {pass} passed, {fail} failed, {skip} skipped",
            results.len(),
            rules.len()
        );
        if agent
            && !notes.is_empty()
            && let (Ok(home), Some(root)) = (forge_home::Home::from_env(), &root)
        {
            let state = home.repo_state_dir(&forge_home::RepoId::from_physical_path(root));
            forge_hooks::state::record_notes(&state, &notes);
        }
    }
    if fail > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
