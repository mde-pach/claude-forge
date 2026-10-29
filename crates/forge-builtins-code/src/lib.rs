//! The builtins of the code packs, in Rust: the `forge_*` functions of the original engine's
//! `javascript.sh`, `supply-chain.sh`, `repository.sh`, `ai-models.sh` and `tests.sh` that pack
//! checks call from their `run` commands.
//!
//! | builtin | pack rule |
//! |---|---|
//! | `bun_cooldown` | typescript/bun/release-age |
//! | `trusted_deps` | typescript/bun/trusted-deps |
//! | `lockfile_lint` | typescript/npm/lockfile-lint (pinned lockfile-lint) |
//! | `dep_major_match` | typescript/express/types-match |
//! | `osv` | supply-chain/vuln-audit (pinned osv-scanner) |
//! | `gitleaks` | secrets/gitleaks (pinned gitleaks) |
//! | `credential_files` | secrets/no-credential-files |
//! | `env_files` | secrets/env-not-committed |
//! | `artifacts` | git/no-build-artifacts |
//! | `generated_edits` | git/generated-files (under `forge_advise`) |
//! | `editorconfig` | git/editorconfig (pinned editorconfig-checker) |
//! | `model_ids` | ai-models/deprecated-model-ids (under `forge_advise`) |
//! | `snapshot_stale` | typescript/vitest/snapshot-stale (under `forge_advise`) |
//!
//! The data builtins those files also define (`forge_toml_get`, `forge_pkg_deps`,
//! `forge_related_py_tests`) and `forge_large_files` / `forge_lockfile_updated` are the checker's.
//!
//! Each builtin reads [`BuiltinCtx`] and the checker's extra context ([`Extra`]: the check's
//! files, the added lines, the file roles, the running rule, the active checks), which a builtin
//! process finds in the environment. Its findings are for the agent; a missing tool or a tool
//! that cannot run is a note for the person, and the builtin then passes.

pub mod ctx;
pub mod js;
pub mod models;
pub mod repo;
pub mod scope;
pub mod snapshots;
pub mod supply;
pub mod tools;
mod util;

use std::sync::Arc;

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};

pub use ctx::Extra;
pub use tools::{OnPath, PinnedTools, Provisioned};

/// Forge's one release cooldown (3 days), in seconds.
pub const COOLDOWN_SECONDS: u64 = 259_200;

/// How the builtins run.
#[derive(Clone)]
pub struct Options {
    /// The minimum age of a version the package managers' cooldown settings must ask for.
    pub cooldown_seconds: u64,
    /// Where Forge's pinned tools (gitleaks, osv-scanner, editorconfig-checker, lockfile-lint)
    /// are.
    pub tools: Arc<dyn PinnedTools>,
}

impl Default for Options {
    /// The tools Forge's cache already holds (never downloads), the 3-day cooldown.
    fn default() -> Self {
        Options {
            cooldown_seconds: COOLDOWN_SECONDS,
            tools: tools::default_tools(),
        }
    }
}

/// A builtin of this crate: [`Builtin::run`] reads the extra context from the environment;
/// [`CodeBuiltin::run_with`] takes it (tests, an in-process caller).
pub trait CodeBuiltin: Send + Sync {
    fn name(&self) -> &'static str;
    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, args: &[String]) -> Report;
}

struct FromEnv(Box<dyn CodeBuiltin>);

impl Builtin for FromEnv {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        self.0.run_with(ctx, &Extra::from_env(ctx), args)
    }
}

/// Every builtin of this crate, each reading the checker's context from the environment.
pub fn builtins(opts: &Options) -> Vec<Box<dyn Builtin + Send>> {
    code_builtins(opts)
        .into_iter()
        .map(|b| Box::new(FromEnv(b)) as Box<dyn Builtin + Send>)
        .collect()
}

/// Every builtin of this crate, taking the extra context as an argument.
pub fn code_builtins(opts: &Options) -> Vec<Box<dyn CodeBuiltin>> {
    let o = Arc::new(opts.clone());
    vec![
        Box::new(js::BunCooldown(o.clone())),
        Box::new(js::TrustedDeps),
        Box::new(js::LockfileLint(o.clone())),
        Box::new(js::DepMajorMatch),
        Box::new(supply::Osv(o.clone())),
        Box::new(repo::Gitleaks(o.clone())),
        Box::new(repo::CredentialFiles),
        Box::new(repo::EnvFiles),
        Box::new(repo::Artifacts),
        Box::new(repo::GeneratedEdits),
        Box::new(repo::Editorconfig(o)),
        Box::new(models::ModelIds),
        Box::new(snapshots::SnapshotStale),
    ]
}

/// Runs builtin `name` with `x`, the accepted changes dropped (as [`BuiltinCtx::finish`]); None
/// when this crate has no such builtin.
pub fn run(
    opts: &Options,
    name: &str,
    ctx: &BuiltinCtx,
    x: &Extra,
    args: &[String],
) -> Option<Report> {
    let name = name.strip_prefix("forge_").unwrap_or(name);
    let b = code_builtins(opts).into_iter().find(|b| b.name() == name)?;
    let mut r = b.run_with(ctx, x, args);
    r.findings
        .retain(|f| !ctx.accepted.iter().any(|a| *a == f.agent_text()));
    Some(r)
}
