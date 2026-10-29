//! What `apply` would change: files to write or delete, drift (an owned
//! file changed since the last apply) and conflicts (a file Forge would write that is not
//! Forge's), settings entries to add or remove, pre-commit blocks, and the new lock. Also the
//! migration from the TypeScript Forge's layout.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::lock::{
    CLAUDE_CODE_MIN, Lock, LockBlock, OLD_PROJECT_LOCK, OLD_USER_LOCK, OldLock, old_owned_hash,
    plain_relative, resolves_inside, sha256,
};
use crate::plan::{Plan, settings_files};
use crate::precommit::{
    PRE_COMMIT_HOOK, block_files, block_hash, has_block, hook_manager, insert_block,
    pre_commit_path, remove_block,
};
use crate::settings::{SettingsEntry, add_entries, conflicting_scalars, has_entry, remove_entries};
use forge_packs::model::Level;

/// What the last apply left: from `forge.lock`, or from the old layout's lock.
#[derive(Debug, Clone, Default)]
pub struct Previous {
    pub files: BTreeMap<String, String>,
    pub settings: Vec<SettingsEntry>,
    pub blocks: Vec<LockBlock>,
    pub rules: BTreeMap<String, String>,
    pub roles: BTreeMap<String, Vec<String>>,
    pub tools: toml::Table,
    /// The lock's own text (for the diff), when there is a lock.
    pub lock_text: Option<String>,
}

impl From<&Lock> for Previous {
    fn from(l: &Lock) -> Self {
        Previous {
            files: l.files.clone(),
            settings: l.settings.clone(),
            blocks: l.blocks.clone(),
            rules: l.rules.clone(),
            roles: l.roles.clone(),
            tools: l.tools.clone(),
            lock_text: None,
        }
    }
}

/// The hash of an owned file as the lock records it.
pub fn owned_hash(content: &str) -> String {
    sha256(content)
}

fn read(base: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(base.join(rel)).ok()
}

/// Reads and validates the lock of `plan`'s level: a lock naming a path Forge does not write
/// is refused as a whole.
pub fn read_lock(base: &Path, level: Level, lock_path: &str) -> Result<Option<Lock>, String> {
    let Some(text) = read(base, lock_path) else {
        return Ok(None);
    };
    let lock = Lock::parse(&text).map_err(|e| format!("{lock_path}: {e}; fix it or delete it"))?;
    let bad = lock_problems(base, level, &lock.files, &lock.settings, &lock.blocks);
    if !bad.is_empty() {
        return Err(format!("{lock_path}: refused: {}", bad.join("; ")));
    }
    Ok(Some(lock))
}

/// Whether a path is one Forge owns at this level (a rules file).
pub fn is_owned_path(level: Level, p: &str) -> bool {
    level == Level::Project
        && p.strip_prefix(".claude/rules/forge-")
            .is_some_and(|rest| rest.ends_with(".md") && !rest.contains('/'))
}

fn inside(base: &Path, level: Level, p: &str) -> bool {
    plain_relative(p)
        && match level {
            Level::Project => resolves_inside(base, p),
            Level::User => p
                .strip_prefix(".claude/")
                .is_some_and(|rest| resolves_inside(&base.join(".claude"), rest)),
        }
}

fn lock_problems(
    base: &Path,
    level: Level,
    files: &BTreeMap<String, String>,
    settings: &[SettingsEntry],
    blocks: &[LockBlock],
) -> Vec<String> {
    let mut out = Vec::new();
    let hook = (level == Level::Project).then(|| pre_commit_path(base));
    for p in files.keys() {
        if Some(p) != hook.as_ref() && !(is_owned_path(level, p) && inside(base, level, p)) {
            out.push(format!("files: {p:?} is not a path forge owns"));
        }
    }
    for s in settings {
        if !settings_files(level).contains(&s.file.as_str()) || !inside(base, level, &s.file) {
            out.push(format!(
                "settings: {:?} is not a settings file forge appends to",
                s.file
            ));
        }
    }
    for b in blocks {
        let allowed = level == Level::Project
            && match b.kind {
                crate::precommit::BlockKind::Hooks => hook_manager(base)
                    .is_some_and(|m| m.kind == b.kind && m.file.as_deref() == Some(&b.file)),
                k => block_files(k).contains(&b.file.as_str()),
            };
        if !allowed || !inside(base, level, &b.file) {
            out.push(format!(
                "blocks: {:?} is not a {:?} hook file forge adds its block to",
                b.file, b.kind
            ));
        }
    }
    out
}

