//! The setup commands: `init`, `apply`, `diff`, `status`, `explain`, `remove`, at project
//! level (the repository) or user level (`--user`: `~/.claude`).

mod apply;
mod io;
mod precommit;
mod remove;
mod status;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use forge_detect::roles::{RoleInputs, resolve_roles};
use forge_packs::PackSet;
use forge_packs::model::{Level, ProjectConfig};
use forge_render::changes::{
    self, Migration, Previous, detect_migration, keep_pre_commit_hook, read_lock,
};
use forge_render::lock::{CLAUDE_CODE_MIN, Lock, LockBlock};
use forge_render::plan::{Plan, PlanInput, plan};
use forge_render::report::ToolProbe;

pub use io::Error;

/// This binary's version, as the lock pins it.
pub const FORGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Args, Debug, Clone, Default)]
pub struct Flags {
    /// The user-level setup (~/.claude) instead of the repository's.
    #[arg(long, conflicts_with = "local")]
    pub user: bool,
    /// Write the settings entries to .claude/settings.local.json (a repository you do not own).
    #[arg(long)]
    pub local: bool,
    /// Overwrite files changed since the last apply, or that are not Forge's.
    #[arg(long)]
    pub force: bool,
    /// Print what would change; write nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Answer yes to the confirmations (shared settings, the old layout's deletion).
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Consent to adding the commit-time check to a hook manager's config.
    #[arg(long)]
    pub pre_commit: bool,
}

#[derive(Subcommand, Debug)]
pub enum ProjectCommand {
    /// Creates the stack's missing tool configs from the packs' templates, then applies.
    Init {
        #[command(flatten)]
        flags: Flags,
    },
    /// Writes the setup: the rules files, the settings entries and the lock.
    Apply {
        #[command(flatten)]
        flags: Flags,
    },
    /// Shows what apply would change; exits 1 when anything differs.
    Diff {
        #[command(flatten)]
        flags: Flags,
    },
    /// The active packs and rules, how each rule is enforced here, drift, missing tools.
    Status {
        #[command(flatten)]
        flags: Flags,
        /// Print one JSON object.
        #[arg(long)]
        json: bool,
    },
    /// A rule's file and where it is enforced here.
    Explain {
        /// The rule id (python/no-eval).
        id: String,
        #[command(flatten)]
        flags: Flags,
    },
    /// Deletes what the lock records: the rules files, the settings entries, the pre-commit block.
    Remove {
        #[command(flatten)]
        flags: Flags,
    },
}

pub fn run(cmd: ProjectCommand) -> ExitCode {
    // what the machine's tools can do, as the checker sees them (machine state: for the person)
    let probe = forge_check::probe::CheckerProbe::new(crate::check::probe_tools(false));
    let r = match cmd {
        ProjectCommand::Init { flags } => apply::init(&flags, &probe),
        ProjectCommand::Apply { flags } => apply::apply(&flags, &probe),
        ProjectCommand::Diff { flags } => apply::diff(&flags),
        ProjectCommand::Status { flags, json } => status::status(&flags, json, &probe),
        ProjectCommand::Explain { id, flags } => status::explain(&id, &flags, &probe),
        ProjectCommand::Remove { flags } => remove::remove(&flags),
    };
    match r {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("{}", e.message);
            ExitCode::from(e.code)
        }
    }
}

/// Everything a command knows about the level it works on.
pub struct Ctx {
    pub level: Level,
    pub base: PathBuf,
    pub set: PackSet,
    pub config: Option<ProjectConfig>,
    pub plan: Plan,
    pub lock: Option<Lock>,
    pub prev: Previous,
    pub migration: Option<Migration>,
    /// Forge's blocks in hook managers' files: the previous ones, plus one apply adds.
    pub blocks: Vec<LockBlock>,
}

/// Claude Code's user config directory: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
fn claude_config_dir() -> Result<PathBuf, Error> {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    forge_render::lock::claude_config_dir(
        var("CLAUDE_CONFIG_DIR"),
        var("HOME").filter(|h| !h.as_os_str().is_empty()),
    )
    .ok_or_else(|| Error::new("forge: HOME is not set"))
}

