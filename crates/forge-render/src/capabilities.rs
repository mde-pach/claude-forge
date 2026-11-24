//! The capabilities active in a repository (§20.2, gate 1): their pack is active, their `when`
//! holds on the repository's facts, and `forge.yaml` does not disable them. Whether one is
//! available on this machine (gate 2) and relevant to a prompt (gate 3) is decided elsewhere.

use forge_detect::Site;
use forge_packs::PackSet;
use forge_packs::model::{Capability, Disable, Level, ProjectConfig};
use serde::Serialize;

/// A capability a repository's packs have that is not active here, and why.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityOff {
    pub id: String,
    pub why: String,
}

#[derive(Debug, Clone, Default)]
pub struct SelectedCapabilities {
    /// Sorted by id.
    pub active: Vec<Capability>,
    pub disabled: Vec<Disable>,
    pub inactive: Vec<CapabilityOff>,
}

/// The capabilities of the active packs and the local ones (project level only).
pub fn select_capabilities(
    set: &PackSet,
    site: &Site,
    level: Level,
    config: Option<&ProjectConfig>,
    warnings: &mut Vec<String>,
) -> SelectedCapabilities {
    let mut out = SelectedCapabilities::default();
    if level != Level::Project {
        return out;
    }
    let active_ids: Vec<&str> = site.active.packs.iter().map(|p| p.id.as_str()).collect();
    let (caps, w) = set.select_capabilities(&active_ids, level);
    warnings.extend(w);
    let disables: Vec<&Disable> = config
        .and_then(|c| c.disable.as_ref())
        .into_iter()
        .flatten()
        .collect();
    for c in caps {
        if let Some(d) = disables.iter().find(|d| d.id == c.id) {
            out.disabled.push((*d).clone());
        } else if site.holds(&c.pack, c.when.as_ref()) {
            out.active.push(c.clone());
        } else {
            out.inactive.push(CapabilityOff {
                id: c.id.clone(),
                why: "its when does not hold in this repository".into(),
            });
        }
    }
    out
}

/// Whether `id` names a capability of `set` (active or not): a `disable` naming one is not
/// about a rule.
pub fn is_capability(set: &PackSet, id: &str) -> bool {
    set.capability(id).is_some()
}