/// Moving from the TypeScript Forge's layout: what its lock records as Forge's, to delete.
#[derive(Debug, Clone, Default)]
pub struct Migration {
    pub old_lock_path: String,
    /// Files and directories to delete (recorded in the old lock, or always Forge's).
    pub deletes: Vec<String>,
    /// Recorded files changed since the old apply: deleted only with --force.
    pub modified: Vec<String>,
    /// What carries over as the previous state (rules files, the git hook, settings entries, blocks).
    pub previous: Previous,
}

/// The old layout's owned paths: everything Forge wrote under `.forge/` except the inputs
/// (`.forge/rules/`, `.forge/packs/`), its plugin, and the rules files.
fn old_owned(level: Level, p: &str) -> bool {
    match level {
        Level::Project => {
            (p.starts_with(".forge/")
                && !p.starts_with(".forge/rules/")
                && !p.starts_with(".forge/packs/"))
                || p.starts_with(".claude/skills/forge-project/")
                || is_owned_path(level, p)
        }
        Level::User => p.starts_with(".claude/skills/forge-user/"),
    }
}

/// The migration, when the old layout's lock is there.
pub fn detect_migration(base: &Path, level: Level) -> Result<Option<Migration>, String> {
    let lock_path = match level {
        Level::Project => OLD_PROJECT_LOCK,
        Level::User => OLD_USER_LOCK,
    };
    let Some(text) = read(base, lock_path) else {
        return Ok(None);
    };
    let old: OldLock = serde_json::from_str(&text)
        .map_err(|e| format!("{lock_path}: not a lock forge 0.5 wrote ({e})"))?;
    let hook = (level == Level::Project).then(|| pre_commit_path(base));
    let mut m = Migration {
        old_lock_path: lock_path.into(),
        ..Migration::default()
    };
    let mut bad = Vec::new();
    for (p, h) in &old.files {
        let is_hook = Some(p) == hook.as_ref();
        if !is_hook && (!old_owned(level, p) || !inside(base, level, p)) {
            bad.push(format!("files: {p:?} is not a path forge owned"));
            continue;
        }
        if is_hook || is_owned_path(level, p) {
            // rewritten by this apply (or deleted when no pack wants it): owned from now on
            m.previous.files.insert(p.clone(), h.clone());
            continue;
        }
        let Some(content) = read(base, p) else {
            continue;
        };
        if old_owned_hash(p, &content) == *h {
            m.deletes.push(p.clone());
        } else {
            m.modified.push(p.clone());
        }
    }
    for s in &old.settings {
        if !settings_files(level).contains(&s.file.as_str()) || !inside(base, level, &s.file) {
            bad.push(format!(
                "settings: {:?} is not a settings file forge appended to",
                s.file
            ));
        }
    }
    let blocks: Vec<LockBlock> = old.blocks.clone();
    if !lock_problems(base, level, &BTreeMap::new(), &[], &blocks).is_empty() {
        bad.push("blocks: a block in a file forge does not add its block to".into());
    }
    if !bad.is_empty() {
        return Err(format!("{lock_path}: refused: {}", bad.join("; ")));
    }
    if level == Level::Project {
        for extra in [".forge/manifest.sha256", ".forge/state"] {
            if base.join(extra).exists() && !m.deletes.iter().any(|d| d == extra) {
                m.deletes.push(extra.into());
            }
        }
    }
    m.deletes.sort();
    m.previous.settings = old.settings;
    m.previous.blocks = blocks;
    m.previous.rules = old.rules;
    Ok(Some(m))
}