/// The base of a level: the repository's top (project), or Claude Code's user config
/// directory (user).
pub fn base_of(level: Level) -> Result<PathBuf, Error> {
    match level {
        Level::User => claude_config_dir(),
        Level::Project => {
            let cwd = std::env::current_dir().map_err(|e| Error::new(format!("forge: {e}")))?;
            forge_detect::git_toplevel(&cwd).ok_or_else(|| {
                Error::new(
                    "forge: not inside a git repository (use --user for the user-level setup)",
                )
            })
        }
    }
}

fn level_of(flags: &Flags) -> Level {
    if flags.user {
        Level::User
    } else {
        Level::Project
    }
}

pub fn load_packs(level: Level, base: &Path) -> Result<PackSet, Error> {
    let home = forge_home::Home::from_env().ok();
    let cache = home.as_ref().map(|h| h.packs_dir());
    let opts = forge_packs::Options {
        cache_dir: cache.as_deref(),
    };
    let repo = (level == Level::Project).then_some(base);
    forge_packs::load(repo, opts).map_err(|e| Error::new(format!("forge: {e}")))
}

/// Plans the level and reads what the last apply left.
pub fn context(flags: &Flags) -> Result<Ctx, Error> {
    let level = level_of(flags);
    let base = base_of(level)?;
    let set = load_packs(level, &base)?;
    let fail = |e: String| Error::new(format!("forge: {e}"));
    let config = match level {
        Level::Project => {
            forge_packs::load_project_config(&base).map_err(|e| fail(e.to_string()))?
        }
        Level::User => None,
    };
    let (facts, roles) = match level {
        Level::Project => {
            let files = forge_detect::list_repo_files(&base);
            let facts =
                forge_detect::read_project_facts(&base, &files).map_err(|e| fail(e.to_string()))?;
            let inputs = RoleInputs {
                repo: &base,
                files: &files,
            };
            let roles = resolve_roles(
                Some(&inputs),
                config.as_ref().and_then(|c| c.files.as_ref()),
            )
            .map_err(|e| fail(e.to_string()))?;
            (Some(facts), roles)
        }
        Level::User => (
            None,
            resolve_roles(None, None).map_err(|e| fail(e.to_string()))?,
        ),
    };
    let mut p = plan(&PlanInput {
        level,
        base: &base,
        set: &set,
        local: flags.local && level == Level::Project,
        config: config.as_ref(),
        facts: facts.as_ref(),
        roles: &roles,
    })
    .map_err(fail)?;
    let lock = read_lock(&base, level, &p.lock_path).map_err(fail)?;
    let migration = detect_migration(&base, level).map_err(fail)?;
    let mut prev = match (&lock, &migration) {
        (Some(l), _) => Previous::from(l),
        (None, Some(m)) => m.previous.clone(),
        (None, None) => Previous::default(),
    };
    // the lock pins this Forge's tools (`[tools]`): what apply provisions
    if level == Level::Project {
        prev.tools = forge_tools::lock_pins()
            .into_iter()
            .map(|(k, v)| (k, toml::Value::String(v)))
            .collect();
    }
    keep_pre_commit_hook(&mut p, &prev);
    let blocks = prev.blocks.clone();
    Ok(Ctx {
        level,
        base,
        set,
        config,
        plan: p,
        lock,
        prev,
        migration,
        blocks,
    })
}

impl Ctx {
    pub fn changes(&self) -> Result<changes::Changes, Error> {
        changes::compute(
            &self.plan,
            &self.prev,
            &self.blocks,
            self.migration.as_ref(),
            FORGE_VERSION,
        )
        .map_err(|e| Error::new(format!("forge: {e}")))
    }
}

