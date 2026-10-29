//! Which rules a repository's check runs: exactly the ones `forge apply` selected. The project
//! facts, activation and placement are forge-detect's; the rule selection (forge.yaml's
//! `disable`, deprecations, `when` gates, file roles, what enforces each target) is
//! forge-render's plan. This module only turns the plan's rules into the checker's.

use std::collections::BTreeMap;
use std::path::Path;

use forge_detect::roles::{RoleInputs, resolve_roles};
use forge_packs::PackSet;
use forge_packs::model::{Level, ProjectConfig, Target};
use forge_render::plan::{Plan, PlanInput, plan};
use forge_render::rules::RuleStatus;

use crate::checker::ActiveRule;
use crate::scope::Roles;

/// What the check of a repository runs, and the plan it comes from.
pub struct Selection {
    pub rules: Vec<ActiveRule>,
    /// Every file role's effective patterns (the tools' configs and forge.yaml applied).
    pub roles: Roles,
    pub plan: Plan,
}

/// Plans the repository at `root` as `forge apply` does, and selects its rules.
pub fn project(
    root: &Path,
    set: &PackSet,
    config: Option<&ProjectConfig>,
) -> Result<Selection, String> {
    let files = forge_detect::list_repo_files(root);
    let facts = forge_detect::read_project_facts(root, &files).map_err(|e| e.to_string())?;
    let inputs = RoleInputs {
        repo: root,
        files: &files,
    };
    let roles = resolve_roles(Some(&inputs), config.and_then(|c| c.files.as_ref()))
        .map_err(|e| e.to_string())?;
    let p = plan(&PlanInput {
        level: Level::Project,
        base: root,
        set,
        local: false,
        config,
        facts: Some(&facts),
        roles: &roles,
    })?;
    let roles = Roles::from_map(
        roles
            .iter()
            .map(|(name, r)| (name.clone(), r.globs()))
            .collect::<BTreeMap<_, _>>(),
    );
    Ok(Selection {
        rules: rules_of(&p),
        roles,
        plan: p,
    })
}

/// The rules the check runs in `root` (see [`project`]).
pub fn active_rules(
    root: &Path,
    set: &PackSet,
    config: Option<&ProjectConfig>,
) -> Result<Vec<ActiveRule>, String> {
    project(root, set, config).map(|s| s.rules)
}

/// The checker's rules of a plan: each active rule with the targets that enforce it here (an
/// ast-grep target a configured tool covers, a check whose tool is configured nowhere, are
/// left out), where it applies, and where each of its checks runs.
pub fn rules_of(plan: &Plan) -> Vec<ActiveRule> {
    plan.active.iter().map(active_rule).collect()
}

fn active_rule(st: &RuleStatus) -> ActiveRule {
    let mut rule = st.rule.clone();
    let mut check_dirs = BTreeMap::new();
    let mut enforce = Vec::new();
    for t in std::mem::take(&mut rule.enforce) {
        let keep = match &t {
            Target::AstGrep(_) => st.astgrep.is_some(),
            Target::Grep(_) => !st.greps.is_empty(),
            Target::Check(c) => match st.checks.iter().find(|p| p.target == *c) {
                Some(p) => {
                    check_dirs.insert(enforce.len(), p.dirs.clone());
                    true
                }
                None => false,
            },
            _ => true,
        };
        if keep {
            enforce.push(t);
        }
    }
    rule.enforce = enforce;
    ActiveRule {
        rule,
        dirs: st.dirs.clone(),
        check_dirs,
    }
}
