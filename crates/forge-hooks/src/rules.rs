//! What a hook needs from the active rules, cached in the state directory so a hook does not load
//! the packs: the guard rules, the languages with typed rules, and the package-manager facts.
//!
//! The cache is keyed by the lock's size and mtime and by the packaging files at the root
//! (`uv.lock`, `pyproject.toml`, …), checked with one directory read: no parse, no plan while
//! neither changed. A change of either plans the repository again, so the guard follows the
//! packs active now (a project that moved from Poetry to uv during the session).

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use forge_engine::Lang;
use forge_guard::{GuardRule, GuardSet, GuardSpec};
use forge_packs::Rule;
use forge_packs::model::{Language, Stage, Target};
use serde::{Deserialize, Serialize};

use crate::state;

/// A guard rule as the cache stores it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredGuardRule {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub review: Option<String>,
    pub targets: Vec<GuardSpec>,
}

impl From<GuardRule> for StoredGuardRule {
    fn from(r: GuardRule) -> Self {
        StoredGuardRule {
            id: r.id,
            description: r.description,
            review: r.review,
            targets: r.targets,
        }
    }
}

impl From<StoredGuardRule> for GuardRule {
    fn from(r: StoredGuardRule) -> Self {
        GuardRule {
            id: r.id,
            description: r.description,
            review: r.review,
            targets: r.targets,
        }
    }
}

/// What the hooks know of the active rules.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HookCache {
    /// Forge's version and its packs' build: another build rebuilds the cache.
    pub build: String,
    pub lock_meta: String,
    pub rule_set: String,
    pub guard: Vec<StoredGuardRule>,
    /// Languages whose typed rules are active (the types engine starts for them).
    pub typed: Vec<Lang>,
    /// Languages whose type check is active (the edit-time typecheck asks the engine).
    #[serde(default)]
    pub typecheck: Vec<Lang>,
    /// The active rule ids.
    pub rules: Vec<String>,
    #[serde(default)]
    pub facts_key: Option<String>,
    #[serde(default)]
    pub python_pm: Option<String>,
    #[serde(default)]
    pub js_pm: Option<String>,
    /// The rules files the lock records (`.claude/rules/…`): what the agent was given.
    #[serde(default)]
    pub rules_files: Vec<String>,
}

impl HookCache {
    pub fn guard_set(&self) -> Option<GuardSet> {
        GuardSet::new_lazy(self.guard.iter().cloned().map(GuardRule::from).collect()).ok()
    }

    pub fn has_rule(&self, id: &str) -> bool {
        self.rules.iter().any(|r| r == id)
    }
}

fn build_id() -> String {
    format!("{} {}", env!("CARGO_PKG_VERSION"), forge_packs::BUILD_ID)
}

fn meta_key(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let t = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some(format!("{}:{t}", m.len()))
}

/// The packaging files at the root, by name, size and mtime.
pub fn facts_key(root: &Path) -> String {
    const NAMES: &[&str] = &[
        "uv.lock",
        "uv.toml",
        "poetry.lock",
        "pdm.lock",
        "Pipfile",
        "Pipfile.lock",
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "package.json",
        "bun.lock",
        "bun.lockb",
        "pnpm-lock.yaml",
        "pnpm-workspace.yaml",
        ".yarnrc.yml",
        "yarn.lock",
        "package-lock.json",
        "npm-shrinkwrap.json",
    ];
    let mut keys: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            if NAMES.contains(&name) || name.starts_with("requirements") {
                if let Some(m) = meta_key(&e.path()) {
                    keys.push(format!("{name}={m}"));
                }
            }
        }
    }
    keys.sort();
    keys.join(";")
}

/// The languages the active rules type-check or ask types for.
pub fn typed_languages<'a>(rules: impl IntoIterator<Item = &'a Rule>) -> Vec<Lang> {
    languages(rules, true)
}

/// The languages whose type check (a `typecheck` stage) is active.
pub fn typecheck_languages<'a>(rules: impl IntoIterator<Item = &'a Rule>) -> Vec<Lang> {
    languages(rules, false)
}

fn languages<'a>(rules: impl IntoIterator<Item = &'a Rule>, with_where: bool) -> Vec<Lang> {
    let mut out = Vec::new();
    for r in rules {
        let lang = match r.scope.language {
            Language::Python => Lang::Python,
            Language::Typescript | Language::Tsx => Lang::Typescript,
            _ => continue,
        };
        let typed = r.enforce.iter().any(|t| match t {
            Target::AstGrep(a) => with_where && a.where_.as_ref().is_some_and(|w| !w.is_empty()),
            Target::Check(c) => c.stage == Stage::Typecheck,
            _ => false,
        });
        if typed && !out.contains(&lang) {
            out.push(lang);
        }
    }
    out.sort();
    out
}

/// Where a level's rules come from.
pub struct Source<'a> {
    /// The lock naming the active rules.
    pub lock: PathBuf,
    /// The repository (its local packs, its facts); None at user level.
    pub repo: Option<&'a Path>,
    /// The parsed-pack cache directory.
    pub packs_cache: Option<PathBuf>,
}

