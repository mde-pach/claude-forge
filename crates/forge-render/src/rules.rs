//! Which rules apply here and what enforces each (§5.3, §6.1, `src/plan/rules.ts` and each
//! target kind's `plan`): the active packs' rules, local rules, forge.yaml's disables,
//! deprecations, version gates and file roles; then, per target, what it adds (settings
//! entries, the checker's work) and what `forge status` says about it.

use std::collections::BTreeMap;

use forge_detect::Site;
use forge_detect::roles::{Roles, apply_roles};
use forge_packs::PackSet;
use forge_packs::model::{
    AstGrepTarget, CheckTarget, Disable, GrepTarget, GuardTarget, Level, ProjectConfig, Rule,
    Target, ToolTarget,
};
use forge_packs::roles::roles_of_rule;
use serde::Serialize;

use crate::settings::{DesiredEntry, SettingsEntry};

/// What a rule's targets do here.
#[derive(Debug, Clone, Serialize)]
pub struct RuleStatus {
    /// The rule as the checker runs it: its file roles replaced by their effective patterns.
    pub rule: Rule,
    /// Where it applies: workspace package directories, or `None` for the whole repository.
    pub dirs: Option<Vec<String>>,
    /// What enforces it here, for the person.
    pub enforced: Vec<String>,
    /// Why a target does not enforce it here.
    pub not_enforced: Vec<String>,
    /// Tool targets whose tool is configured: audited (the project's own config enforces them).
    pub audits: Vec<ToolTarget>,
    /// The ast-grep target the checker runs (standing in for an absent tool).
    pub astgrep: Option<Placed<AstGrepTarget>>,
    pub checks: Vec<Placed<CheckTarget>>,
    pub greps: Vec<Placed<GrepTarget>>,
    pub guards: Vec<GuardTarget>,
}

/// A target and the workspace packages it runs in (`None`: where the rule applies).
#[derive(Debug, Clone, Serialize)]
pub struct Placed<T> {
    pub target: T,
    pub dirs: Option<Vec<String>>,
}

