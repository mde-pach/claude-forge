//! The commit-time check (§10.3) at the command line: offering it, and saying whether commits
//! are checked.

use forge_packs::model::Level;
use forge_render::lock::LockBlock;
use forge_render::precommit::{
    CHECK, PRE_COMMIT_HOOK, block_hash, has_block, hook_manager, insert_block, manager_inactive,
    pre_commit_path, runs_forge_check,
};
use serde::Serialize;

use super::io::{Error, confirm, show_diff, stdin_is_terminal};
use super::{Ctx, Flags};

fn read(ctx: &Ctx, rel: &str) -> Option<String> {
    std::fs::read_to_string(ctx.base.join(rel)).ok()
}

/// Offers the commit-time check: git's own hook, or a block in the hook manager's config.
/// Returns whether the plan changed.
pub fn offer(ctx: &mut Ctx, flags: &Flags) -> Result<bool, Error> {
    if ctx.level != Level::Project || flags.dry_run {
        return Ok(false);
    }
    let p = pre_commit_path(&ctx.base);
    if ctx.plan.files.contains_key(&p) || !ctx.blocks.is_empty() {
        return Ok(false);
    }
    if !flags.yes && !flags.pre_commit && !stdin_is_terminal() {
        return Ok(false);
    }
    let Some(m) = hook_manager(&ctx.base) else {
        if ctx.base.join(&p).exists() {
            return Ok(false);
        }
        if !confirm(
            &format!("forge: create {p} running \"{CHECK}\"?"),
            flags.yes || flags.pre_commit,
        )? {
            return Ok(false);
        }
        ctx.plan.files.insert(p, PRE_COMMIT_HOOK.into());
        return Ok(true);
    };
    let Some(file) = m.file.clone() else {
        return Ok(false);
    };
    // the manager's config is the team's, usually tracked: --yes alone does not edit it
    if !flags.pre_commit && !stdin_is_terminal() {
        return Ok(false);
    }
    let before = read(ctx, &file);
    if before.as_deref().is_some_and(runs_forge_check) {
        return Ok(false);
    }
    let Some(after) = insert_block(m.kind, before.as_deref()) else {
        return Ok(false);
    };
    show_diff(before.as_deref(), Some(&after), &file);
    if !flags.pre_commit
        && !confirm(
            &format!("forge: add \"{CHECK}\" to {file} ({})?", m.name),
            false,
        )?
    {
        return Ok(false);
    }
    ctx.blocks.push(LockBlock {
        file,
        kind: m.kind,
        created: before.is_none(),
        hash: block_hash(&after).unwrap_or_default(),
    });
    Ok(true)
}

/// Whether commits are checked.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PreCommitState {
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Forge's own hook or block (not the person's line).
    pub forge: bool,
    /// The command that installs the hook manager in this clone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activate: Option<String>,
}

pub fn state(ctx: &Ctx) -> PreCommitState {
    let pc = pre_commit_path(&ctx.base);
    if ctx.prev.files.contains_key(&pc) && ctx.base.join(&pc).exists() {
        return PreCommitState {
            installed: true,
            file: Some(pc),
            forge: true,
            activate: None,
        };
    }
    let m = hook_manager(&ctx.base);
    let mut found: Option<(String, bool)> = ctx
        .prev
        .blocks
        .iter()
        .find(|b| has_block(&read(ctx, &b.file).unwrap_or_default()))
        .map(|b| (b.file.clone(), true));
    let own = match &m {
        Some(m) => m.file.clone(),
        None => Some(pc),
    };
    if found.is_none()
        && let Some(own) = own
        && runs_forge_check(&read(ctx, &own).unwrap_or_default())
    {
        found = Some((own, false));
    }
    let Some((file, forge)) = found else {
        return PreCommitState::default();
    };
    let activate = m
        .as_ref()
        .filter(|m| m.file.as_deref() == Some(file.as_str()))
        .and_then(|m| manager_inactive(&ctx.base, m));
    PreCommitState {
        installed: activate.is_none(),
        file: Some(file),
        forge,
        activate,
    }
}

/// How to install the commit-time check.
pub fn hint(ctx: &Ctx) -> String {
    let pc = pre_commit_path(&ctx.base);
    match hook_manager(&ctx.base) {
        None if ctx.base.join(&pc).exists() => {
            format!("add \"{CHECK}\" to {pc} (forge does not touch an existing hook)")
        }
        None => format!(
            "run forge apply interactively (or with --yes) to create {pc}, or add \"{CHECK}\" to your hook manager"
        ),
        Some(m) => match &m.file {
            Some(f) if insert_block(m.kind, read(ctx, f).as_deref()).is_some() => format!(
                "run forge apply interactively (or with --pre-commit) to add \"{CHECK}\" to {f}, or add it yourself"
            ),
            _ => format!("add \"{CHECK}\" to {}", m.name),
        },
    }
}

/// The line apply and status print when commits are not checked.
pub fn not_checked(ctx: &Ctx, st: &PreCommitState) -> String {
    match (&st.activate, &st.file) {
        (Some(a), Some(f)) => format!(
            "{f} runs the check{}, but its hook manager is not installed in this clone, so commits are not checked. To activate: {a}",
            if st.forge { " (forge)" } else { "" }
        ),
        _ => format!(
            "NOT INSTALLED, so commits are not checked. To install: {}",
            hint(ctx)
        ),
    }
}