/// The cache of `source`, rebuilt when the lock or the facts changed. None when there is no lock
/// (nothing applied) or the rules cannot be loaded; the hooks then do nothing.
pub fn load(state_dir: &Path, source: &Source) -> Option<HookCache> {
    let meta = meta_key(&source.lock)?;
    let file = state_dir.join("hook-cache.json");
    let cache: HookCache = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let build = build_id();
    let fk = source.repo.map(facts_key);
    if cache.build == build && cache.lock_meta == meta && cache.facts_key == fk {
        return Some(cache);
    }
    let text = std::fs::read_to_string(&source.lock).ok()?;
    let lock = forge_render::lock::Lock::parse(&text).ok()?;
    let mut fresh = rebuild(source, &lock)?;
    fresh.build = build;
    fresh.lock_meta = meta;
    fresh.rule_set = lock.rule_set.clone();
    fresh.facts_key = fk;
    fresh.rules_files = lock
        .files
        .keys()
        .filter(|f| f.starts_with(".claude/rules/"))
        .cloned()
        .collect();
    if state::ensure_dir(state_dir) {
        if let Ok(b) = serde_json::to_vec(&fresh) {
            state::write_atomic(&file, &b);
        }
    }
    Some(fresh)
}

/// The active rules: at user level, the lock's. In a repository, the rules a plan of the
/// repository as it is now selects (packaging files created or removed during the session
/// change the packs, as they do for the checks, §10.1), with the lock deciding for the packs it
/// records: a rule of a pack the lock records is active only if the lock records it too; a pack
/// the lock does not record gives the rules the plan selects. A plan that fails falls back to
/// the lock's rules.
fn rebuild(source: &Source, lock: &forge_render::lock::Lock) -> Option<HookCache> {
    let opts = forge_packs::Options {
        cache_dir: source.packs_cache.as_deref(),
    };
    let set = forge_packs::load(source.repo, opts).ok()?;
    let all = || {
        set.packs
            .values()
            .flat_map(|p| p.rules.iter())
            .chain(set.local_rules.iter())
    };
    let by_lock = || -> Vec<&Rule> { all().filter(|r| lock.rules.contains_key(&r.id)).collect() };
    let (active, (python_pm, js_pm)) = match source.repo {
        None => (by_lock(), (None, None)),
        Some(repo) => match plan_now(repo, &set) {
            Err(_) => (by_lock(), pm_facts(repo)),
            Ok((now, facts)) => {
                // the rules of the packs the lock records, and the local rules: the lock's
                let decided: Vec<&str> = set
                    .packs
                    .values()
                    .filter(|p| p.rules.iter().any(|r| lock.rules.contains_key(&r.id)))
                    .flat_map(|p| p.rules.iter())
                    .chain(set.local_rules.iter())
                    .map(|r| r.id.as_str())
                    .collect();
                let active = all()
                    .filter(|r| now.contains(&r.id))
                    .filter(|r| lock.rules.contains_key(&r.id) || !decided.contains(&r.id.as_str()))
                    .collect();
                (active, facts)
            }
        },
    };
    let guard = active
        .iter()
        .filter_map(|r| forge_guard::converted::rule(r))
        .map(StoredGuardRule::from)
        .collect();
    Some(HookCache {
        guard,
        typed: typed_languages(active.iter().copied()),
        typecheck: typecheck_languages(active.iter().copied()),
        rules: active.iter().map(|r| r.id.clone()).collect(),
        python_pm,
        js_pm,
        ..HookCache::default()
    })
}

/// The root's `python.pm` and `js.pm` facts.
type PmFacts = (Option<String>, Option<String>);

/// The rule ids a plan of `repo` as it is now selects, and the root's package-manager facts.
fn plan_now(repo: &Path, set: &forge_packs::PackSet) -> Result<(Vec<String>, PmFacts), String> {
    let config = forge_packs::load_project_config(repo).map_err(|e| e.to_string())?;
    let sel = forge_check::select::project(repo, set, config.as_ref())?;
    let fact = |name| sel.facts.root.fact(name).map(str::to_owned);
    Ok((
        sel.plan
            .active
            .iter()
            .map(|st| st.rule.id.clone())
            .collect(),
        (
            fact(forge_detect::FACT_PYTHON_PM),
            fact(forge_detect::FACT_JS_PM),
        ),
    ))
}

/// The root's `python.pm` and `js.pm` facts.
pub fn pm_facts(repo: &Path) -> PmFacts {
    let files = forge_detect::list_repo_files(repo);
    match forge_detect::read_project_facts(repo, &files) {
        Ok(f) => (
            f.root.fact(forge_detect::FACT_PYTHON_PM).map(str::to_owned),
            f.root.fact(forge_detect::FACT_JS_PM).map(str::to_owned),
        ),
        Err(_) => (None, None),
    }
}

pub fn js_pm(s: Option<&str>) -> Option<forge_guard::JsPm> {
    forge_guard::JsPm::from_fact(s?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_rules_round_trip() {
        let set = forge_packs::load_builtin(forge_packs::Options::default()).unwrap();
        let rules: Vec<StoredGuardRule> = set
            .packs
            .values()
            .flat_map(|p| p.rules.iter())
            .filter_map(forge_guard::converted::rule)
            .map(StoredGuardRule::from)
            .collect();
        assert!(rules.len() > 10);
        let json = serde_json::to_vec(&rules).unwrap();
        let back: Vec<StoredGuardRule> = serde_json::from_slice(&json).unwrap();
        assert_eq!(back, rules);
        GuardSet::new(back.into_iter().map(GuardRule::from).collect()).unwrap();
    }
}
