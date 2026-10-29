//! `forge check` (for CI, pre-commit and people) and `forge internal <builtin>` (what check
//! commands call).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use forge_check::builtins::{AdapterTools, BuiltinTools, Registry};
use forge_check::tools::ToolResolver;
use forge_check::{Checker, Mode, Options, checker::ActiveRule};
use forge_core::Note;
use serde::{Deserialize, Serialize};

use crate::provisioning;

#[derive(Args, Debug)]
#[command(group = clap::ArgGroup::new("mode").multiple(false))]
pub struct CheckArgs {
    /// Uncommitted changes against HEAD (pre-commit).
    #[arg(long, group = "mode")]
    changed: bool,
    /// Everything since `git merge-base <REF> HEAD` (CI on a pull request).
    #[arg(long, value_name = "REF", group = "mode")]
    since: Option<String>,
    /// The whole project (CI on the default branch; the default).
    #[arg(long, group = "mode")]
    all: bool,
    /// The files changed during a turn, against its snapshot directory.
    #[arg(long, value_name = "DIR", group = "mode")]
    turn: Option<PathBuf>,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
    /// Check only these rules (comma-separated ids), whatever the detection says.
    #[arg(long, value_name = "IDS", hide = true)]
    rules: Option<String>,
}

#[derive(Args, Debug)]
pub struct InternalArgs {
    /// The builtin, without its `forge_` prefix.
    name: String,
    /// Its arguments.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

/// How the helpers that ask package registries behave: answers cached in Forge's cache.
fn helper_options() -> forge_helpers::Options {
    forge_helpers::Options {
        cache_dir: forge_home::Home::from_env()
            .ok()
            .map(|h| h.cache_root().join("registry")),
        ..forge_helpers::Options::default()
    }
}

/// The builtins `forge internal` runs: the checker's machinery, the helpers (forge-helpers:
/// `forge_helper <runtime> <file>`, `forge_ratchet <runtime> <file>`), and the domain
/// builtins (forge-builtins-data, -infra, -code), whose tools are `tools` through the checker's
/// adapters. A crate of builtins is one line here.
pub fn default_registry(tools: Arc<dyn ToolResolver>) -> Registry {
    let mut r = Registry::new();
    for b in forge_helpers::builtins(&helper_options()) {
        r.register_helper(b);
    }
    let tools: Arc<dyn BuiltinTools> = Arc::new(AdapterTools::new(tools, forge_tools::lock_pins()));
    let data = forge_builtins_data::Options {
        tools: tools.clone(),
        ..forge_builtins_data::Options::default()
    };
    let infra = forge_builtins_infra::Options {
        tools: tools.clone(),
        cache_dir: forge_home::Home::from_env()
            .ok()
            .map(|h| h.cache_root().join("infra")),
    };
    let code = forge_builtins_code::Options {
        tools,
        ..forge_builtins_code::Options::default()
    };
    for b in forge_builtins_data::builtins(data)
        .into_iter()
        .chain(forge_builtins_infra::builtins(&infra))
        .chain(forge_builtins_code::builtins(&code))
    {
        r.register_check(b);
    }
    r
}

pub fn internal(args: InternalArgs) -> ExitCode {
    let tools = provisioning::builtin_tools();
    let code = forge_check::builtins::internal_main(
        &default_registry(tools.clone()),
        &args.name,
        args.args,
        tools,
    );
    ExitCode::from(code.clamp(0, 255) as u8)
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

/// What a repository's checker runs: the rules `forge apply` selects there (its packs, local
/// rules, forge.yaml), the file roles, the accepted changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Selected {
    pub rules: Vec<ActiveRule>,
    pub roles: BTreeMap<String, Vec<String>>,
    pub accept: Vec<forge_packs::model::Accept>,
}

/// Plans the repository as `forge apply` does (`only`: these rule ids, whatever the detection).
pub fn select(root: &Path, only: Option<&str>) -> Result<Selected, String> {
    let cache = forge_home::Home::from_env().ok().map(|h| h.packs_dir());
    let set = forge_packs::load(
        Some(root),
        forge_packs::Options {
            cache_dir: cache.as_deref(),
        },
    )
    .map_err(|e| e.to_string())?;
    let config = forge_packs::load_project_config(root).map_err(|e| e.to_string())?;
    let selection = forge_check::select::project(root, &set, config.as_ref())?;
    let rules: Vec<ActiveRule> = match only {
        Some(ids) => {
            let want: Vec<&str> = ids.split(',').map(str::trim).collect();
            set.packs
                .values()
                .flat_map(|p| p.rules.iter())
                .chain(set.local_rules.iter())
                .filter(|r| want.contains(&r.id.as_str()))
                .cloned()
                .map(ActiveRule::new)
                .collect()
        }
        None => selection.rules,
    };
    Ok(Selected {
        rules,
        roles: selection.roles.map().clone(),
        accept: config.and_then(|c| c.accept).unwrap_or_default(),
    })
}

/// The cache of [`select`] for the hooks.
#[derive(Serialize, Deserialize)]
struct SelectionCache {
    key: String,
    selected: Selected,
}

/// What the selection depends on, as the hooks' rule cache keys it (Forge's build, the lock's
/// `rule_set`, the packaging files at the root), plus forge.yaml and the local packs and rules.
/// None when the repository has no lock (nothing applied): no cache then.
fn selection_key(home: &forge_home::Home, root: &Path, state: &Path) -> Option<String> {
    let source = forge_hooks::rules::Source {
        lock: root.join(forge_home::LOCK_FILE),
        repo: Some(root),
        packs_cache: Some(home.packs_dir()),
    };
    let hc = forge_hooks::rules::load(state, &source)?;
    let meta = |p: &Path| -> String {
        std::fs::metadata(p)
            .ok()
            .map(|m| {
                let t = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                format!("{}:{t}", m.len())
            })
            .unwrap_or_default()
    };
    Some(format!(
        "{}|{}|{}|{}|{}|{}",
        hc.build,
        hc.rule_set,
        hc.facts_key.unwrap_or_default(),
        meta(&root.join("forge.yaml")),
        meta(&root.join(".forge/packs")),
        meta(&root.join(".forge/rules")),
    ))
}

/// [`select`], from the state directory's cache when its key still holds: an edit's hook does
/// not plan the repository again.
pub fn select_cached(root: &Path) -> Result<Selected, String> {
    let Ok(home) = forge_home::Home::from_env() else {
        return select(root, None);
    };
    let Ok(id) = forge_home::RepoId::for_root(root) else {
        return select(root, None);
    };
    let state = home.repo_state_dir(&id);
    let Some(key) = selection_key(&home, root, &state) else {
        return select(root, None);
    };
    let file = state.join("checker-cache.json");
    if let Some(c) = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice::<SelectionCache>(&b).ok())
        .filter(|c| c.key == key)
    {
        return Ok(c.selected);
    }
    let selected = select(root, None)?;
    let cache = SelectionCache { key, selected };
    if let Ok(b) = serde_json::to_vec(&cache) {
        let tmp = file.with_extension(format!("tmp.{}", std::process::id()));
        if std::fs::create_dir_all(&state).is_ok() && std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
    Ok(cache.selected)
}

/// The checker of a selection. `download`: a missing pinned tool may be installed (`forge
/// check`, Stop; not after an edit). No types engine: see [`with_engine`].
pub fn checker_of(root: &Path, selected: Selected, download: bool) -> Checker {
    let mut c = Checker::new(root.to_path_buf(), selected.rules);
    c.roles = forge_check::scope::Roles::from_map(selected.roles);
    c.accept = selected.accept;
    c.tools = provisioning::tools(download);
    c
}

/// The checker for a repository, planned now (see [`select`]).
pub fn checker_with(root: &Path, only: Option<&str>, download: bool) -> Result<Checker, String> {
    Ok(checker_of(root, select(root, only)?, download))
}

/// The checker as `forge check` runs it (downloads allowed).
pub fn checker_for(root: &Path, only: Option<&str>) -> Result<Checker, String> {
    checker_with(root, only, true)
}

/// Gives the checker the types engine: started when none runs (`start`: `forge check`, Stop),
/// else only the daemon already running (after an edit). What could not be set up is notes.
pub fn with_engine(checker: &mut Checker, exe: &Path, start: bool) -> Vec<Note> {
    if start {
        let (oracle, notes) = forge_check::engine::start(
            &checker.root,
            &checker.rules,
            checker.tools.as_ref(),
            !provisioning::downloads_off(),
            exe,
            env!("CARGO_PKG_VERSION"),
            None,
        );
        checker.types = oracle.map(|o| Arc::new(o) as Arc<dyn forge_check::astgrep::TypeOracle>);
        notes
    } else {
        checker.types = forge_check::engine::running(&checker.root)
            .map(|o| Arc::new(o) as Arc<dyn forge_check::astgrep::TypeOracle>);
        Vec::new()
    }
}

/// `forge engine serve --config <file>`: the types engine daemon (started detached), with the
/// configuration the checker wrote.
pub fn engine_serve(config: &Path) -> ExitCode {
    match forge_check::engine::serve(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("forge engine: {e}");
            ExitCode::from(2)
        }
    }
}

