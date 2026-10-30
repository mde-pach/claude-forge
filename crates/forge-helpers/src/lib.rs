//! The checks packs call as helpers, in Rust: the TypeScript Forge's Python and JavaScript helpers
//! ported so that Forge needs no Python, Bun or Node of its own.
//!
//! Each helper is a [`Builtin`] named after the helper's file stem: a pack's
//! `forge_helper <runtime> <file> [args]` or `forge_ratchet <runtime> <file>` runs the builtin
//! [`name_for_script`]`(file)` with `args` (the file name may be passed as the first argument: it
//! picks the Python or the JavaScript half of `ratchet`, `new_deps` and `public_api`; without it
//! both halves run).
//!
//! What a builtin reads comes from its [`BuiltinCtx`]: the root, the base copies of the changed
//! files, the changed files, the accepted changes. Its findings are written for the agent; what
//! stops it from checking (a registry that does not answer, a file it cannot read) is a note.

pub mod drf;
pub mod fk_index;
pub mod new_deps;
pub mod node_version;
pub mod pm_policy;
pub mod public_api;
pub mod pyast;
pub mod pytest_fw;
pub mod ratchet;
pub mod untyped;
pub mod util;

use std::path::PathBuf;
use std::time::Duration;

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};

/// What the agent reads after a change the user may accept (a loosened setting, an approved
/// install script).
pub const ACCEPT_HINT: &str = "Restore it, or ask the user to accept this change.";

/// How the helpers that call registries behave.
#[derive(Debug, Clone)]
pub struct Options {
    /// Where registry answers are cached; None: no cache.
    pub cache_dir: Option<PathBuf>,
    /// The time one registry request may take.
    pub timeout: Duration,
    /// How long a cached registry answer is used.
    pub cache_ttl: Duration,
    /// The PyPI base URL (a mirror; the tests).
    pub pypi: String,
    /// The npm registry base URL.
    pub npm: String,
}

impl Default for Options {
    fn default() -> Self {
        let env = |k: &str, d: &str| {
            std::env::var(k)
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| d.to_owned())
        };
        Options {
            cache_dir: None,
            timeout: Duration::from_secs(5),
            cache_ttl: Duration::from_secs(3600),
            pypi: env("FORGE_REGISTRY_PYPI", "https://pypi.org"),
            npm: env("FORGE_REGISTRY_NPM", "https://registry.npmjs.org"),
        }
    }
}

/// Every helper builtin. Each drops the findings the user accepted (see [`accepts`]).
pub fn builtins(opts: &Options) -> Vec<Box<dyn Builtin + Send>> {
    vec![
        Box::new(Accepting(ratchet::Ratchet)),
        Box::new(Accepting(new_deps::NewDeps::new(opts.clone()))),
        Box::new(Accepting(public_api::PublicApi)),
        Box::new(Accepting(drf::DrfPermissions)),
        Box::new(Accepting(fk_index::FkIndex)),
        Box::new(Accepting(pytest_fw::PytestFilterwarnings)),
        Box::new(Accepting(node_version::NodeVersion)),
        Box::new(Accepting(pm_policy::PmPolicy)),
    ]
}

/// A builtin whose accepted findings are dropped, with the forms accept entries were written in
/// before (see [`accepts`]).
pub struct Accepting<B>(pub B);

impl<B: Builtin> Builtin for Accepting<B> {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        let mut r = self.0.run(ctx, args);
        r.findings
            .retain(|f| is_hint(f) || !accepts(&ctx.accepted, &f.agent_text()));
        // the hint is for accepting something that is still reported
        if r.findings.iter().all(is_hint) {
            r.findings.clear();
        }
        r
    }
}

/// A line that says what to do about the findings above it, not a finding of its own.
fn is_hint(f: &forge_core::Finding) -> bool {
    f.path.is_none() && (f.message == ACCEPT_HINT || f.message == drf::HINT)
}

/// Whether an accepted change covers a finding's text. An accept entry holds the finding as it
/// was reported, and may hold it in an older form: `path:0 message` for a finding about a file
/// as a whole, the acceptance sentence of the earlier helpers; it was matched as a part of the
/// entry, which stays true.
pub fn accepts(accepted: &[String], text: &str) -> bool {
    accepted.iter().any(|a| {
        a == text || a.contains(text) || {
            let n = regex::Regex::new(r"^(\S+):0 ")
                .map(|re| re.replace(a, "$1 ").into_owned())
                .unwrap_or_else(|_| a.clone())
                .replace(
                    "; a removal intended for a major version is accepted in forge.yaml (accept)",
                    "; if the removal is intended for a major version, ask the user to accept this change",
                );
            n == text
        }
    })
}

/// The builtin a pack's helper file stands for (`ratchet.py` → `ratchet`).
pub fn name_for_script(script: &str) -> Option<&'static str> {
    let stem = script
        .rsplit('/')
        .next()?
        .trim_end_matches(".py")
        .trim_end_matches(".mjs");
    [
        "ratchet",
        "new_deps",
        "public_api",
        "drf_permissions",
        "fk_index",
        "pytest_filterwarnings",
        "node_version",
        "pm_policy",
    ]
    .into_iter()
    .find(|n| *n == stem)
}

/// Runs a builtin in-process with `ctx`, accepted changes dropped (as [`BuiltinCtx::finish`]).
pub fn run(b: &dyn Builtin, ctx: &BuiltinCtx, args: &[String]) -> Report {
    let mut r = b.run(ctx, args);
    r.findings
        .retain(|f| !ctx.accepted.iter().any(|a| *a == f.agent_text()));
    r
}
