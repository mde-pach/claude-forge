//! `forge test [--builtin] [--engine] [--json]`: runs rules' examples through the machinery the
//! built-in packs' example tests use (`forge_guard::examples`, `forge_check::examples`): the guard,
//! the ast-grep and grep patterns, and the check stages on fixtures. Without `--builtin`, the
//! repository's local rules (its local packs and standalone rules); with it, the built-in packs.
//! With `--engine`, a rule with a type constraint (`where`) has all its examples run through the
//! checker and the types engine, in a project holding its `examples.deps`, installed once per
//! set of dependencies into Forge's cache with Forge's Bun or uv (network the first time);
//! without it, only its `match` examples are proven, by the pattern alone.
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
    /// Run the examples of rules with a type constraint through the types engine (installs
    /// their dependencies into Forge's cache the first time).
    #[arg(long)]
    pub engine: bool,
    /// Print one JSON object per example, then a summary object.
    #[arg(long)]
    pub json: bool,
}

/// The installed examples' projects, by rule id (`--engine`).
pub type Projects =
    std::collections::BTreeMap<String, Result<forge_check::examples::Project, String>>;

/// Every result of one rule's examples, each target in turn.
pub fn rule_results(
    rule: &Rule,
    set: &PackSet,
    exe: &Path,
    tools: &Arc<dyn ToolResolver>,
    projects: Option<&Projects>,
) -> Vec<ExampleResult> {
    let roles = Roles::defaults();
    let mut out = forge_guard::examples::run(rule);
    match projects.and_then(|p| p.get(&rule.id)) {
        Some(project) => out.extend(forge_check::examples::typed(rule, set, exe, tools, project)),
        None => out.extend(forge_check::examples::ast_grep(rule, &roles)),
    }
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
    projects: Option<&Projects>,
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
                    let res = rule_results(rule, set, exe, tools, projects);
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
    let projects = args.engine.then(|| install_projects(&rules));
    let results = run_rules(&rules, &set, &exe, &tools, projects.as_ref());
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

/// The examples' project of each rule with a type constraint, installed once per set of files
/// (manifest, tsconfig) under `<cache>/example-projects/<hash>`: `bun install` for npm
/// dependencies, `uv sync` for PyPI ones, with Forge's own Bun and uv.
fn install_projects(rules: &[&Rule]) -> Projects {
    let mut out = Projects::new();
    let mut done: std::collections::BTreeMap<
        String,
        Result<forge_check::examples::Project, String>,
    > = std::collections::BTreeMap::new();
    for rule in rules {
        let Some(files) = forge_check::examples::project_files(rule) else {
            continue;
        };
        let joined: String = files.iter().map(|(f, t)| format!("{f}\0{t}\0")).collect();
        let key = forge_render::lock::sha256(joined)[..16].to_string();
        let project = done
            .entry(key.clone())
            .or_insert_with(|| install(&key, &files))
            .clone();
        out.insert(rule.id.clone(), project);
    }
    out
}

fn install(
    key: &str,
    files: &std::collections::BTreeMap<String, String>,
) -> Result<forge_check::examples::Project, String> {
    let home = forge_home::Home::from_env().map_err(|e| e.to_string())?;
    let dir = home.cache_root().join("example-projects").join(key);
    let python = files.contains_key("pyproject.toml");
    let link = if python { ".venv" } else { "node_modules" };
    let ready = dir.join(".ready");
    if !ready.is_file() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (f, t) in files {
            std::fs::write(dir.join(f), t).map_err(|e| format!("{f}: {e}"))?;
        }
        let provisioner = forge_tools::provision::Provisioner::new(home.clone());
        let tool = if python { "uv" } else { "bun" };
        let exe = provisioner.ensure(tool).map_err(|n| n.text)?;
        let args: &[&str] = if python {
            &["sync", "--quiet"]
        } else {
            &["install", "--silent"]
        };
        let out = std::process::Command::new(&exe)
            .args(args)
            .current_dir(&dir)
            .output()
            .map_err(|e| format!("{tool}: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "{tool} {}: {}",
                args[0],
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        std::fs::write(&ready, "").map_err(|e| e.to_string())?;
    }
    Ok(forge_check::examples::Project {
        files: files.clone(),
        link: Some((link.into(), dir.join(link))),
    })
}