/// `forge engine start`: makes sure the engine of `root` runs for its typed rules and registers
/// `session` (detached, after SessionStart: it may download the pinned ty).
pub fn engine_start(root: &Path, session: &str, exe: &Path) -> Vec<Note> {
    let selected = match select_cached(root) {
        Ok(s) => s,
        Err(e) => {
            return vec![Note::new(
                forge_core::NoteKind::ForgeFailure,
                format!("The types engine was not started: the rules could not be loaded ({e})."),
            )];
        }
    };
    let tools = provisioning::tools(true);
    let (_, notes) = forge_check::engine::start(
        root,
        &selected.rules,
        tools.as_ref(),
        !provisioning::downloads_off(),
        exe,
        env!("CARGO_PKG_VERSION"),
        Some(session),
    );
    notes
}

/// The tools as `forge status` and `forge apply` ask them.
pub fn probe_tools(download: bool) -> Arc<dyn ToolResolver> {
    provisioning::tools(download)
}

pub fn run(args: CheckArgs) -> ExitCode {
    let mode = if args.changed {
        Mode::Changed
    } else if let Some(r) = args.since.clone() {
        Mode::Since(r)
    } else if let Some(t) = args.turn.clone() {
        Mode::Turn(t)
    } else {
        Mode::All
    };
    let result = repo_root().and_then(|root| {
        let mut checker = checker_for(&root, args.rules.as_deref())?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let notes = with_engine(&mut checker, &exe, true);
        let mut o = checker
            .check(mode, &Options::stop(exe))
            .map_err(|e| e.to_string())?;
        o.report.notes.extend(notes);
        Ok(o)
    });
    match result {
        Ok(o) => {
            let mut out = std::io::stdout().lock();
            if args.json {
                let _ = writeln!(out, "{}", forge_check::output::json(&o));
            } else {
                let _ = write!(out, "{}", forge_check::output::person_text(&o));
            }
            ExitCode::from(o.status.exit_code() as u8)
        }
        Err(e) => {
            eprintln!("forge check: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `forge_<name>` a pack's check command calls is a builtin `forge internal` runs:
    /// the checker's machinery or a domain crate's. A name nobody registered would only be a
    /// "not available yet" note, so the rule would never be enforced.
    #[test]
    fn every_builtin_the_packs_call_is_registered() {
        let registry = default_registry(Arc::new(forge_check::tools::LocalTools::default()));
        let packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        let re = regex::Regex::new(r"forge_[a-z_]+").unwrap();
        let mut used = std::collections::BTreeSet::new();
        let mut stack = vec![packs];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Ok(text) = std::fs::read_to_string(&p) {
                    used.extend(re.find_iter(&text).map(|m| m.as_str().to_string()));
                }
            }
        }
        assert!(used.len() > 50, "{used:?}");
        let missing: Vec<&String> = used
            .iter()
            .filter(|n| !registry.has(n.trim_start_matches("forge_")))
            .collect();
        assert!(missing.is_empty(), "unregistered builtins: {missing:?}");
        // a check command reaches a builtin through the preamble's shell functions
        let undefined: Vec<&String> = used
            .iter()
            .filter(|n| {
                !forge_check::shell::BUILTIN_NAMES.contains(&n.trim_start_matches("forge_"))
            })
            .collect();
        assert!(undefined.is_empty(), "no shell function for: {undefined:?}");
        let dead: Vec<&&str> = forge_check::shell::BUILTIN_NAMES
            .iter()
            .filter(|n| !registry.has(n))
            .collect();
        assert!(
            dead.is_empty(),
            "shell functions no builtin answers: {dead:?}"
        );
    }
}
