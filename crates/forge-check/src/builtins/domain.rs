//! The seam the domain builtins (forge-builtins-data, -infra, -code) plug in through: a
//! [`CheckBuiltin`] gets forge-core's context and the checker's ([`CheckCtx`]), and reaches
//! tools through one [`BuiltinTools`], which [`AdapterTools`] answers with the checker's
//! adapters and tool resolver.
//!
//! Whether a missing pinned tool may be downloaded is the resolver's: the checker allows it at
//! Stop and under `forge check` (`FORGE_TOOL_DOWNLOAD`), never after an edit, and
//! `FORGE_NO_DOWNLOAD` turns it off everywhere.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use forge_core::builtin::BuiltinCtx;
use forge_core::{Note, NoteKind, Report};

use super::CheckCtx;
use crate::adapters::{Adapter, DoCtx, resolve};
use crate::tools::ToolResolver;

/// A builtin of a domain crate.
pub trait CheckBuiltin: Send + Sync {
    /// The name without the `forge_` prefix.
    fn name(&self) -> &'static str;
    fn run(&self, ctx: &BuiltinCtx, check: &CheckCtx, args: &[String]) -> Report;
}

/// What a tool printed and how it exited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolOutput {
    /// The exit status; 128 + 9 when a signal ended it.
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl ToolOutput {
    /// stdout then stderr.
    pub fn text(&self) -> String {
        let sep = if self.stdout.is_empty() || self.stdout.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        format!("{}{sep}{}", self.stdout, self.stderr)
    }
}

/// The outcome of asking for one operation of a project tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRun {
    /// It ran (whatever its exit status).
    Ran(ToolOutput),
    /// The project does not have the tool, or it could not start; the text says why.
    Missing(String),
    /// The installed version cannot do the operation; the text says why (for the person).
    Unsupported(String),
}

/// How the domain builtins reach tools.
pub trait BuiltinTools: Send + Sync {
    /// Runs `op` of the project's `tool` (alembic `upgrade-sql`, sqlfluff `lint`, typeorm
    /// `migrate` / `migrations-check`, terraform / tofu `validate`, helm `lint` / `template`…)
    /// in `cwd`, as the tool's adapter says for the installed version, `args` after.
    fn run(&self, root: &Path, cwd: &Path, tool: &str, op: &str, args: &[String]) -> ToolRun;

    /// Forge's pinned `tool`: the words that run it. `Err` says why it cannot be had, for the
    /// person.
    fn pinned(&self, tool: &str) -> Result<Vec<String>, String>;

    /// Whether `op` of `tool` can run from `cwd`: the tool is there, and its adapter covers
    /// the installed version (`forge_condor` leaves unused imports to ruff when it runs).
    fn available(&self, _root: &Path, _cwd: &Path, _tool: &str, _op: &str) -> bool {
        false
    }
}

/// The checker's adapters over a tool resolver.
#[derive(Clone)]
pub struct AdapterTools {
    pub resolver: Arc<dyn ToolResolver>,
    /// Forge's pins, by tool (forge-tools' table); a tool missing here takes its adapter's pin.
    pub pins: BTreeMap<String, String>,
}

impl AdapterTools {
    pub fn new(resolver: Arc<dyn ToolResolver>, pins: BTreeMap<String, String>) -> AdapterTools {
        AdapterTools { resolver, pins }
    }
}

impl BuiltinTools for AdapterTools {
    fn run(&self, root: &Path, cwd: &Path, tool: &str, op: &str, args: &[String]) -> ToolRun {
        let ctx = DoCtx {
            root,
            cwd,
            rule: "",
            tools: self.resolver.as_ref(),
            stdin: None,
        };
        let r = match resolve(tool, op, args, &ctx) {
            Ok(r) => r,
            Err(n) if n.kind == NoteKind::MissingTool => return ToolRun::Missing(n.text),
            Err(n) => return ToolRun::Unsupported(n.text),
        };
        let out = Command::new(&r.cmd.argv[0])
            .args(&r.cmd.argv[1..])
            .args(&r.args)
            .current_dir(cwd)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output();
        let out = match out {
            Ok(o) => o,
            Err(e) => return ToolRun::Missing(format!("{} could not run: {e}", r.name)),
        };
        let status = out.status.code().unwrap_or(128 + 9);
        if let Some(why) = r.usage_error(op, status, &out.stdout, &out.stderr) {
            return ToolRun::Unsupported(why);
        }
        ToolRun::Ran(ToolOutput {
            status,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn available(&self, root: &Path, cwd: &Path, tool: &str, op: &str) -> bool {
        let ctx = DoCtx {
            root,
            cwd,
            rule: "",
            tools: self.resolver.as_ref(),
            stdin: None,
        };
        resolve(tool, op, &[], &ctx).is_ok()
    }

    fn pinned(&self, tool: &str) -> Result<Vec<String>, String> {
        let version = self
            .pins
            .get(tool)
            .cloned()
            .or_else(|| Adapter::load(tool).and_then(|a| a.pinned))
            .ok_or_else(|| format!("Forge does not pin {tool}"))?;
        self.resolver.pinned(tool, &version).map(|c| c.argv)
    }
}

/// No tool at all (tests, and a machine where none can be found).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTools;

impl BuiltinTools for NoTools {
    fn run(&self, _: &Path, _: &Path, tool: &str, _: &str, _: &[String]) -> ToolRun {
        ToolRun::Missing(format!("{tool} is not installed"))
    }

    fn pinned(&self, tool: &str) -> Result<Vec<String>, String> {
        Err(format!("{tool} is not installed"))
    }
}

/// A note about the running rule.
pub fn rule_note(ctx: &BuiltinCtx, kind: NoteKind, text: impl Into<String>) -> Note {
    let n = Note::new(kind, text);
    if ctx.rule.is_empty() {
        n
    } else {
        n.for_rule(ctx.rule.clone())
    }
}
