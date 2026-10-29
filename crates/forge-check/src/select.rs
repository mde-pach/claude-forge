//! Which rules a repository's check runs, until forge-detect provides project facts and the
//! planner's selection: a small stand-in for `activePacks` and `selectRules` (`src/plan/`).
//!
//! A project-level pack is active when forge.yaml adds it, or when one of its `detect` keys
//! matches (`always`, `files_any`, `deps_any`, `content_any`) under an active parent; packs it
//! requires come along; forge.yaml's `remove` takes a pack, its subtree and the packs requiring
//! it out. Rules are the active packs' and the local ones, less forge.yaml's `disable`, with a
//! `when.deps` gate on a declared dependency. Workspaces are not looked into, versions not
//! compared: forge-detect does both.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use forge_packs::PackSet;
use forge_packs::model::{DepSpec, Ecosystem, Level, Pack, ProjectConfig};

use crate::change::{Git, split_z};
use crate::checker::ActiveRule;

/// What detection reads from the repository.
#[derive(Debug, Default)]
pub struct Facts {
    pub files: Vec<String>,
    pub npm: BTreeSet<String>,
    pub pypi: BTreeSet<String>,
}

fn pep503(n: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in n.chars() {
        if matches!(c, '-' | '_' | '.') {
            sep = true;
        } else {
            if sep && !out.is_empty() {
                out.push('-');
            }
            sep = false;
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

fn requirement_name(spec: &str) -> Option<String> {
    let t = spec.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with('-') {
        return None;
    }
    let name: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    (!name.is_empty()).then(|| pep503(&name))
}

impl Facts {
    pub fn read(root: &Path) -> Facts {
        let git = Git::new(root);
        let mut files: Vec<String> = split_z(
            &git.run(&[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .unwrap_or_default(),
        )
        .filter(|p| !crate::scope::is_excluded(p))
        .collect();
        files.sort();
        files.dedup();
        let mut f = Facts {
            files,
            ..Facts::default()
        };
        for p in f
            .files
            .iter()
            .filter(|p| *p == "package.json" || p.ends_with("/package.json"))
        {
            let Ok(t) = std::fs::read_to_string(root.join(p)) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else {
                continue;
            };
            for s in [
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
            ] {
                if let Some(m) = v.get(s).and_then(|x| x.as_object()) {
                    f.npm.extend(m.keys().cloned());
                }
            }
        }
        if let Ok(t) = std::fs::read_to_string(root.join("pyproject.toml")) {
            let mut section = String::new();
            let mut in_deps = false;
            for line in t.lines() {
                let l = line.trim();
                if l.starts_with('[') {
                    section = l.trim_matches(|c| c == '[' || c == ']').trim().to_string();
                    in_deps = false;
                    continue;
                }
                if section == "project"
                    && (l.starts_with("dependencies") || l.starts_with("optional-dependencies"))
                {
                    in_deps = true;
                }
                if section.starts_with("tool.poetry") && section.ends_with("dependencies") {
                    if let Some((k, _)) = l.split_once('=')
                        && k.trim() != "python"
                    {
                        f.pypi.insert(pep503(k.trim().trim_matches('"')));
                    }
                    continue;
                }
                if (in_deps
                    || section == "dependency-groups"
                    || section == "project.optional-dependencies")
                    && l.contains('"')
                {
                    for part in l.split('"').skip(1).step_by(2) {
                        if let Some(n) = requirement_name(part) {
                            f.pypi.insert(n);
                        }
                    }
                }
                if in_deps && l.contains(']') {
                    in_deps = false;
                }
            }
        }
        for p in f
            .files
            .iter()
            .filter(|p| !p.contains('/') && p.starts_with("requirements") && p.ends_with(".txt"))
        {
            for l in std::fs::read_to_string(root.join(p))
                .unwrap_or_default()
                .lines()
            {
                if let Some(n) = requirement_name(l) {
                    f.pypi.insert(n);
                }
            }
        }
        f
    }

    pub fn has_dep(&self, d: &DepSpec) -> bool {
        match d.ecosystem {
            Ecosystem::Npm => self.npm.contains(&d.name),
            Ecosystem::Pypi => self.pypi.contains(&pep503(&d.name)),
        }
    }
}

fn detected(root: &Path, p: &Pack, facts: &Facts) -> bool {
    let Some(d) = &p.detect else { return false };
    if d.always == Some(true) {
        return true;
    }
    if let Some(globs) = &d.files_any
        && globs.iter().any(|g| {
            facts
                .files
                .iter()
                .any(|f| forge_packs::glob::glob_match(g, f))
        })
    {
        return true;
    }
    if let Some(deps) = &d.deps_any
        && deps.iter().any(|x| facts.has_dep(x))
    {
        return true;
    }
    if let Some(cs) = &d.content_any {
        for c in cs {
            let Ok(re) = regex::Regex::new(&crate::grep::ere_to_regex(&c.pattern)) else {
                continue;
            };
            for f in facts
                .files
                .iter()
                .filter(|f| c.files.iter().any(|g| forge_packs::glob::glob_match(g, f)))
            {
                if std::fs::read_to_string(root.join(f))
                    .is_ok_and(|t| t.lines().any(|l| re.is_match(l)))
                {
                    return true;
                }
            }
        }
    }
    false
}

/// The active packs, sorted.
pub fn active_packs(
    root: &Path,
    set: &PackSet,
    config: Option<&ProjectConfig>,
    facts: &Facts,
) -> Vec<String> {
    let mut why: BTreeMap<String, ()> = BTreeMap::new();
    for id in config
        .and_then(|c| c.packs.as_ref())
        .and_then(|p| p.add.as_ref())
        .into_iter()
        .flatten()
    {
        for a in set.lineage(id) {
            why.insert(a.to_string(), ());
        }
    }
    loop {
        let mut changed = false;
        for p in set.packs.values() {
            if p.level != Level::Project || why.contains_key(&p.id) {
                continue;
            }
            if p.parent.as_ref().is_none_or(|par| why.contains_key(par)) && detected(root, p, facts)
            {
                why.insert(p.id.clone(), ());
                changed = true;
            }
        }
        let ids: Vec<String> = why.keys().cloned().collect();
        for id in ids {
            for req in set
                .packs
                .get(&id)
                .map(|p| p.requires.clone())
                .unwrap_or_default()
            {
                for a in set.lineage(&req) {
                    if why.insert(a.to_string(), ()).is_none() {
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut removed: BTreeSet<String> = config
        .and_then(|c| c.packs.as_ref())
        .and_then(|p| p.remove.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    loop {
        let mut changed = false;
        for id in why.keys() {
            if removed.contains(id) {
                continue;
            }
            let p = &set.packs[id];
            if p.parent.as_ref().is_some_and(|x| removed.contains(x))
                || p.requires.iter().any(|r| removed.contains(r))
            {
                removed.insert(id.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    why.into_keys().filter(|id| !removed.contains(id)).collect()
}

/// The rules the check runs in `root`.
pub fn active_rules(root: &Path, set: &PackSet, config: Option<&ProjectConfig>) -> Vec<ActiveRule> {
    let facts = Facts::read(root);
    let packs = active_packs(root, set, config, &facts);
    let refs: Vec<&str> = packs.iter().map(String::as_str).collect();
    let (rules, _) = set.select_rules(&refs, Level::Project);
    let disabled: BTreeSet<&str> = config
        .and_then(|c| c.disable.as_ref())
        .into_iter()
        .flatten()
        .map(|d| d.id.as_str())
        .collect();
    rules
        .into_iter()
        .filter(|r| !disabled.contains(r.id.as_str()))
        .filter(|r| r.deprecated.is_none())
        .filter(|r| {
            r.when
                .as_ref()
                .is_none_or(|w| w.deps.iter().all(|d| facts.has_dep(d)))
        })
        .map(|r| ActiveRule::new(r.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(pep503("Django_REST.framework"), "django-rest-framework");
        assert_eq!(
            requirement_name("fastapi[all]>=0.110").as_deref(),
            Some("fastapi")
        );
        assert_eq!(requirement_name("# x"), None);
    }
}
