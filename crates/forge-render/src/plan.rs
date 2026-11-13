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
use crate::rules::{Deprecation, Gated, RuleStatus, SettingsTarget, SwitchedOff, select};
use crate::settings::{DesiredEntry, SettingsEntry};

pub const SETTINGS_FILE: &str = ".claude/settings.json";
pub const LOCAL_SETTINGS_FILE: &str = ".claude/settings.local.json";
/// The project's MCP servers, where Forge registers Serena.
pub const MCP_FILE: &str = ".mcp.json";
/// The user level's settings, relative to Claude Code's config directory.
pub const USER_SETTINGS_FILE: &str = "settings.json";

/// Permission asks that protect Forge's inputs and output from being changed unnoticed (§8.3).
pub fn owned_asks(level: Level, base: &Path) -> Vec<String> {
    match level {
        Level::Project => [
            "Edit(.forge/**)",
            "Edit(forge.yaml)",
            "Edit(forge.lock)",
            "Edit(.claude/rules/forge-*.md)",
        ]
        .map(String::from)
        .to_vec(),
        Level::User => vec![format!(
            "Edit({})",
            permission_path(&base.join(USER_LOCK_FILE))
        )],
    }
}

/// An absolute path as a permission rule names it: `~/…` under the home directory, else
/// `//…` (Claude Code's absolute-path form).
fn permission_path(p: &Path) -> String {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from);
    match home.as_deref().and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => format!("/{}", p.display()),
    }
}

/// How a file of a level is named to the person: relative to the repository (project), with
/// its directory spelled out (user: `~/.claude/settings.json`).
pub fn display_path(level: Level, base: &Path, rel: &str) -> String {
    match level {
        Level::Project => rel.to_string(),
        Level::User => {
            let p = base.join(rel);
            let home = std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(PathBuf::from);
            match home.as_deref().and_then(|h| p.strip_prefix(h).ok()) {
                Some(rest) => format!("~/{}", rest.display()),
                None => p.display().to_string(),
            }
        }
    }
}

/// The settings files a level may write to.
pub fn settings_files(level: Level) -> &'static [&'static str] {
    match level {
        Level::Project => &[SETTINGS_FILE, LOCAL_SETTINGS_FILE, MCP_FILE],
        Level::User => &[USER_SETTINGS_FILE],
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

/// Serena, every session's (§17.8): its project files in `.serena/`, its server in `.mcp.json`,
/// and the setting that lets Claude Code start it without asking.
fn serena_setup(
    base: &Path,
    settings_file: &str,
    languages: &[forge_core::serena::Language],
) -> (BTreeMap<String, String>, Vec<DesiredEntry>) {
    use forge_core::serena;
    let name = base
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let files = BTreeMap::from([
        (
            serena::PROJECT_FILE.to_string(),
            serena::project_yml(&name, languages),
        ),
        (
            serena::GITIGNORE_FILE.to_string(),
            serena::GITIGNORE.to_string(),
        ),
    ]);
    let version = forge_tools_pin(serena::TOOL);
    // its tools load with the session, not on demand (Claude Code's tool search would hide them)
    let server = serde_json::json!({
        "command": "sh",
        "args": ["-c", serena::mcp_command(&version)],
        "alwaysLoad": true,
    });
    let settings = vec![
        DesiredEntry {
            entry: SettingsEntry {
                file: MCP_FILE.into(),
                path: format!("mcpServers.{}", serena::SERVER),
                value: server,
            },
            append: false,
        },
        DesiredEntry {
            entry: SettingsEntry {
                file: settings_file.into(),
                path: "enabledMcpjsonServers".into(),
                value: serde_json::Value::String(serena::SERVER.into()),
            },
            append: true,
        },
        // its tools run without a permission prompt; Forge's hooks still apply their rules to them
        DesiredEntry {
            entry: SettingsEntry {
                file: settings_file.into(),
                path: "permissions.allow".into(),
                value: serde_json::Value::String(format!("mcp__{}", serena::SERVER)),
            },
            append: true,
        },
    ];
    (files, settings)
}

/// A tool's pin in Forge's catalog.
fn forge_tools_pin(tool: &str) -> String {
    forge_tools::Catalog::builtin()
        .get(tool)
        .map(|t| t.version.to_string())
        .unwrap_or_default()
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
    let settings_file = match level {
        Level::User => USER_SETTINGS_FILE,
        Level::Project if input.local => LOCAL_SETTINGS_FILE,
        Level::Project => SETTINGS_FILE,
    };
    let settings_label = display_path(level, input.base, settings_file);
    let selected = select(
        input.set,
        &site,
        level,
        input.config,
        input.roles,
        SettingsTarget {
            file: settings_file,
            label: &settings_label,
        },
        &mut warnings,
    )?;

    let mut settings: Vec<DesiredEntry> = owned_asks(level, input.base)
        .into_iter()
        .map(|v| DesiredEntry {
            entry: SettingsEntry {
                file: settings_file.into(),
                path: "permissions.ask".into(),
                value: serde_json::Value::String(v),
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
                places: Some(site.pack_places(&p.id)),
            })
            .collect();
        files = render_rules_files(&contexts, input.facts, &activation);
        let languages = forge_core::serena::languages(
            |pack| activation.packs.iter().any(|p| p.id == pack),
            |glob| input.facts.is_some_and(|f| f.root.has_files(glob)),
            forge_core::serena::typescript_major(input.base),
        );
        // Serena where it has a language to serve; never with --local, which writes no shared
        // file (.mcp.json is one, and Claude Code takes MCP servers from nowhere else)
        if input.local && !languages.is_empty() {
            warnings.push(
                "Serena is not set up with --local: its server goes in .mcp.json, a shared file; run forge apply without --local to set it up".into(),
            );
        } else if !languages.is_empty() {
            let (serena_files, serena_settings) =
                serena_setup(input.base, settings_file, &languages);
            files.extend(serena_files);
            settings.extend(serena_settings);
        }
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
