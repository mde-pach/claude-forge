//! Derived lock files kept in sync with their manifest (§10.5): uv's `uv.lock` (`forge_uv_lock`)
//! and the JavaScript package managers' locks (`forge_js_lock <npm|pnpm|yarn|bun>`).

use forge_check::builtins::{RunMode, ToolRun};
use forge_core::{Finding, NoteKind, Report};

use crate::Run;

/// A package manager's lock: the files it may be, the manifest it is derived from, and the
/// command that writes it (for the agent and the person).
pub struct Lock {
    pub tool: &'static str,
    /// Its possible names, the one the tool reads first where several exist.
    pub files: &'static [&'static str],
    /// The name the tool creates.
    pub default: &'static str,
    pub manifest: &'static str,
    pub command: &'static str,
}

pub const UV: Lock = Lock {
    tool: "uv",
    files: &["uv.lock"],
    default: "uv.lock",
    manifest: "pyproject.toml",
    command: "uv lock",
};

/// The lock of a JavaScript package manager.
pub fn js(pm: &str) -> Option<Lock> {
    Some(match pm {
        "npm" => Lock {
            tool: "npm",
            // npm writes npm-shrinkwrap.json where there is one
            files: &["npm-shrinkwrap.json", "package-lock.json"],
            default: "package-lock.json",
            manifest: "package.json",
            command: "npm install --package-lock-only",
        },
        "pnpm" => Lock {
            tool: "pnpm",
            files: &["pnpm-lock.yaml"],
            default: "pnpm-lock.yaml",
            manifest: "package.json",
            command: "pnpm install --lockfile-only",
        },
        "yarn" => Lock {
            tool: "yarn",
            files: &["yarn.lock"],
            default: "yarn.lock",
            manifest: "package.json",
            command: "yarn install --mode=update-lockfile",
        },
        "bun" => Lock {
            tool: "bun",
            files: &["bun.lock", "bun.lockb"],
            default: "bun.lock",
            manifest: "package.json",
            command: "bun install --lockfile-only",
        },
        _ => return None,
    })
}

/// `forge_js_lock <pm>`: the package manager's lock matches the package.json files, kept so as
/// [`keep`] says.
pub(crate) fn js_lock(run: &Run<'_>, args: &[String]) -> Report {
    match args.first().and_then(|pm| js(pm)) {
        Some(l) => keep(run, &l),
        None => run.only_note(
            NoteKind::ForgeFailure,
            format!(
                "forge_js_lock needs npm, pnpm, yarn or bun, not {:?}",
                args.first().map(String::as_str).unwrap_or("")
            ),
        ),
    }
}

/// The last non-empty line of a tool's output: its error.
fn last_line(text: &str) -> &str {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// The lock matches its manifest, and Forge keeps it so (§10.5): the lock is derived, so a stale
/// one is updated rather than reported. In the agent's turn, the update is advice; in a commit
/// (`--changed`), a refusal to commit again with the new lock; in CI (`--all`, `--since`),
/// nothing is written and a stale or missing lock is a finding. A change is never held to a lock
/// the project never had: creating it is setup, which `forge apply` offers (§3.5).
///
/// The rules run this in the `codegen` stage, after the `lint` stage's new-dependency checks
/// (`supply-chain/new-npm-dep-exists`, `new-pypi-dep-exists`): a run stops after a failing stage,
/// so a dependency name that does not exist, or is too new, is never resolved into the lock.
pub(crate) fn keep(run: &Run<'_>, l: &Lock) -> Report {
    run.check.mark_attributed();
    let mut r = Report::default();
    let root = run.root();
    let changed = |f: &str| run.changed().iter().any(|c| c == f);
    let now = l.files.iter().find(|f| run.exists(f));
    let lock = *now.unwrap_or(&l.default);
    let ci = run.all() || run.check.mode == RunMode::Since;
    if !ci {
        let before = l.files.iter().any(|f| {
            if changed(f) {
                run.base_path(f).is_some()
            } else {
                run.exists(f)
            }
        });
        if now.is_none() && !before {
            return r;
        }
    } else if now.is_none() {
        r.findings.push(run.finding(format!(
            "{lock} is missing: create it with `{}` and commit it, so every install resolves the same versions.",
            l.command
        )));
        return r;
    }
    match run.opts.tools.run(root, root, l.tool, "lock-check", &[]) {
        ToolRun::Ran(o) if o.status == 0 => return r,
        ToolRun::Ran(_) => {}
        ToolRun::Missing(why) => {
            r.notes
                .push(run.note(NoteKind::MissingTool, format!("skipped: {why}")));
            return r;
        }
        ToolRun::Unsupported(why) => {
            r.notes.push(run.note(NoteKind::NotEnforced, why));
            return r;
        }
    }
    let stale = format!(
        "{lock} does not match {}: run `{}` and commit the result.",
        l.manifest, l.command
    );
    if ci {
        r.findings.push(run.finding(stale));
        return r;
    }
    match run.opts.tools.run(root, root, l.tool, "lock", &[]) {
        ToolRun::Ran(o) if o.status == 0 => {
            let text = format!("{lock} was updated to match {}.", l.manifest);
            if run.check.mode == RunMode::Changed {
                r.findings
                    .push(run.finding(format!("{text} Add it and commit again.")));
            } else {
                r.findings.push(Finding::advice(run.ctx.rule.clone(), text));
            }
        }
        ToolRun::Ran(o) => {
            let why = last_line(&o.stderr);
            r.findings.push(run.finding(format!(
                "{} does not resolve, so {lock} cannot be updated: {}",
                l.manifest,
                if why.is_empty() {
                    last_line(&o.stdout)
                } else {
                    why
                }
            )));
        }
        ToolRun::Missing(why) => r
            .notes
            .push(run.note(NoteKind::MissingTool, format!("skipped: {why}"))),
        // this release checks the lock but cannot update it alone: the stale lock is the
        // change's to fix
        ToolRun::Unsupported(why) => {
            r.findings.push(run.finding(stale));
            r.notes.push(run.note(NoteKind::NotEnforced, why));
        }
    }
    r
}
