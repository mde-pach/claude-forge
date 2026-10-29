//! `forge check` (for CI, pre-commit and people) and `forge internal <builtin>` (what check
//! commands call).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use forge_check::builtins::Registry;
use forge_check::tools::ToolResolver;
use forge_check::{Checker, Mode, Options, checker::ActiveRule};
use forge_core::Note;

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
/// builtins. A crate of builtins is one line here.
pub fn default_registry() -> Registry {
    let mut r = Registry::new();
    for b in forge_helpers::builtins(&helper_options()) {
        r.register_helper(b);
    }
    // the domain builtins, as their crates land:
    // for b in forge_builtins_data::builtins(&opts) { r.register(b); }
    // for b in forge_builtins_infra::builtins(&opts) { r.register(b); }
    // for b in forge_builtins_code::builtins(&opts) { r.register(b); }
    r
}

/// The same as [`default_registry`] (the name the hooks used first).
pub fn registry() -> Registry {
    default_registry()
}

pub fn internal(args: InternalArgs) -> ExitCode {
    let code = forge_check::builtins::internal_main(
        &default_registry(),
        &args.name,
        args.args,
        provisioning::builtin_tools(),
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

/// The checker for a repository: the rules `forge apply` selects there (its packs, local rules,
/// forge.yaml), the file roles, and where tools are. `download`: a missing pinned tool may be
/// installed (`forge check`, Stop; not after an edit). No types engine: see [`with_engine`].
pub fn checker_with(root: &Path, only: Option<&str>, download: bool) -> Result<Checker, String> {
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
    let mut c = Checker::new(root.to_path_buf(), rules);
    c.roles = selection.roles;
    c.accept = config.and_then(|c| c.accept).unwrap_or_default();
    c.tools = provisioning::tools(download);
    Ok(c)
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
        );
        checker.types = oracle.map(|o| Arc::new(o) as Arc<dyn forge_check::astgrep::TypeOracle>);
        notes
    } else {
        checker.types = forge_check::engine::running(&checker.root)
            .map(|o| Arc::new(o) as Arc<dyn forge_check::astgrep::TypeOracle>);
        Vec::new()
    }
}

/// `forge engine serve --config <json>`: the types engine daemon (started detached).
pub fn engine_serve(config: &str) -> ExitCode {
    match serde_json::from_str::<forge_engine::EngineConfig>(config)
        .map_err(|e| e.to_string())
        .and_then(forge_engine::serve)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("forge engine: {e}");
            ExitCode::from(2)
        }
    }
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
