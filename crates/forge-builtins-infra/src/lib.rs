//! The infrastructure builtins pack checks call: Dockerfiles (`forge_hadolint`,
//! `forge_docker_*`), Kubernetes and Helm (`forge_kube_linter`, `forge_helm`,
//! `forge_chart_version`), Terraform and OpenTofu (`forge_tf_validate`, `forge_tflint`,
//! `forge_trivy_config`), GitHub Actions (`forge_zizmor`, `forge_actionlint`, `forge_ci_jobs`) and
//! OpenAPI (`forge_openapi_lint`, `forge_openapi_ratchet`, `forge_oasdiff`, `forge_openapi_sync`).
//! Ported from the shell engine of the TypeScript Forge (0.5).
//!
//! Each is a [`CheckBuiltin`] named after its shell function without `forge_`: it reads
//! forge-core's [`BuiltinCtx`] and the checker's [`CheckCtx`] (the check's files, the file roles,
//! the running rule, the run's scratch directory).
//!
//! Tools come from forge-check's [`BuiltinTools`]: Forge's pinned tools (hadolint, zizmor,
//! actionlint, kube-linter, kubeconform, tflint and its rulesets, trivy, oasdiff, redocly,
//! spectral) and the project's own (terraform / tofu, helm) as the checker's adapters drive
//! them. Whether a missing pinned tool may be downloaded is the checker's to say (Stop and
//! `forge check`, never after an edit).
//!
//! A tool that is missing, or that cannot run, is a note for the person and never fails the
//! check; what the change got wrong is a finding for the agent.

pub mod cx;
mod docker;
mod github_actions;
mod kubernetes;
mod openapi;
mod terraform;

use std::path::PathBuf;
use std::sync::Arc;

use forge_check::builtins::{BuiltinTools, CheckBuiltin, CheckCtx, NoTools};
use forge_core::Report;
use forge_core::builtin::BuiltinCtx;

pub use cx::Cx;

/// What the builtins run tools with.
#[derive(Clone)]
pub struct Options {
    pub tools: Arc<dyn BuiltinTools>,
    /// A cache directory of Forge's (kubeconform's schemas, trivy's checks); None: a temporary one.
    pub cache_dir: Option<PathBuf>,
}

impl Options {
    pub fn new(tools: Arc<dyn BuiltinTools>) -> Options {
        Options {
            tools,
            cache_dir: None,
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Options::new(Arc::new(NoTools))
    }
}

type RunFn = fn(&Cx<'_>, &[String]) -> Report;

/// Every builtin of this crate: its name and implementation.
const TABLE: &[(&str, RunFn)] = &[
    ("hadolint", docker::hadolint),
    ("docker_nonroot", docker::nonroot),
    ("docker_secrets", docker::secrets),
    ("docker_locked", docker::locked),
    ("docker_ignore", docker::ignore),
    ("docker_copy_order", docker::copy_order),
    ("chart_version", kubernetes::chart_version),
    ("kube_linter", kubernetes::kube_linter),
    ("helm", kubernetes::helm),
    ("tf_validate", terraform::tf_validate),
    ("tflint", terraform::tflint),
    ("trivy_config", terraform::trivy_config),
    ("zizmor", github_actions::zizmor),
    ("actionlint", github_actions::actionlint),
    ("ci_jobs", github_actions::ci_jobs),
    ("openapi_lint", openapi::openapi_lint),
    ("openapi_ratchet", openapi::openapi_ratchet),
    ("oasdiff", openapi::oasdiff),
    ("openapi_sync", openapi::openapi_sync),
];

/// The names of the builtins (without `forge_`).
pub fn names() -> Vec<&'static str> {
    TABLE.iter().map(|(n, _)| *n).collect()
}

struct Infra {
    name: &'static str,
    run: RunFn,
    opts: Arc<Options>,
}

impl CheckBuiltin for Infra {
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&self, ctx: &BuiltinCtx, check: &CheckCtx, args: &[String]) -> Report {
        (self.run)(&Cx::new(ctx, check, &self.opts), args)
    }
}

/// Every builtin, for the checker's registry.
pub fn builtins(opts: &Options) -> Vec<Box<dyn CheckBuiltin>> {
    let opts = Arc::new(opts.clone());
    TABLE
        .iter()
        .map(|(name, run)| {
            Box::new(Infra {
                name,
                run: *run,
                opts: opts.clone(),
            }) as Box<dyn CheckBuiltin>
        })
        .collect()
}

/// Runs builtin `name` in-process; None when there is no such builtin.
pub fn run(
    opts: &Options,
    name: &str,
    ctx: &BuiltinCtx,
    check: &CheckCtx,
    args: &[String],
) -> Option<Report> {
    let (_, f) = TABLE.iter().find(|(n, _)| *n == name)?;
    Some(f(&Cx::new(ctx, check, opts), args))
}
