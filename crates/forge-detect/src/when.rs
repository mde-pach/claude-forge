//! When a context item, or a capability, holds (§5.4): its `when` judged on repository facts
//! alone, in a place its pack applies to.

use forge_packs::glob::glob_to_regex;
use forge_packs::model::ContextWhen;

use crate::{Activation, DepFacts, Site};

/// Whether a `when` holds in one of `places`, with `active` the active packs: one of its packs
/// is active, and in one place every other condition holds.
pub fn when_holds(w: &ContextWhen, places: &[&DepFacts], active: &Activation) -> bool {
    if !w.packs.is_empty() && !w.packs.iter().any(|p| active.is_active(p)) {
        return false;
    }
    if places.is_empty() {
        // no project facts (user level): only a condition on packs can hold
        return w.deps.is_empty() && w.files.is_empty() && w.facts.is_empty();
    }
    places.iter().any(|f| holds_in(w, f))
}

fn holds_in(w: &ContextWhen, f: &DepFacts) -> bool {
    w.deps.iter().all(|d| f.dep_matches(d))
        && (w.files.is_empty()
            || w.files.iter().any(|g| {
                let re = glob_to_regex(g);
                f.files
                    .iter()
                    .any(|x| crate::facts::reaches(g, x) && re.is_match(x).unwrap_or(false))
            }))
        && w.facts.iter().all(|(name, want)| {
            f.fact(name)
                .is_some_and(|v| want.as_slice().iter().any(|w| w == v))
        })
}

impl Site<'_> {
    /// The facts of the places `pack` applies to: the root and the packages it was detected in
    /// (a pack detected nowhere, or no pack at all, is judged at the root alone).
    pub fn places_of(&self, pack: &str) -> Vec<&DepFacts> {
        let Some(facts) = self.facts else {
            return Vec::new();
        };
        let dirs = self.pack_places(pack);
        std::iter::once(&facts.root)
            .chain(facts.workspaces.iter())
            .filter(|w| dirs.contains(&w.dir))
            .collect()
    }

    /// Whether `when` (of an item of `pack`, or none) holds here.
    pub fn holds(&self, pack: &str, when: Option<&ContextWhen>) -> bool {
        when.is_none_or(|w| when_holds(w, &self.places_of(pack), self.active))
    }
}