/// Refuses a Claude Code older than [`CLAUDE_CODE_MIN`] (or none): the hook entries need it.
pub fn claude_gate() -> Result<(), Error> {
    let out = std::process::Command::new("claude")
        .arg("--version")
        .output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else {
        return Err(Error::new(format!(
            "forge: Claude Code not found; forge needs claude >= {CLAUDE_CODE_MIN}"
        )));
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let v = regex::Regex::new(r"(\d+\.\d+\.\d+)")
        .expect("regex")
        .captures(&text)
        .map(|c| c[1].to_string());
    match v {
        Some(v)
            if forge_detect::versions::compare(&v, CLAUDE_CODE_MIN) != std::cmp::Ordering::Less =>
        {
            Ok(())
        }
        v => Err(Error::new(format!(
            "forge: Claude Code {} is too old; forge needs claude >= {CLAUDE_CODE_MIN}",
            v.as_deref().unwrap_or("(unknown version)")
        ))),
    }
}

/// The tool pins of the lock on disk that differ from this Forge's: `(tool, locked, ours)`
/// (an empty side: not pinned there).
pub fn pin_differences(ctx: &Ctx) -> Vec<(String, String, String)> {
    let Some(lock) = &ctx.lock else {
        return Vec::new();
    };
    if ctx.level != Level::Project {
        return Vec::new();
    }
    let locked: std::collections::BTreeMap<String, String> = lock
        .tools
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.as_str().map_or_else(|| v.to_string(), str::to_owned),
            )
        })
        .collect();
    forge_tools::pin_differences(&locked)
}

/// The person's notes: what the launcher left for this repository, and the probe's.
pub fn person_notes(ctx: &Ctx, probe: &dyn ToolProbe) -> Vec<String> {
    let mut out = Vec::new();
    if ctx.level == Level::Project
        && let (Ok(home), Ok(id)) = (
            forge_home::Home::from_env(),
            forge_home::RepoId::for_root(&ctx.base),
        )
    {
        out.extend(forge_launcher::take_notes(&home, &id).unwrap_or_default());
        out.extend(forge_hooks::state::take_repo_notes(&home, &ctx.base));
    }
    out.extend(probe.notes(&ctx.plan).into_iter().map(|n| n.to_string()));
    out.extend(sandbox_note(ctx));
    out
}

/// The sandbox the settings turn on, when this Linux machine lacks what it runs on: Claude Code
/// then runs commands without it, saying so only on its own stderr.
fn sandbox_note(ctx: &Ctx) -> Option<String> {
    if !cfg!(target_os = "linux")
        || !ctx
            .plan
            .settings
            .iter()
            .any(|e| e.entry.path.starts_with("sandbox"))
    {
        return None;
    }
    sandbox_missing(&std::env::var("PATH").unwrap_or_default())
}

/// The note for a `PATH` that lacks what Claude Code's Linux sandbox runs on (`bwrap`, `socat`).
fn sandbox_missing(path: &str) -> Option<String> {
    let missing: Vec<&str> = ["bwrap", "socat"]
        .into_iter()
        .filter(|t| {
            !path
                .split(':')
                .any(|d| !d.is_empty() && forge_check::tools::is_exe(&Path::new(d).join(t)))
        })
        .collect();
    (!missing.is_empty()).then(|| {
        format!(
            "Claude Code's sandbox is turned on but cannot run here ({} not installed; e.g. apt install bubblewrap socat): Claude Code runs commands without it.",
            missing.join(" and ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::sandbox_missing;

    #[test]
    fn the_sandbox_note_names_what_is_missing() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let path = bin.display().to_string();
        assert!(
            sandbox_missing(&path)
                .unwrap()
                .contains("(bwrap and socat not installed;")
        );
        use std::os::unix::fs::PermissionsExt;
        for t in ["bwrap", "socat"] {
            std::fs::write(bin.join(t), "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(bin.join(t), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(sandbox_missing(&path).is_none());
        std::fs::remove_file(bin.join("socat")).unwrap();
        assert!(
            sandbox_missing(&path)
                .unwrap()
                .contains("(socat not installed;")
        );
    }
}
