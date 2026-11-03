//! Which packs are active (§5.1) and why.

use std::collections::BTreeMap;

use forge_packs::PackSet;
use forge_packs::model::{Level, Pack, ProjectConfig};
use serde::Serialize;

use crate::facts::{DepFacts, ProjectFacts};

/// An active (or removed) pack and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackStatus {
    pub id: String,
    pub why: String,
}

/// Whether the pack is detected in these facts (any one of its `detect` facts).
pub fn detect_pack(pack: &Pack, f: &DepFacts) -> bool {
    let Some(d) = &pack.detect else { return false };
    d.always == Some(true)
        || d.files_any.iter().flatten().any(|g| f.has_files(g))
        || d.deps_any.iter().flatten().any(|dep| f.dep_matches(dep))
        || d.library_any
            .iter()
            .flatten()
            .any(|e| f.library.contains(e))
        || d.content_any
            .iter()
            .flatten()
            .any(|c| f.contains(&c.files, &c.pattern))
        || d.yaml_keys.as_ref().is_some_and(|k| f.yaml_keys(k))
        || d.sources_any.as_ref().is_some_and(|g| f.sources(g))
        || d.facts_any.iter().flatten().any(|(name, want)| {
            f.fact(name)
                .is_some_and(|v| want.as_slice().iter().any(|w| w == v))
        })
}

/// Detected at the root or in a workspace package (each judged on its own facts).
pub fn detect_anywhere(pack: &Pack, facts: &ProjectFacts) -> bool {
    detect_pack(pack, &facts.root) || facts.workspaces.iter().any(|w| detect_pack(pack, w))
}

/// The active packs, sorted by id, and the ones forge.yaml's `remove` turned off (with their
/// subtree and the packs requiring them).
#[derive(Debug, Clone, Default)]
pub struct Activation {
    pub packs: Vec<PackStatus>,
    pub removed: Vec<PackStatus>,
    pub warnings: Vec<String>,
}

impl Activation {
    pub fn is_active(&self, id: &str) -> bool {
        self.packs.iter().any(|p| p.id == id)
    }

    pub fn why(&self, id: &str) -> Option<&str> {
        self.packs
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.why.as_str())
    }
}

pub fn active_packs(
    set: &PackSet,
    level: Level,
    config: Option<&ProjectConfig>,
    facts: Option<&ProjectFacts>,
) -> Activation {
    let mut out = Activation::default();
    if level == Level::User {
        out.packs = set
            .packs
            .values()
            .filter(|p| p.level == Level::User)
            .map(|p| PackStatus {
                id: p.id.clone(),
                why: "user level".into(),
            })
            .collect();
        out.packs.sort_by(|a, b| a.id.cmp(&b.id));
        return out;
    }
    let mut why: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();
    let activate = |why: &mut indexmap::IndexMap<String, String>, id: &str, reason: String| {
        if why.contains_key(id) {
            return false;
        }
        why.insert(id.to_string(), reason);
        true
    };
    let changes = config.and_then(|c| c.packs.as_ref());
    for id in changes.and_then(|c| c.add.as_ref()).into_iter().flatten() {
        match set.packs.get(id) {
            None => out
                .warnings
                .push(format!("forge.yaml: packs.add: unknown pack \"{id}\"")),
            Some(p) if p.level != Level::Project => out.warnings.push(format!(
                "forge.yaml: packs.add: \"{id}\" is a user-level pack (use forge apply --user)"
            )),
            Some(_) => {
                let line = set.lineage(id);
                for a in &line[..line.len() - 1] {
                    activate(&mut why, a, format!("parent of {id}"));
                }
                activate(&mut why, id, "forge.yaml".into());
            }
        }
    }
    for id in changes
        .and_then(|c| c.remove.as_ref())
        .into_iter()
        .flatten()
    {
        if !set.packs.contains_key(id) {
            out.warnings
                .push(format!("forge.yaml: packs.remove: unknown pack \"{id}\""));
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for p in set.packs.values() {
            if p.level != Level::Project || why.contains_key(&p.id) {
                continue;
            }
            let parent_ok = p
                .parent
                .as_ref()
                .is_none_or(|parent| why.contains_key(parent));
            if parent_ok && facts.is_some_and(|f| detect_anywhere(p, f)) {
                changed = activate(&mut why, &p.id, "detected".into()) || changed;
            }
        }
        let ids: Vec<String> = why.keys().cloned().collect();
        for id in ids {
            let reqs = set.packs[&id].requires.clone();
            for req in reqs {
                for a in set.lineage(&req) {
                    changed = activate(&mut why, a, format!("required by {id}")) || changed;
                }
            }
        }
    }
    let mut removed: BTreeMap<String, String> = BTreeMap::new();
    for id in changes
        .and_then(|c| c.remove.as_ref())
        .into_iter()
        .flatten()
    {
        if why.contains_key(id) {
            removed.insert(id.clone(), "removed in forge.yaml".into());
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for id in why.keys() {
            if removed.contains_key(id) {
                continue;
            }
            let p = &set.packs[id];
            let req = p.requires.iter().find(|r| removed.contains_key(*r));
            let reason = match (&p.parent, req) {
                (Some(parent), _) if removed.contains_key(parent) => {
                    Some(format!("parent {parent} is not active"))
                }
                (_, Some(r)) => Some(format!("requires removed pack {r}")),
                _ => None,
            };
            if let Some(r) = reason {
                removed.insert(id.clone(), r);
                changed = true;
            }
        }
    }
    let mut packs: Vec<PackStatus> = why
        .into_iter()
        .filter(|(id, _)| !removed.contains_key(id))
        .map(|(id, why)| PackStatus { id, why })
        .collect();
    packs.sort_by(|a, b| a.id.cmp(&b.id));
    out.packs = packs;
    out.removed = removed
        .into_iter()
        .map(|(id, why)| PackStatus { id, why })
        .collect();
    out
}
