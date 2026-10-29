//! The rules files the agent reads: `.claude/rules/forge-<pack id, "/" as "-">.md`, one per
//! active pack with context. Their text depends only on the repository's content (they are
//! committed and shared), never on machine state.
//!
//! A pack's context is a list of items `{id?, text, when?}`. Until the loader reads
//! conditioned items from `pack.yaml`, a pack's `context.md` is one item with no condition.
//! An item is rendered when its `when` holds; an item whose `id` an earlier pack (in id
//! order) rendered is left out. A pack detected in only some workspace packages gets
//! `paths:` frontmatter, so Claude Code loads the file for those packages' files only.

use std::collections::{BTreeMap, BTreeSet};

use forge_detect::{Activation, DepFacts, ProjectFacts};
use forge_packs::glob::glob_to_regex;
use forge_packs::model::{DepSpec, OneOrMany, Pack};
use serde::{Deserialize, Serialize};

/// First lines of every rules file: the facts guide defaults, and an explicit instruction wins.
pub const CONTEXT_INTRO: &str = "These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.\n\n";

/// A piece of a pack's context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextItem {
    /// Items with the same id are rendered once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ContextWhen>,
}

/// When an item holds. Every condition given must hold, in one place the pack applies to (the
/// root or a workspace package); `packs` is about the whole repository.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWhen {
    /// Every dependency is declared, in its version range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<DepSpec>,
    /// A file matches one of the globs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Every named fact has the value (or one of the values): `python.pm: uv`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub facts: BTreeMap<String, OneOrMany<String>>,
    /// Every pack is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<String>,
}

impl ContextWhen {
    /// Whether it holds in one of `places`, with `active` the active packs.
    pub fn holds(&self, places: &[&DepFacts], active: &Activation) -> bool {
        if !self.packs.iter().all(|p| active.is_active(p)) {
            return false;
        }
        places.iter().any(|f| self.holds_in(f))
    }

    fn holds_in(&self, f: &DepFacts) -> bool {
        self.deps.iter().all(|d| f.dep_matches(d))
            && (self.files.is_empty()
                || self.files.iter().any(|g| {
                    let re = glob_to_regex(g);
                    f.files.iter().any(|x| re.is_match(x).unwrap_or(false))
                }))
            && self.facts.iter().all(|(name, want)| {
                f.fact(name)
                    .is_some_and(|v| want.as_slice().iter().any(|w| w == v))
            })
    }
}

/// A pack's context items: today its context.md, as one unconditioned item.
pub fn pack_context(pack: &Pack) -> Vec<ContextItem> {
    pack.context
        .iter()
        .map(|text| ContextItem {
            id: None,
            text: text.clone(),
            when: None,
        })
        .collect()
}

/// The rules file name of a pack.
pub fn rules_file_name(pack_id: &str) -> String {
    format!("forge-{}.md", pack_id.replace('/', "-"))
}

/// The rules file path of a pack, relative to the repository.
pub fn rules_file_path(pack_id: &str) -> String {
    format!(".claude/rules/{}", rules_file_name(pack_id))
}

/// A pack to render: its items and where it applies (`None`: the whole repository).
pub struct PackContext<'a> {
    pub id: &'a str,
    pub items: Vec<ContextItem>,
    pub dirs: Option<Vec<String>>,
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
        let places: Vec<&DepFacts> = match (facts, &p.dirs) {
            (None, _) => Vec::new(),
            (Some(f), Some(dirs)) => f
                .workspaces
                .iter()
                .filter(|w| dirs.contains(&w.dir))
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
                Some(w) => w.holds(&places, active),
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