/// A tool diagnostic an active pack switches off, and the rules that asked for it.
#[derive(Debug, Clone, Serialize)]
pub struct SwitchedOff {
    pub pack: String,
    pub tool: String,
    pub expects: String,
    pub reason: String,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Gated {
    pub id: String,
    pub needs: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Deprecation {
    pub id: String,
    pub replaced_by: String,
}

/// The rules of a plan.
#[derive(Debug, Clone, Default)]
pub struct Selected {
    pub active: Vec<RuleStatus>,
    pub disabled: Vec<Disable>,
    pub deprecated: Vec<Deprecation>,
    pub gated: Vec<Gated>,
    pub switched_off: Vec<SwitchedOff>,
    /// Each file role the active rules name, with the rules naming it.
    pub role_use: BTreeMap<String, Vec<String>>,
    /// Settings entries the targets want (permissions, sandbox).
    pub settings: Vec<DesiredEntry>,
}

fn phase(t: &Target) -> u8 {
    match t {
        Target::Tool(_) => 0,
        Target::AstGrep(_) => 1,
        _ => 2,
    }
}

fn in_dirs(dirs: &Option<Vec<String>>) -> String {
    match dirs {
        Some(d) => format!(" (in {})", d.join(", ")),
        None => String::new(),
    }
}

/// Selects and plans the rules. `site` knows the repository; `level`, the settings file and
/// forge.yaml decide the rest.
pub fn select(
    set: &PackSet,
    site: &Site,
    level: Level,
    config: Option<&ProjectConfig>,
    roles: &Roles,
    settings_file: &str,
    warnings: &mut Vec<String>,
) -> Result<Selected, String> {
    let project = level == Level::Project;
    let active_ids: Vec<&str> = site.active.packs.iter().map(|p| p.id.as_str()).collect();
    let (rules, w) = set.select_rules(&active_ids, level);
    warnings.extend(w);
    let mut by_id: indexmap::IndexMap<String, &Rule> =
        rules.into_iter().map(|r| (r.id.clone(), r)).collect();

    let mut out = Selected::default();
    if project {
        for d in config
            .and_then(|c| c.disable.as_ref())
            .into_iter()
            .flatten()
        {
            if by_id.shift_remove(&d.id).is_some() {
                out.disabled.push(d.clone());
            } else {
                warnings.push(format!(
                    "forge.yaml: disable: rule \"{}\" is not active here",
                    d.id
                ));
            }
        }
    }
    by_id.retain(|id, r| match &r.deprecated {
        Some(d) => {
            out.deprecated.push(Deprecation {
                id: id.clone(),
                replaced_by: d.replaced_by.clone(),
            });
            false
        }
        None => true,
    });
    if project {
        by_id.retain(|id, r| {
            if site.gated_out(r) {
                let needs = r
                    .when
                    .as_ref()
                    .map(|w| {
                        w.deps
                            .iter()
                            .map(|d| format!("{} {}", d.name, d.version.as_deref().unwrap_or("")))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                out.gated.push(Gated {
                    id: id.clone(),
                    needs,
                });
                false
            } else {
                true
            }
        });
    }

    for p in &site.active.packs {
        for o in set.packs[&p.id].tool_off.iter().flatten() {
            out.switched_off.push(SwitchedOff {
                pack: p.id.clone(),
                tool: o.tool.clone(),
                expects: o.expects.clone(),
                reason: o.reason.clone(),
                rules: Vec::new(),
            });
        }
    }

    let mut sorted: Vec<&Rule> = by_id.into_values().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for r in sorted {
        for name in roles_of_rule(r) {
            out.role_use.entry(name).or_default().push(r.id.clone());
        }
        let (rule, empty) = apply_roles(r, roles).map_err(|e| format!("{}: {e}", r.file))?;
        let st = plan_rule(rule, &empty, site, level, settings_file, &mut out, warnings)?;
        out.active.push(st);
    }
    Ok(out)
}

fn plan_rule(
    rule: Rule,
    empty_roles: &[String],
    site: &Site,
    level: Level,
    settings_file: &str,
    out: &mut Selected,
    warnings: &mut Vec<String>,
) -> Result<RuleStatus, String> {
    let dirs = site.rule_dirs(&rule);
    let mut st = RuleStatus {
        rule: rule.clone(),
        dirs: dirs.clone(),
        enforced: Vec::new(),
        not_enforced: Vec::new(),
        audits: Vec::new(),
        astgrep: None,
        checks: Vec::new(),
        greps: Vec::new(),
        guards: Vec::new(),
    };
    if !empty_roles.is_empty() {
        let names: Vec<String> = empty_roles.iter().map(|e| format!("@{e}")).collect();
        st.not_enforced.push(format!(
            "not enforced here: file role {} has no files (set them in forge.yaml: files: {{{}: {{set: [...]}}}})",
            names.join(", "),
            empty_roles[0]
        ));
        return Ok(st);
    }
    if rule
        .enforce
        .iter()
        .filter(|t| matches!(t, Target::AstGrep(_)))
        .count()
        > 1
    {
        return Err(format!(
            "{}: at most one ast-grep target per rule",
            rule.file
        ));
    }
    let mut targets: Vec<&Target> = rule.enforce.iter().collect();
    targets.sort_by_key(|t| phase(t));
    let tools: Vec<&ToolTarget> = rule
        .enforce
        .iter()
        .filter_map(|t| match t {
            Target::Tool(x) => Some(x),
            _ => None,
        })
        .collect();
    let switched = |out: &Selected, t: &ToolTarget| {
        out.switched_off
            .iter()
            .position(|o| o.tool == t.tool.as_str() && o.expects == t.expects)
    };
    for t in targets {
        match t {
            Target::Tool(t) => {
                if let Some(i) = switched(out, t) {
                    out.switched_off[i].rules.push(rule.id.clone());
                    let o = &out.switched_off[i];
                    st.not_enforced.push(format!(
                        "not enforced here: {} {} switched off by {} ({})",
                        t.tool, t.expects, o.pack, o.reason
                    ));
                } else if site.tool_present(t.tool) {
                    st.audits.push(t.clone());
                    st.enforced
                        .push(format!("{} ({}), audited", t.tool, t.expects));
                } else {
                    st.not_enforced
                        .push(format!("not enforced here: {} not configured", t.tool));
                }
            }
            Target::AstGrep(a) => {
                let tool_covers = tools.iter().any(|t| switched(out, t).is_some())
                    || (!tools.is_empty() && tools.iter().any(|t| site.tool_present(t.tool)));
                if tool_covers {
                    continue;
                }
                if level == Level::User {
                    warnings.push(format!(
                        "{}: ast-grep targets are project-level only",
                        rule.id
                    ));
                    continue;
                }
                let missing = site.missing_needs(&rule, a);
                if !missing.is_empty() {
                    st.not_enforced.push(format!(
                        "not enforced here: the types engine needs {} installed to see the types this rule checks (add it as a development dependency)",
                        missing.join(", ")
                    ));
                    continue;
                }
                let types = a.where_.as_ref().map(|_| {
                    if rule.scope.language == forge_packs::model::Language::Python {
                        " with types (ty)"
                    } else {
                        " with types (the project's TypeScript)"
                    }
                });
                st.enforced.push(format!(
                    "ast-grep {}{}{}",
                    a.mode,
                    types.unwrap_or(""),
                    in_dirs(&dirs)
                ));
                st.astgrep = Some(Placed {
                    target: a.clone(),
                    dirs: dirs.clone(),
                });
                st.not_enforced.clear();
            }
            Target::Permission(p) => {
                out.settings.push(DesiredEntry {
                    entry: SettingsEntry {
                        file: settings_file.to_string(),
                        path: format!("permissions.{}", p.decision),
                        value: serde_json::Value::String(p.rule.clone()),
                    },
                    append: true,
                });
                st.enforced.push(format!(
                    "permission {} {} ({settings_file})",
                    p.decision, p.rule
                ));
            }
            Target::Guard(g) => {
                st.guards.push(g.clone());
                if !st.enforced.iter().any(|e| e.starts_with("guard")) {
                    st.enforced
                        .push(format!("guard {} (PreToolUse Bash)", g.decision));
                }
            }
            Target::Sandbox(s) => {
                if level != Level::User {
                    warnings.push(format!("{}: sandbox targets are user-level only", rule.id));
                    continue;
                }
                let path = format!("sandbox.{}", s.path);
                match &s.value {
                    serde_json::Value::Array(items) => {
                        for v in items {
                            out.settings.push(DesiredEntry {
                                entry: SettingsEntry {
                                    file: settings_file.to_string(),
                                    path: path.clone(),
                                    value: v.clone(),
                                },
                                append: true,
                            });
                        }
                    }
                    v => out.settings.push(DesiredEntry {
                        entry: SettingsEntry {
                            file: settings_file.to_string(),
                            path: path.clone(),
                            value: v.clone(),
                        },
                        append: false,
                    }),
                }
                st.enforced
                    .push(format!("sandbox {} ({settings_file})", s.path));
            }
            Target::Check(c) => {
                if level == Level::User {
                    warnings.push(format!("{}: check targets are project-level only", rule.id));
                    continue;
                }
                let place = site.check_place(&rule, c);
                if !place.absent.is_empty() {
                    let names: Vec<&str> = place.absent.iter().map(|a| a.as_str()).collect();
                    st.not_enforced.push(format!(
                        "not enforced here: {} not configured",
                        names.join(", ")
                    ));
                } else {
                    st.enforced.push(format!(
                        "check {}: {}{}",
                        c.stage,
                        c.run,
                        in_dirs(&place.dirs)
                    ));
                    st.checks.push(Placed {
                        target: c.clone(),
                        dirs: place.dirs,
                    });
                }
            }
            Target::Grep(g) => {
                if level == Level::User {
                    warnings.push(format!("{}: grep targets are project-level only", rule.id));
                    continue;
                }
                st.enforced
                    .push(format!("grep {}{}", g.mode, in_dirs(&dirs)));
                st.greps.push(Placed {
                    target: g.clone(),
                    dirs: dirs.clone(),
                });
            }
        }
    }
    Ok(st)
}
