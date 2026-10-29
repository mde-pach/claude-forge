//! The built-in checks: each reads the whole parsed command line.

pub mod ephemeral;
pub mod package;

use crate::ctx::GuardCtx;
use crate::parse::Parsed;
use crate::rule::Check;

/// What a check found: the detail shown after the reason ("Here: …"), and a reason of its own
/// when the check words it (python-pm).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub reason: Option<String>,
    pub detail: String,
}

pub fn run(check: Check, parsed: &Parsed, raw: &str, ctx: &GuardCtx) -> Option<Hit> {
    match check {
        Check::EphemeralExec => ephemeral::check(parsed, raw, ctx).map(|detail| Hit {
            reason: None,
            detail,
        }),
        Check::PackageAdd => package::package_add(parsed, ctx).map(|detail| Hit {
            reason: None,
            detail,
        }),
        Check::PythonPm => package::python_pm(parsed, ctx).map(|(reason, detail)| Hit {
            reason: Some(reason),
            detail,
        }),
    }
}
