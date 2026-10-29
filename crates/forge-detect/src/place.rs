//! Where things apply in a repository (§5.1, `src/plan/placement.ts`): which audited tools are
//! configured where, in which workspace packages a pack was detected, where a rule's version
//! gate holds, and where a check runs.

use std::collections::BTreeMap;
use std::path::Path;

use forge_packs::PackSet;
use forge_packs::model::{AstGrepTarget, AuditedTool, CheckTarget, Level, Rule};

use crate::activate::{Activation, detect_pack};
use crate::deps::read_toml;
use crate::facts::{DepFacts, ProjectFacts};
use crate::tools::{audited_tools, per_package};

/// ESLint's flat config files (ESLint 9 and later read nothing else).
pub const ESLINT_CONFIGS: &[&str] = &[
    "eslint.config.js",
    "eslint.config.mjs",
    "eslint.config.cjs",
    "eslint.config.ts",
    "eslint.config.mts",
    "eslint.config.cts",
];

#[derive(Debug, Clone, Copy, Default)]
struct ToolHere {
    config: bool,
    dep: bool,
}

fn tools_in(dir: &Path, deps: &DepFacts) -> BTreeMap<AuditedTool, ToolHere> {
    let has = |f: &str| dir.join(f).exists();
    let py = read_toml(&dir.join("pyproject.toml"));
    let scripts = !["pyproject.toml", "setup.py", "setup.cfg", "Pipfile"]
        .iter()
        .any(|f| has(f))
        && deps.sources(&["**/*.py".to_string()]);
    let ruff_cfg = py
        .as_ref()
        .is_some_and(|p| p.pointer("/tool/ruff").is_some());
    let mut out = BTreeMap::new();
    out.insert(
        AuditedTool::Biome,
        ToolHere {
            config: has("biome.json") || has("biome.jsonc"),
            dep: deps.npm.contains_key("@biomejs/biome"),
        },
    );
    out.insert(
        AuditedTool::Ruff,
        ToolHere {
            config: has("ruff.toml") || has(".ruff.toml") || ruff_cfg || scripts,
            dep: deps.pypi.contains_key("ruff"),
        },
    );
    out.insert(
        AuditedTool::Ty,
        ToolHere {
            config: has("pyproject.toml") || has("ty.toml") || scripts,
            dep: false,
        },
    );
    out.insert(
        AuditedTool::Tsc,
        ToolHere {
            config: has("tsconfig.json"),
            dep: deps.npm.contains_key("typescript"),
        },
    );
    out.insert(
        AuditedTool::Eslint,
        ToolHere {
            config: ESLINT_CONFIGS.iter().any(|f| has(f)),
            dep: deps.npm.contains_key("eslint"),
        },
    );
    out
}

/// Where each audited tool is configured or depended on: "" for the root, else workspace
/// package directories. A root that only depends on a tool does not count when a workspace
/// package has the tool's config file.
pub fn tool_places(repo: &Path, facts: &ProjectFacts) -> BTreeMap<AuditedTool, Vec<String>> {
    let root = tools_in(repo, &facts.root);
    let ws: Vec<(&str, BTreeMap<AuditedTool, ToolHere>)> = facts
        .workspaces
        .iter()
        .map(|w| (w.dir.as_str(), tools_in(&repo.join(&w.dir), w)))
        .collect();
    let mut out = BTreeMap::new();
    for &t in AuditedTool::ALL {
        let r = root[&t];
        let at_root = r.config || (r.dep && !ws.iter().any(|(_, w)| w[&t].config));
        let mut places = Vec::new();
        if at_root {
            places.push(String::new());
        }
        places.extend(
            ws.iter()
                .filter(|(_, w)| w[&t].config || w[&t].dep)
                .map(|(d, _)| d.to_string()),
        );
        out.insert(t, places);
    }
    out
}

/// The repository a plan places rules in.
pub struct Site<'a> {
    pub set: &'a PackSet,
    pub facts: Option<&'a ProjectFacts>,
    pub active: &'a Activation,
    /// Where each audited tool is configured (project level).
    pub places: Option<BTreeMap<AuditedTool, Vec<String>>>,
}

