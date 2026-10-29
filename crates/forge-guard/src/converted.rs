//! The structural form of every current pack guard rule (`converted.yaml`), until the packs
//! carry it themselves: by rule id, the targets that replace the rule's guard targets, or why
//! the rule is dropped.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::rule::GuardSpec;
use crate::set::GuardRule;

pub const SOURCE: &str = include_str!("converted.yaml");

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Converted {
    #[serde(default)]
    pub targets: Vec<GuardSpec>,
    /// The rule is no guard rule any more, and why.
    #[serde(default)]
    pub dropped: Option<String>,
    /// A description in place of the rule's (wording fixes the agent reads).
    #[serde(default)]
    pub description: Option<String>,
}

static CONVERTED: LazyLock<Result<BTreeMap<String, Converted>, String>> = LazyLock::new(|| {
    let parsed = forge_packs::yaml::parse_strict(SOURCE).map_err(|e| e.to_string())?;
    serde_json::from_value(parsed.value).map_err(|e| e.to_string())
});

/// Every converted rule, by id.
pub fn all() -> Result<&'static BTreeMap<String, Converted>, String> {
    CONVERTED.as_ref().map_err(Clone::clone)
}

/// The rule with its converted targets; None when it has no guard targets or is dropped.
/// A rule with no conversion keeps its pack targets.
pub fn rule(pack_rule: &forge_packs::Rule) -> Option<GuardRule> {
    let mut r = GuardRule::from_pack(pack_rule)?;
    if let Some(c) = all().ok().and_then(|m| m.get(&r.id)) {
        if c.dropped.is_some() {
            return None;
        }
        r.targets = c.targets.clone();
        if let Some(d) = &c.description {
            r.description = d.clone();
        }
    }
    Some(r)
}
