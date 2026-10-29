//! Claude Forge's pack loader.
//!
//! Packs stay YAML, the format of record. Three inputs, one loader:
//!
//! - the built-in packs: the repository's `packs/`, embedded in the binary at compile time
//!   and parsed at run time;
//! - local packs: `<repo>/.forge/packs/<id>/`, laid out as built-in packs are;
//! - local rules: `<repo>/.forge/rules/*.yaml`, standalone rules.
//!
//! A pack's id is its directory path; its directory holds `pack.yaml`, `rules/<name>.yaml`
//! (the rule id is `<pack id>/<name>`) and an optional `init/`. `rules` and
//! `init` are reserved; any other sub-directory is a child pack (with a pack.yaml, at its
//! parent's level) or empty. A user-level pack has no `detect`.
//!
//! Local packs add to the built-in ones: a local pack id may not be a built-in pack's id
//! (an error), and a local pack may require built-in packs. Local rules keep their
//! semantics: one with the id of an active pack's rule replaces it
//! ([`PackSet::select_rules`]).
//!
//! Loading parses, validates (`schemas/*.schema.json`, then each target kind's checks) and
//! lints ([`lint`]); a cache directory, when given, keeps the parsed trees ([`cache`]).

pub mod cache;
mod error;
pub mod glob;
pub mod lint;
pub mod loader;
pub mod model;
pub mod roles;
pub mod schema;
pub mod tree;
mod validate;
pub mod yaml;

use std::collections::HashMap;
use std::path::Path;

use include_dir::{Dir, include_dir};
use indexmap::IndexMap;
use serde_json::Value;

pub use crate::cache::CacheOutcome;
pub use crate::error::{ErrorKind, LoadError};
pub use crate::lint::{Finding, LintKind};
pub use crate::loader::Loaded;
pub use crate::model::{Level, Origin, Pack, PackSource, ProjectConfig, Rule};

use crate::loader::Place;
use crate::tree::{Disk, Embedded, Tree};

/// The built-in packs, as the repository's `packs/` directory was at build time.
pub static BUILTIN_PACKS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../packs");

/// A hash of this crate's sources, the shipped schemas and the built-in packs.
pub const BUILD_ID: &str = env!("FORGE_PACKS_BUILD_ID");

/// Local packs and rules, relative to the repository.
pub const LOCAL_PACKS_DIR: &str = ".forge/packs";
pub const LOCAL_RULES_DIR: &str = ".forge/rules";

#[derive(Debug, Clone, Copy, Default)]
pub struct Options<'a> {
    /// Where parsed trees are cached; None: no cache.
    pub cache_dir: Option<&'a Path>,
}

/// How each tree was loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheReport {
    pub builtin: CacheOutcome,
    /// None when no repository was given.
    pub local: Option<CacheOutcome>,
}

/// The loaded packs: built-in ones first, then local ones, each parent before its children.
#[derive(Debug, Clone, PartialEq)]
pub struct PackSet {
    pub packs: IndexMap<String, Pack>,
    /// `.forge/rules/*.yaml`, by file name.
    pub local_rules: Vec<Rule>,
    pub findings: Vec<Finding>,
    pub cache: CacheReport,
}

/// A built-in file (an `init/` file, a pack's data), by its path below `packs/`.
pub fn builtin_file(path: &str) -> Option<&'static [u8]> {
    BUILTIN_PACKS.get_file(path).map(|f| f.contents())
}

const BUILTIN_PLACE_SHOWN: &str = "packs/";

fn load_builtin_tree(opts: Options) -> Result<(Loaded, CacheOutcome), LoadError> {
    // the build id covers the embedded packs: no need to hash them at run time
    let key = cache::key([("builtin".to_string(), std::borrow::Cow::Borrowed(&b""[..]))]);
    cache::cached(opts.cache_dir, "builtin-packs", &key, || {
        let tree = Embedded::new(&BUILTIN_PACKS);
        loader::load_packs(&Place {
            tree: &tree,
            prefix: "",
            shown: BUILTIN_PLACE_SHOWN,
            source: PackSource::Builtin,
        })
    })
}

fn load_local_tree(repo: &Path, opts: Options) -> Result<(Loaded, CacheOutcome), LoadError> {
    let packs = Disk(repo.join(LOCAL_PACKS_DIR));
    let rules = Disk(repo.join(LOCAL_RULES_DIR));
    let packs_prefix = format!("{LOCAL_PACKS_DIR}/");
    let rules_prefix = format!("{LOCAL_RULES_DIR}/");
    let packs_place = Place {
        tree: &packs,
        prefix: &packs_prefix,
        shown: &packs_prefix,
        source: PackSource::Local,
    };
    let rules_place = Place {
        tree: &rules,
        prefix: &rules_prefix,
        shown: &rules_prefix,
        source: PackSource::Local,
    };
    let mut inputs = Vec::new();
    for (place, tree) in [
        (&packs_place, &packs as &dyn Tree),
        (&rules_place, &rules as &dyn Tree),
    ] {
        for f in tree.all_files() {
            let bytes = tree.read(&f).map_err(|e| LoadError {
                file: format!("{}{f}", place.shown),
                kind: ErrorKind::Io,
                message: e,
            })?;
            inputs.push((
                format!("{}{f}", place.prefix),
                bytes.unwrap_or_default().into_owned().into(),
            ));
        }
    }
    let key = cache::key(inputs);
    cache::cached(opts.cache_dir, "local-packs", &key, || {
        let mut loaded = loader::load_packs(&packs_place)?;
        let rules = loader::load_rules_dir(&rules_place)?;
        loaded.rules = rules.rules;
        loaded.findings.extend(rules.findings);
        Ok(loaded)
    })
}