#[derive(Debug, Clone)]
pub struct FileWrite {
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Debug, Clone)]
pub struct SettingsChange {
    pub file: String,
    pub before: String,
    /// `None`: the file is left empty, so it is deleted.
    pub after: Option<String>,
    /// Shared with the team (not settings.local.json): its changes are confirmed.
    pub shared: bool,
}

#[derive(Debug, Clone)]
pub struct BlockWrite {
    pub path: String,
    pub before: Option<String>,
    pub after: String,
    pub created: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Changes {
    pub writes: Vec<FileWrite>,
    pub deletes: Vec<String>,
    pub drift: Vec<String>,
    pub conflicts: Vec<String>,
    pub settings: Vec<SettingsChange>,
    pub blocks: Vec<BlockWrite>,
    pub block_drift: Vec<String>,
    pub missing_entries: Vec<SettingsEntry>,
    pub rules_changed: Vec<String>,
    pub roles_changed: Vec<String>,
    /// Scalar settings set differently by the person (never overwritten), and blocks Forge cannot add back.
    pub overridden: Vec<String>,
    pub lock: Option<Lock>,
    pub lock_text: String,
    /// The lock text differs from the one on disk (compared as data).
    pub lock_changed: bool,
    /// Migration: the old layout's files and directories to delete.
    pub old_deletes: Vec<String>,
    pub old_modified: Vec<String>,
}

impl Changes {
    /// Something apply would write or delete.
    pub fn any(&self) -> bool {
        !self.writes.is_empty()
            || !self.deletes.is_empty()
            || !self.settings.is_empty()
            || !self.blocks.is_empty()
            || !self.old_deletes.is_empty()
            || self.lock_changed
    }
}

/// The plan's changes against the previous state. `blocks` are Forge's blocks to keep (the
/// previous ones plus one apply adds). `forge_version` goes into the lock.
pub fn compute(
    plan: &Plan,
    prev: &Previous,
    blocks: &[LockBlock],
    migration: Option<&Migration>,
    forge_version: &str,
) -> Result<Changes, String> {
    let base = plan.base.as_path();
    let mut ch = Changes::default();
    for (path, after) in &plan.files {
        let before = read(base, path);
        let same = before.as_deref() == Some(after.as_str());
        if let Some(b) = &before {
            match prev.files.get(path) {
                Some(h) if owned_hash(b) != *h => ch.drift.push(path.clone()),
                None if !same => ch.conflicts.push(path.clone()),
                _ => {}
            }
        }
        if !same {
            ch.writes.push(FileWrite {
                path: path.clone(),
                before,
                after: after.clone(),
            });
        }
    }
    for (path, hash) in &prev.files {
        if plan.files.contains_key(path) {
            continue;
        }
        let Some(before) = read(base, path) else {
            continue;
        };
        if owned_hash(&before) != *hash {
            ch.drift.push(path.clone());
        }
        ch.deletes.push(path.clone());
    }

    // settings: re-add recorded entries, add new ones, remove the ones no longer wanted
    let desired: BTreeSet<String> = plan.settings.iter().map(|e| e.entry.key()).collect();
    let mut files: Vec<String> = vec![plan.settings_file.clone()];
    for e in &prev.settings {
        if !files.contains(&e.file) {
            files.push(e.file.clone());
        }
    }
    let mut kept: Vec<SettingsEntry> = Vec::new();
    for file in &files {
        let err = |e: crate::settings::SettingsError| format!("{file}: {e}; fix it and re-run");
        let before = read(base, file).unwrap_or_default();
        let mut text: Option<String> = Some(before.clone());
        let drop: Vec<SettingsEntry> = prev
            .settings
            .iter()
            .filter(|e| &e.file == file && !desired.contains(&e.key()))
            .cloned()
            .collect();
        if !drop.is_empty() && !before.trim().is_empty() {
            text = remove_entries(&before, &drop).map_err(err)?;
        }
        let want: Vec<_> = plan
            .settings
            .iter()
            .filter(|e| &e.entry.file == file)
            .cloned()
            .collect();
        for (e, current) in conflicting_scalars(&before, &want).map_err(err)? {
            ch.overridden.push(format!(
                "{file}: {} is {current}; forge wants {} (left unchanged)",
                e.path, e.value
            ));
        }
        for e in prev
            .settings
            .iter()
            .filter(|e| &e.file == file && desired.contains(&e.key()))
        {
            if !has_entry(&before, e).map_err(err)? {
                ch.missing_entries.push(e.clone());
            }
            kept.push(e.clone());
        }
        if !want.is_empty() {
            let (t, added) = add_entries(text.as_deref().unwrap_or(""), &want).map_err(err)?;
            text = Some(t);
            for e in added {
                if !kept.iter().any(|k| k.key() == e.key()) {
                    kept.push(e);
                }
            }
        }
        let after = text.filter(|t| !t.is_empty());
        if after.as_deref().unwrap_or("") != before {
            ch.settings.push(SettingsChange {
                file: file.clone(),
                before,
                after,
                shared: !file.ends_with("settings.local.json"),
            });
        }
    }
    let order: BTreeMap<String, usize> = plan
        .settings
        .iter()
        .enumerate()
        .map(|(i, e)| (e.entry.key(), i))
        .collect();
    kept.sort_by_key(|e| order.get(&e.key()).copied().unwrap_or(0));

    // pre-commit blocks: added back when missing, brought up to date when Forge's own
    let mut lock_blocks = Vec::new();
    for b in blocks {
        let before = read(base, &b.file);
        let mut rec = b.clone();
        match before.as_deref() {
            Some(text) if has_block(text) => {
                if block_hash(text).as_deref() != Some(b.hash.as_str()) {
                    ch.block_drift.push(b.file.clone());
                } else if let Some(after) = remove_block(b.kind, text)
                    .and_then(|without| insert_block(b.kind, Some(&without)))
                {
                    if after != text {
                        rec.hash = block_hash(&after).unwrap_or_default();
                        ch.blocks.push(BlockWrite {
                            path: b.file.clone(),
                            before: before.clone(),
                            after,
                            created: false,
                        });
                    }
                } else {
                    ch.overridden.push(format!(
                        "{}: forge's pre-commit block cannot be updated (run \"{}\" from it yourself)",
                        b.file,
                        crate::precommit::CHECK
                    ));
                }
            }
            _ => match insert_block(b.kind, before.as_deref()) {
                Some(after) => {
                    rec.hash = block_hash(&after).unwrap_or_default();
                    ch.blocks.push(BlockWrite {
                        path: b.file.clone(),
                        before: before.clone(),
                        after,
                        created: before.is_none(),
                    });
                }
                None => ch.overridden.push(format!(
                    "{}: forge's pre-commit block cannot be added back (add \"{}\" yourself)",
                    b.file,
                    crate::precommit::CHECK
                )),
            },
        }
        lock_blocks.push(rec);
    }

    for (id, hash) in &prev.rules {
        if plan.rules.get(id).is_some_and(|h| h != hash) {
            ch.rules_changed.push(id.clone());
        }
    }
    for r in &plan.roles {
        let now = &plan.role_globs[&r.name];
        if let Some(was) = prev.roles.get(&r.name)
            && was != now
        {
            ch.roles_changed.push(format!(
                "files @{} changed since the last apply: {}",
                r.name,
                role_summary(r)
            ));
        }
    }

    let lock = Lock {
        forge: forge_version.into(),
        claude_code_min: CLAUDE_CODE_MIN.into(),
        rule_set: plan.rule_set.clone(),
        rules: plan.rules.clone(),
        files: plan
            .files
            .iter()
            .map(|(p, t)| (p.clone(), owned_hash(t)))
            .collect(),
        roles: plan.role_globs.clone(),
        tools: prev.tools.clone(),
        settings: kept,
        blocks: lock_blocks,
    };
    ch.lock_text = lock.render();
    let on_disk = read(base, &plan.lock_path);
    ch.lock_changed = match on_disk.as_deref().map(Lock::parse) {
        Some(Ok(l)) => l != lock,
        _ => true,
    };
    ch.lock = Some(lock);
    if let Some(m) = migration {
        ch.old_deletes = m.deletes.clone();
        ch.old_deletes.push(m.old_lock_path.clone());
        ch.old_modified = m.modified.clone();
    }
    Ok(ch)
}

/// Keeps the git hook Forge created earlier in the plan.
pub fn keep_pre_commit_hook(plan: &mut Plan, prev: &Previous) {
    if plan.level != Level::Project {
        return;
    }
    let p = pre_commit_path(&plan.base);
    if prev.files.contains_key(&p) {
        plan.files.insert(p, PRE_COMMIT_HOOK.into());
    }
}

/// A role's patterns with their sources: "a/**, b/** (alembic.ini); c/** (forge.yaml)".
pub fn role_summary(r: &crate::plan::RoleStatus) -> String {
    if r.patterns.is_empty() {
        return "no patterns".into();
    }
    let mut by: Vec<(String, Vec<String>)> = Vec::new();
    for p in &r.patterns {
        let g = format!("{}{}", if p.exclude { "!" } else { "" }, p.glob);
        match by.iter_mut().find(|(s, _)| *s == p.source) {
            Some((_, v)) => v.push(g),
            None => by.push((p.source.clone(), vec![g])),
        }
    }
    by.into_iter()
        .map(|(s, g)| format!("{} ({s})", g.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Line diff (LCS) in unified style without hunk headers; "" when equal (`lineDiff`).
pub fn line_diff(before: &str, after: &str, label: &str) -> String {
    if before == after {
        return String::new();
    }
    let split = |s: &str| -> Vec<String> {
        if s.is_empty() {
            Vec::new()
        } else {
            s.strip_suffix('\n')
                .unwrap_or(s)
                .split('\n')
                .map(str::to_owned)
                .collect()
        }
    };
    let a = split(before);
    let b = split(after);
    let (n, m) = (a.len(), b.len());
    let mut out = vec![format!("--- {label}"), format!("+++ {label}")];
    if n * m > 4_000_000 {
        out.extend(a.iter().map(|l| format!("-{l}")));
        out.extend(b.iter().map(|l| format!("+{l}")));
        return out.join("\n") + "\n";
    }
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops: Vec<String> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push(format!(" {}", a[i]));
            i += 1;
            j += 1;
        } else if j < m && (i == n || dp[i][j + 1] >= dp[i + 1][j]) {
            ops.push(format!("+{}", b[j]));
            j += 1;
        } else {
            ops.push(format!("-{}", a[i]));
            i += 1;
        }
    }
    let mut keep = vec![false; ops.len()];
    for k in 0..ops.len() {
        if !ops[k].starts_with(' ') {
            let end = (k + 2).min(ops.len() - 1);
            keep[k.saturating_sub(2)..=end].fill(true);
        }
    }
    let mut skipped = false;
    for (k, o) in ops.into_iter().enumerate() {
        if keep[k] {
            out.push(o);
            skipped = false;
        } else if !skipped {
            out.push("@@".into());
            skipped = true;
        }
    }
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff() {
        assert_eq!(
            line_diff("a\nb\n", "a\nc\n", "f"),
            "--- f\n+++ f\n a\n+c\n-b\n"
        );
        assert_eq!(line_diff("x", "x", "f"), "");
    }
}
