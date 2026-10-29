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
//! Each builtin is a forge-check [`CheckBuiltin`]: it reads [`BuiltinCtx`] and the checker's
//! context ([`Extra`], forge-check's `CheckCtx`: the check's files, the added lines, the file
//! roles, the running rule, the active checks). Its findings are for the agent; a missing tool
//! or a tool that cannot run is a note for the person, and the builtin then passes.

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

use forge_check::builtins::{BuiltinTools, CheckBuiltin, NoTools};
use forge_core::Report;
use forge_core::builtin::BuiltinCtx;

pub use ctx::Extra;

/// Forge's one release cooldown (3 days), in seconds.
pub const COOLDOWN_SECONDS: u64 = 259_200;

/// How the builtins run.
#[derive(Clone)]
pub struct Options {
    /// The minimum age of a version the package managers' cooldown settings must ask for.
    pub cooldown_seconds: u64,
    /// Where Forge's pinned tools (gitleaks, osv-scanner, editorconfig-checker, lockfile-lint)
    /// are.
    pub tools: Arc<dyn BuiltinTools>,
}

impl Default for Options {
    /// No tool, the 3-day cooldown.
    fn default() -> Self {
        Options {
            cooldown_seconds: COOLDOWN_SECONDS,
            tools: Arc::new(NoTools),
        }
    }
}

/// Every builtin of this crate.
pub fn builtins(opts: &Options) -> Vec<Box<dyn CheckBuiltin>> {
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
    let b = builtins(opts).into_iter().find(|b| b.name() == name)?;
    let mut r = b.run(ctx, x, args);
    r.findings
        .retain(|f| !ctx.accepted.iter().any(|a| *a == f.agent_text()));
    Some(r)
}
