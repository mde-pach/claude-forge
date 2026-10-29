//! The plan (§3.1): what `forge apply` writes at one level, and what `forge status` says
//! about each rule. The phases, in order: the active packs, where things apply, the rules
//! that apply and what enforces each, then the files and settings entries to write.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use forge_detect::roles::{RolePattern, Roles};
use forge_detect::{Activation, PackStatus, ProjectFacts, Site, active_packs};
use forge_packs::PackSet;
use forge_packs::model::{AuditedTool, Disable, Level, ProjectConfig};
use serde::Serialize;

use crate::context::{PackContext, pack_context, render_rules_files};
use crate::hooks::hook_entries;
use crate::lock::{LOCK_FILE, USER_LOCK_FILE, rule_set_hash};
use crate::rules::{Deprecation, Gated, RuleStatus, SwitchedOff, select};
use crate::settings::{DesiredEntry, SettingsEntry};

pub const SETTINGS_FILE: &str = ".claude/settings.json";
pub const LOCAL_SETTINGS_FILE: &str = ".claude/settings.local.json";

/// Permission asks that protect Forge's inputs and output from being changed unnoticed (§8.3).
pub fn owned_asks(level: Level) -> &'static [&'static str] {
    match level {
        Level::Project => &[
            "Edit(.forge/**)",
            "Edit(forge.yaml)",
            "Edit(forge.lock)",
            "Edit(.claude/rules/forge-*.md)",
        ],
        Level::User => &["Edit(~/.claude/forge-user.lock)"],
    }
}

/// The settings files a level may write to.
pub fn settings_files(level: Level) -> &'static [&'static str] {
    match level {
        Level::Project => &[SETTINGS_FILE, LOCAL_SETTINGS_FILE],
        Level::User => &[SETTINGS_FILE],
    }
}

/// The lock of a level, relative to its base.
pub fn lock_path(level: Level) -> &'static str {
    match level {
        Level::Project => LOCK_FILE,
        Level::User => USER_LOCK_FILE,
    }
}

/// A file role an active rule names, as `forge status` shows it.
#[derive(Debug, Clone, Serialize)]
pub struct RoleStatus {
    pub name: String,
    pub patterns: Vec<RolePattern>,
    pub notes: Vec<String>,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub level: Level,
    /// The repository root (project) or the home directory (user).
    pub base: PathBuf,
    pub settings_file: String,
    pub lock_path: String,
    /// Owned files to write, relative to base: path → text.
    pub files: BTreeMap<String, String>,
    pub settings: Vec<DesiredEntry>,
    /// Active rule id → sha256 of its file.
    pub rules: BTreeMap<String, String>,
    pub packs: Vec<PackStatus>,
    pub removed_packs: Vec<PackStatus>,
    pub active: Vec<RuleStatus>,
    pub disabled: Vec<Disable>,
    pub gated: Vec<Gated>,
    pub switched_off: Vec<SwitchedOff>,
    pub deprecated: Vec<Deprecation>,
    pub warnings: Vec<String>,
    pub roles: Vec<RoleStatus>,
    /// The roles' globs, as the lock records them.
    pub role_globs: BTreeMap<String, Vec<String>>,
    /// Where each audited tool's audits run ("" is the root).
    pub tool_dirs: BTreeMap<AuditedTool, Vec<String>>,
    pub rule_set: String,
}

pub struct PlanInput<'a> {
    pub level: Level,
    pub base: &'a Path,
    pub set: &'a PackSet,
    /// Project only: entries go to `.claude/settings.local.json`.
    pub local: bool,
    pub config: Option<&'a ProjectConfig>,
    pub facts: Option<&'a ProjectFacts>,
    pub roles: &'a Roles,
}

pub fn plan(input: &PlanInput) -> Result<Plan, String> {
    let level = input.level;
    let activation: Activation = active_packs(input.set, level, input.config, input.facts);
    let mut warnings = activation.warnings.clone();
    let site = Site::new(level, input.base, input.set, input.facts, &activation);
    let settings_file = if level == Level::Project && input.local {
        LOCAL_SETTINGS_FILE
    } else {
        SETTINGS_FILE
    };
    let selected = select(
        input.set,
        &site,
        level,
        input.config,
        input.roles,
        settings_file,
        &mut warnings,
    )?;

    let mut settings: Vec<DesiredEntry> = owned_asks(level)
        .iter()
        .map(|v| DesiredEntry {
            entry: SettingsEntry {
                file: settings_file.into(),
                path: "permissions.ask".into(),
                value: serde_json::Value::String((*v).into()),
            },
            append: true,
        })
        .collect();
    settings.extend(selected.settings.iter().cloned());
    settings.extend(hook_entries(level, settings_file));
    let mut seen = BTreeSet::new();
    settings.retain(|e| seen.insert(e.entry.key()));

    let mut files = BTreeMap::new();
    if level == Level::Project {
        let contexts: Vec<PackContext> = activation
            .packs
            .iter()
            .map(|p| PackContext {
                id: &p.id,
                items: pack_context(&input.set.packs[&p.id]),
                dirs: site.pack_dirs(&p.id),
            })
            .collect();
        files = render_rules_files(&contexts, input.facts, &activation);
    }

    let rules: BTreeMap<String, String> = selected
        .active
        .iter()
        .map(|s| (s.rule.id.clone(), s.rule.hash.clone()))
        .collect();
    let roles: Vec<RoleStatus> = if level == Level::Project {
        selected
            .role_use
            .iter()
            .map(|(name, ids)| {
                let r = input.roles.get(name).cloned().unwrap_or_default();
                RoleStatus {
                    name: name.clone(),
                    patterns: r.patterns,
                    notes: r.notes,
                    rules: ids.clone(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let role_globs: BTreeMap<String, Vec<String>> = roles
        .iter()
        .map(|r| {
            let globs = r
                .patterns
                .iter()
                .map(|p| {
                    if p.exclude {
                        format!("!{}", p.glob)
                    } else {
                        p.glob.clone()
                    }
                })
                .collect();
            (r.name.clone(), globs)
        })
        .collect();
    let pack_ids: Vec<String> = activation.packs.iter().map(|p| p.id.clone()).collect();
    let tool_dirs = AuditedTool::ALL
        .iter()
        .map(|t| (*t, site.tool_dirs(*t)))
        .collect();
    Ok(Plan {
        level,
        base: input.base.to_path_buf(),
        settings_file: settings_file.into(),
        lock_path: lock_path(level).into(),
        files,
        settings,
        rule_set: rule_set_hash(&rules, &role_globs, &pack_ids),
        rules,
        packs: activation.packs.clone(),
        removed_packs: activation.removed.clone(),
        active: selected.active,
        disabled: selected.disabled,
        gated: selected.gated,
        switched_off: selected.switched_off,
        deprecated: selected.deprecated,
        warnings,
        roles,
        role_globs,
        tool_dirs,
    })
}
