//! The infrastructure builtins pack checks call: Dockerfiles (`forge_hadolint`,
//! `forge_docker_*`), Kubernetes and Helm (`forge_kube_linter`, `forge_helm`,
//! `forge_chart_version`), Terraform and OpenTofu (`forge_tf_validate`, `forge_tflint`,
//! `forge_trivy_config`), GitHub Actions (`forge_zizmor`, `forge_actionlint`, `forge_ci_jobs`) and
//! OpenAPI (`forge_openapi_lint`, `forge_openapi_ratchet`, `forge_oasdiff`, `forge_openapi_sync`).
//! Rust ports of `templates/engine/{docker,kubernetes,terraform,github-actions,openapi}.sh`.
//!
//! Each is a [`Builtin`] named after its shell function without `forge_`. What a builtin reads:
//! forge-core's [`BuiltinCtx`], plus what the checker sets for a check command ([`Extra`]: the
//! check's files, the file roles, the running rule, the run's scratch directory).
//!
//! Tools:
//! - Forge's pinned tools (hadolint, zizmor, actionlint, kube-linter, kubeconform, tflint and its
//!   rulesets, trivy, oasdiff, redocly, spectral) come from a [`forge_tools::ToolResolver`]. In
//!   the hooks ([`Scope::Turn`](forge_core::builtin::Scope::Turn)) a builtin only `resolve`s
//!   (never downloads: the person is not waiting, and session start provisions them); under
//!   `forge check` and CI (`changed`, `all`) it `ensure`s, which may download.
//! - The project's own tools (terraform / tofu, helm) come from a [`ProjectTools`]; the
//!   checker's adapter layer implements it (version ranges), [`PathTools`] finds them on PATH.
//!
//! A tool that is missing, or that cannot run, is a note for the person and never fails the
//! check; what the change got wrong is a finding for the agent.

pub mod cx;
mod docker;
mod github_actions;
mod kubernetes;
mod openapi;
mod terraform;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_core::builtin::{Builtin, BuiltinCtx};
use forge_core::{Note, NoteKind, Report};

pub use cx::{Cx, Extra};

/// The project's own infrastructure CLIs (terraform, tofu, helm), as the checker's adapters
/// resolve them.
pub trait ProjectTools: Send + Sync {
    /// The words that run `tool` for `op` (`validate`, `lint`, `template`), before the
    /// builtin's own arguments; a note when it is not installed or its version cannot do `op`.
    fn command(&self, root: &Path, tool: &str, op: &str) -> Result<Vec<String>, Note>;
}

/// The project tools on PATH, any version.
#[derive(Debug, Clone, Default)]
pub struct PathTools {
    /// PATH, when not the process's.
    pub path: Option<String>,
}

impl ProjectTools for PathTools {
    fn command(&self, _root: &Path, tool: &str, _op: &str) -> Result<Vec<String>, Note> {
        let path = self
            .path
            .clone()
            .or_else(|| std::env::var("PATH").ok())
            .unwrap_or_default();
        path.split(':')
            .filter(|d| !d.is_empty())
            .map(|d| Path::new(d).join(tool))
            .find(|p| cx::is_exe(p))
            .map(|p| vec![p.to_string_lossy().into_owned()])
            .ok_or_else(|| Note::new(NoteKind::MissingTool, format!("{tool} is not installed")))
    }
}

/// Forge's pinned tools.
pub type Pinned = Arc<dyn forge_tools::ToolResolver + Send + Sync>;

/// What the builtins run tools with.
#[derive(Clone)]
pub struct Options {
    pub pinned: Pinned,
    pub project: Arc<dyn ProjectTools>,
    /// A cache directory of Forge's (kubeconform's schemas, trivy's checks); None: a temporary one.
    pub cache_dir: Option<PathBuf>,
}

impl Options {
    pub fn new(pinned: Pinned) -> Options {
        Options {
            pinned,
            project: Arc::new(PathTools::default()),
            cache_dir: None,
        }
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

impl Builtin for Infra {
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        let cx = Cx::new(ctx, Extra::from_env(), &self.opts);
        (self.run)(&cx, args)
    }
}

/// Every builtin, for the checker's registry. Each reads [`Extra`] from the environment the
/// checker sets for a check command.
pub fn builtins(opts: &Options) -> Vec<Box<dyn Builtin + Send>> {
    let opts = Arc::new(opts.clone());
    TABLE
        .iter()
        .map(|(name, run)| {
            Box::new(Infra {
                name,
                run: *run,
                opts: opts.clone(),
            }) as Box<dyn Builtin + Send>
        })
        .collect()
}

/// Runs builtin `name` in-process with an explicit [`Extra`]; None when there is no such builtin.
pub fn run(
    opts: &Options,
    name: &str,
    ctx: &BuiltinCtx,
    extra: Extra,
    args: &[String],
) -> Option<Report> {
    let (_, f) = TABLE.iter().find(|(n, _)| *n == name)?;
    let cx = Cx::new(ctx, extra, opts);
    Some(f(&cx, args))
}