impl<'a> Site<'a> {
    pub fn new(
        level: Level,
        repo: &Path,
        set: &'a PackSet,
        facts: Option<&'a ProjectFacts>,
        active: &'a Activation,
    ) -> Site<'a> {
        let places = match (level, facts) {
            (Level::Project, Some(f)) => Some(tool_places(repo, f)),
            _ => None,
        };
        Site {
            set,
            facts: if level == Level::Project { facts } else { None },
            active,
            places,
        }
    }

    fn workspaces(&self) -> &'a [DepFacts] {
        self.facts.map(|f| f.workspaces.as_slice()).unwrap_or(&[])
    }

    /// Whether an audited tool is configured anywhere.
    pub fn tool_present(&self, t: AuditedTool) -> bool {
        self.places
            .as_ref()
            .is_some_and(|p| p.get(&t).is_some_and(|v| !v.is_empty()))
    }

    /// Where each audited tool's audits run: its places, or the root when it has none.
    pub fn tool_dirs(&self, t: AuditedTool) -> Vec<String> {
        match self.places.as_ref().and_then(|p| p.get(&t)) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => vec![String::new()],
        }
    }

    /// The workspace packages where a pack was detected, or `None` when it applies to the
    /// whole repository (detected at the root, added in forge.yaml, required, a parent of an
    /// added pack, or a local rule's pack outside the tree).
    pub fn pack_dirs(&self, id: &str) -> Option<Vec<String>> {
        let pack = self.set.packs.get(id)?;
        let facts = self.facts?;
        if facts.workspaces.is_empty()
            || self.active.why(id) != Some("detected")
            || detect_pack(pack, &facts.root)
        {
            return None;
        }
        Some(
            facts
                .workspaces
                .iter()
                .filter(|w| detect_pack(pack, w))
                .map(|w| w.dir.clone())
                .collect(),
        )
    }

    /// The places a pack's context conditions are judged in: the root ("") and the workspace
    /// packages where the pack is detected; the root alone when it is detected nowhere (added
    /// in forge.yaml, required, a parent of an added pack). A condition such as
    /// `python.pm: none` is then never judged in a package the pack does not apply to.
    pub fn pack_places(&self, id: &str) -> Vec<String> {
        let (Some(pack), Some(facts)) = (self.set.packs.get(id), self.facts) else {
            return vec![String::new()];
        };
        let mut out: Vec<String> = Vec::new();
        if detect_pack(pack, &facts.root) {
            out.push(String::new());
        }
        out.extend(
            facts
                .workspaces
                .iter()
                .filter(|w| detect_pack(pack, w))
                .map(|w| w.dir.clone()),
        );
        if out.is_empty() {
            out.push(String::new());
        }
        out
    }

    /// Where a rule applies: its pack's places, narrowed to those where its version gate
    /// holds. `Some([])`: the gate holds nowhere.
    pub fn rule_dirs(&self, rule: &Rule) -> Option<Vec<String>> {
        let where_ = self.pack_dirs(&rule.pack);
        let (Some(when), Some(facts)) = (&rule.when, self.facts) else {
            return where_;
        };
        let holds = |f: &DepFacts| when.deps.iter().all(|d| f.dep_matches(d));
        match where_ {
            Some(dirs) => Some(
                facts
                    .workspaces
                    .iter()
                    .filter(|w| dirs.contains(&w.dir) && holds(w))
                    .map(|w| w.dir.clone())
                    .collect(),
            ),
            None if holds(&facts.root) => None,
            None => Some(
                facts
                    .workspaces
                    .iter()
                    .filter(|w| holds(w))
                    .map(|w| w.dir.clone())
                    .collect(),
            ),
        }
    }

    /// Whether the rule's version gate leaves it out everywhere.
    pub fn gated_out(&self, rule: &Rule) -> bool {
        rule.when.is_some()
            && self.facts.is_some()
            && self.rule_dirs(rule).is_some_and(|d| d.is_empty())
    }

    /// The packages an ast-grep target with `needs` lacks where the rule applies.
    pub fn missing_needs(&self, rule: &Rule, t: &AstGrepTarget) -> Vec<String> {
        let Some(needs) = &t.needs else {
            return Vec::new();
        };
        let Some(facts) = self.facts else {
            return needs.iter().map(|n| n.name.clone()).collect();
        };
        let dirs = self.rule_dirs(rule);
        let spec = |n: &forge_packs::model::Need| forge_packs::model::DepSpec {
            ecosystem: n.ecosystem,
            name: n.name.clone(),
            version: None,
        };
        needs
            .iter()
            .filter(|n| match &dirs {
                Some(d) => !self
                    .workspaces()
                    .iter()
                    .filter(|w| d.contains(&w.dir))
                    .all(|w| w.dep_matches(&spec(n))),
                None => !std::iter::once(&facts.root)
                    .chain(facts.workspaces.iter())
                    .any(|w| w.dep_matches(&spec(n))),
            })
            .map(|n| n.name.clone())
            .collect()
    }

    /// Where a check runs: at the root (`dirs: None`), or in workspace packages; `absent`: the
    /// audited tools it needs that are configured nowhere it could run.
    pub fn check_place(&self, rule: &Rule, t: &CheckTarget) -> CheckPlace {
        let audited = audited_tools(t);
        let absent: Vec<AuditedTool> = audited
            .iter()
            .copied()
            .filter(|x| !self.tool_present(*x))
            .collect();
        let (Some(places), Some(_)) = (&self.places, self.facts) else {
            return CheckPlace { dirs: None, absent };
        };
        if !absent.is_empty() || self.workspaces().is_empty() || !per_package(t) {
            return CheckPlace { dirs: None, absent };
        }
        let where_ = self.rule_dirs(rule);
        if where_.is_none()
            && audited
                .iter()
                .all(|x| places[x].iter().any(|d| d.is_empty()))
        {
            return CheckPlace::default();
        }
        let dirs: Vec<String> = self
            .workspaces()
            .iter()
            .filter(|w| {
                where_.as_ref().is_none_or(|d| d.contains(&w.dir))
                    && audited.iter().all(|x| places[x].contains(&w.dir))
            })
            .map(|w| w.dir.clone())
            .collect();
        if dirs.is_empty() {
            CheckPlace {
                dirs: None,
                absent: audited,
            }
        } else {
            CheckPlace {
                dirs: Some(dirs),
                absent: Vec::new(),
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckPlace {
    pub dirs: Option<Vec<String>>,
    pub absent: Vec<AuditedTool>,
}