/// Every pack's `requires`, and its context items' `when.packs`, name loaded packs.
fn check_requires(
    packs: &IndexMap<String, Pack>,
    shown: impl Fn(&Pack) -> String,
) -> Result<(), LoadError> {
    for p in packs.values() {
        if let Some(req) = p.requires.iter().find(|r| !packs.contains_key(*r)) {
            return Err(LoadError {
                file: shown(p),
                kind: ErrorKind::Layout,
                message: format!("pack {}: requires unknown pack {req}", p.id),
            });
        }
        let named = p
            .context
            .iter()
            .filter_map(|c| c.when.as_ref())
            .flat_map(|w| &w.packs);
        if let Some(unknown) = named.into_iter().find(|n| !packs.contains_key(*n)) {
            return Err(LoadError {
                file: shown(p),
                kind: ErrorKind::Layout,
                message: format!(
                    "pack {}: a context item's when names unknown pack {unknown}",
                    p.id
                ),
            });
        }
    }
    Ok(())
}

fn pack_file_shown(p: &Pack) -> String {
    match p.source {
        PackSource::Builtin => format!("{BUILTIN_PLACE_SHOWN}{}/pack.yaml", p.dir),
        PackSource::Local => format!("{}/pack.yaml", p.dir),
    }
}

/// The built-in packs.
pub fn load_builtin(opts: Options) -> Result<PackSet, LoadError> {
    load(None, opts)
}

/// The built-in packs, then the repository's local packs and rules (when `repo` is given).
pub fn load(repo: Option<&Path>, opts: Options) -> Result<PackSet, LoadError> {
    let (builtin, builtin_outcome) = load_builtin_tree(opts)?;
    check_requires(&builtin.packs, pack_file_shown)?;
    let mut set = PackSet {
        packs: builtin.packs,
        local_rules: Vec::new(),
        findings: builtin.findings,
        cache: CacheReport {
            builtin: builtin_outcome,
            local: None,
        },
    };
    let Some(repo) = repo else { return Ok(set) };
    let (local, local_outcome) = load_local_tree(repo, opts)?;
    for (id, pack) in local.packs {
        if set.packs.contains_key(&id) {
            return Err(LoadError {
                file: pack_file_shown(&pack),
                kind: ErrorKind::Layout,
                message: format!(
                    "pack id \"{id}\" is a built-in pack's; a local pack needs an id of its own"
                ),
            });
        }
        set.packs.insert(id, pack);
    }
    check_requires(&set.packs, pack_file_shown)?;
    set.local_rules = local.rules;
    set.findings.extend(local.findings);
    set.cache.local = Some(local_outcome);
    Ok(set)
}

/// `loadProjectConfig`: the repository's forge.yaml, if it has one.
pub fn load_project_config(repo: &Path) -> Result<Option<ProjectConfig>, LoadError> {
    let file = repo.join("forge.yaml");
    let err = |kind, message: String| LoadError {
        file: "forge.yaml".into(),
        kind,
        message,
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(err(ErrorKind::Io, e.to_string())),
    };
    let parsed = yaml::parse_strict(&text).map_err(|e| err(ErrorKind::Yaml, e.to_string()))?;
    schema::validate(schema::Kind::Project, &parsed.value)
        .map_err(|e| err(ErrorKind::Schema, e))?;
    serde_json::from_value(parsed.value)
        .map(Some)
        .map_err(|e| err(ErrorKind::Schema, format!("invalid project: {e}")))
}

impl PackSet {
    /// The packs from `source`, as JSON (an array, in load order): `forge packs dump`.
    pub fn bundle(&self, source: Option<PackSource>) -> Value {
        Value::Array(
            self.packs
                .values()
                .filter(|p| source.is_none_or(|s| p.source == s))
                .map(|p| serde_json::to_value(p).expect("packs serialize"))
                .collect(),
        )
    }

    /// The pack and its ancestors, root first.
    pub fn lineage(&self, id: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut at = self.packs.get(id);
        while let Some(p) = at {
            out.insert(0, p.id.as_str());
            at = p
                .parent
                .as_deref()
                .and_then(|parent| self.packs.get(parent));
        }
        out
    }

    /// The rules of the active packs (in their order), with local rules applied at project
    /// level: a local rule replaces the pack rule with its id, and is kept when its pack is
    /// active or is no pack at all; one for a known pack that is not active is ignored, with
    /// a warning. Sorted by id (before disables).
    pub fn select_rules<'a>(
        &'a self,
        active: &[&str],
        level: Level,
    ) -> (Vec<&'a Rule>, Vec<String>) {
        let mut by_id: IndexMap<&str, &Rule> = IndexMap::new();
        for id in active {
            for r in self.packs.get(*id).into_iter().flat_map(|p| &p.rules) {
                by_id.insert(&r.id, r);
            }
        }
        let mut warnings = Vec::new();
        if level == Level::Project {
            for r in &self.local_rules {
                if by_id.contains_key(r.id.as_str())
                    || !self.packs.contains_key(&r.pack)
                    || active.contains(&r.pack.as_str())
                {
                    by_id.insert(&r.id, r);
                } else {
                    warnings.push(format!(
                        "{}: pack \"{}\" is not active here; rule {} is ignored",
                        r.file, r.pack, r.id
                    ));
                }
            }
        }
        let mut rules: Vec<&Rule> = by_id.into_values().collect();
        rules.sort_by(|a, b| a.id.cmp(&b.id));
        (rules, warnings)
    }

    /// Findings per lint.
    pub fn finding_counts(&self) -> HashMap<LintKind, usize> {
        let mut out = HashMap::new();
        for f in &self.findings {
            *out.entry(f.lint).or_default() += 1;
        }
        out
    }
}
