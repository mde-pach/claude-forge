//! `forge check` (for CI, pre-commit and people) and `forge internal <builtin>` (what check
//! commands call).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use forge_check::builtins::Registry;
use forge_check::{Checker, Mode, Options, checker::ActiveRule};

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

/// The builtins `forge internal` runs: the checker's, and (as they land) the domain builtins
/// and helpers of other crates.
pub fn registry() -> Registry {
    Registry::new()
}

pub fn internal(args: InternalArgs) -> ExitCode {
    let code = forge_check::builtins::internal_main(&registry(), &args.name, args.args);
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

/// The checker for a repository: its packs, local rules, forge.yaml.
pub fn checker_for(root: &Path, only: Option<&str>) -> Result<Checker, String> {
    let cache = forge_home::Home::from_env().ok().map(|h| h.packs_dir());
    let set = forge_packs::load(
        Some(root),
        forge_packs::Options {
            cache_dir: cache.as_deref(),
        },
    )
    .map_err(|e| e.to_string())?;
    let config = forge_packs::load_project_config(root).map_err(|e| e.to_string())?;
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
        None => forge_check::select::active_rules(root, &set, config.as_ref()),
    };
    let mut c = Checker::new(root.to_path_buf(), rules);
    c.roles = forge_check::scope::Roles::with_config(config.as_ref());
    c.accept = config.and_then(|c| c.accept).unwrap_or_default();
    Ok(c)
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
        let checker = checker_for(&root, args.rules.as_deref())?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        checker
            .check(mode, &Options::stop(exe))
            .map_err(|e| e.to_string())
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
