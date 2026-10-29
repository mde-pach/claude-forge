//! The rules files the agent reads: `.claude/rules/forge-<pack id, "/" as "-">.md`, one per
//! active pack with context. Their text depends only on the repository's content (they are
//! committed and shared), never on machine state.
//!
//! A pack's context is its pack.yaml's list of items `{id?, text, when?}`. An item is
//! rendered when its `when` holds; an item whose `id` an earlier pack (in id order) rendered
//! is left out. A pack detected in only some workspace packages gets
//! `paths:` frontmatter, so Claude Code loads the file for those packages' files only.

use std::collections::{BTreeMap, BTreeSet};

use forge_detect::{Activation, DepFacts, ProjectFacts};
use forge_packs::glob::glob_to_regex;
use forge_packs::model::Pack;
pub use forge_packs::model::{ContextItem, ContextWhen};

/// First lines of every rules file: the facts guide defaults, and an explicit instruction wins.
pub const CONTEXT_INTRO: &str = "These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.\n\n";

/// Whether an item's `when` holds in one of `places`, with `active` the active packs: one of
/// its packs is active, and in one place every other condition holds.
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
                f.files.iter().any(|x| re.is_match(x).unwrap_or(false))
            }))
        && w.facts.iter().all(|(name, want)| {
            f.fact(name)
                .is_some_and(|v| want.as_slice().iter().any(|w| w == v))
        })
}

/// A pack's context items (its pack.yaml's `context`).
pub fn pack_context(pack: &Pack) -> Vec<ContextItem> {
    pack.context.clone()
}

/// The rules file name of a pack.
pub fn rules_file_name(pack_id: &str) -> String {
    format!("forge-{}.md", pack_id.replace('/', "-"))
}

/// The rules file path of a pack, relative to the repository.
pub fn rules_file_path(pack_id: &str) -> String {
    format!(".claude/rules/{}", rules_file_name(pack_id))
}

/// A pack to render: its items, where its rules file applies (`dirs`, `None`: the whole
/// repository), and the places its items' conditions are judged in (`places`: "" for the
/// root, else workspace package directories; `None`: the root and every package).
pub struct PackContext<'a> {
    pub id: &'a str,
    pub items: Vec<ContextItem>,
    pub dirs: Option<Vec<String>>,
    pub places: Option<Vec<String>>,
}

/// The rules files of the active packs, in id order: path → text.
pub fn render_rules_files(
    packs: &[PackContext],
    facts: Option<&ProjectFacts>,
    active: &Activation,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for p in packs {
        let places: Vec<&DepFacts> = match (facts, &p.places) {
            (None, _) => Vec::new(),
            (Some(f), Some(places)) => std::iter::once(&f.root)
                .chain(f.workspaces.iter())
                .filter(|w| places.contains(&w.dir))
                .collect(),
            (Some(f), None) => std::iter::once(&f.root)
                .chain(f.workspaces.iter())
                .collect(),
        };
        let mut body = String::new();
        for item in &p.items {
            if item.id.as_ref().is_some_and(|id| seen.contains(id)) {
                continue;
            }
            let holds = match &item.when {
                None => true,
                Some(w) => when_holds(w, &places, active),
            };
            if !holds {
                continue;
            }
            if let Some(id) = &item.id {
                seen.insert(id.clone());
            }
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(&item.text);
        }
        if body.trim().is_empty() {
            continue;
        }
        let mut text = String::new();
        if let Some(dirs) = p.dirs.as_ref().filter(|d| !d.is_empty()) {
            text.push_str("---\npaths:\n");
            for d in dirs {
                text.push_str(&format!("  - \"{d}/**\"\n"));
            }
            text.push_str("---\n\n");
        }
        text.push_str(CONTEXT_INTRO);
        text.push_str(&body);
        out.insert(rules_file_path(p.id), text);
    }
    out
}
