//! Model ids: deprecated and unknown Claude model ids on added lines (the original
//! `ai-models.sh`). The model list is the ai-models pack's data file, embedded at build time.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::LazyLock;

use forge_core::builtin::BuiltinCtx;
use forge_core::{Finding, Report};
use regex::Regex;

use crate::CodeBuiltin;
use crate::ctx::{Extra, read};
use crate::scope::is_prose;

/// The ai-models pack's model table.
pub const MODELS_TSV: &str = include_str!("../../../packs/ai-models/models.tsv");

/// A deprecated model: its retirement date ("-": none announced) and replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deprecated {
    pub retires: String,
    pub replacement: String,
}

/// The deprecated ids and the known (published) Claude ids of a model table.
#[derive(Debug, Clone, Default)]
pub struct Models {
    pub deprecated: BTreeMap<String, Deprecated>,
    pub known: BTreeSet<String>,
}

impl Models {
    /// `id<TAB>provider<TAB>status<TAB>retires<TAB>replacement<TAB>rejects` rows.
    pub fn parse(tsv: &str) -> Models {
        let mut m = Models::default();
        for l in tsv
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        {
            let c: Vec<&str> = l.split('\t').collect();
            if c.len() < 5 {
                continue;
            }
            if c[2] == "deprecated" {
                m.deprecated.insert(
                    c[0].to_string(),
                    Deprecated {
                        retires: c[3].to_string(),
                        replacement: c[4].to_string(),
                    },
                );
            }
            if c[1] == "anthropic" {
                m.known.insert(c[0].to_string());
            }
        }
        m
    }

    /// The pack's rendered arguments: `<id>|<retires>|<replacement>;…` and the known ids.
    pub fn from_args(deprecated: &str, known: &str) -> Models {
        let mut m = Models::default();
        for row in deprecated.split(';') {
            let c: Vec<&str> = row.split('|').collect();
            if c.len() == 3 {
                m.deprecated.insert(
                    c[0].to_string(),
                    Deprecated {
                        retires: c[1].to_string(),
                        replacement: c[2].to_string(),
                    },
                );
            }
        }
        m.known = known.split_whitespace().map(str::to_string).collect();
        m
    }
}

static EMBEDDED: LazyLock<Models> = LazyLock::new(|| Models::parse(MODELS_TSV));

/// `forge_model_ids [<deprecated> <known>]`: for each quoted model id on an added line of an
/// in-scope file (not prose: docs, Markdown, changelogs), optionally after pydantic-ai's
/// `anthropic:` / `openai:` or LiteLLM's `anthropic/` / `openai/` prefix: a deprecated id with
/// its date and replacement; a dated Claude id that is not a published one. Without arguments
/// the embedded model table is used. Run as advice.
pub struct ModelIds;

impl CodeBuiltin for ModelIds {
    fn name(&self) -> &'static str {
        "model_ids"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, args: &[String]) -> Report {
        static QUOTED: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"["'][A-Za-z0-9._:/@-]+["']"#).expect("regex"));
        static PREFIX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^(anthropic|openai)[:/]").expect("regex"));
        static DATED: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"^claude-(opus|sonnet|haiku|fable|mythos)-[0-9][0-9-]*-20[0-9]{6}$")
                .expect("regex")
        });
        let given;
        let models: &Models = match args {
            [d, k, ..] => {
                given = Models::from_args(d, k);
                &given
            }
            _ => &EMBEDDED,
        };
        let mut r = Report::default();
        for f in x.files.iter().filter(|f| !is_prose(f)) {
            let Some(text) = read(&ctx.root, f) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                let n = i as u32 + 1;
                if !x.on_added(f, n) {
                    continue;
                }
                let mut seen = HashSet::new();
                for m in QUOTED.find_iter(line) {
                    let s = m.as_str();
                    let id = PREFIX.replace(&s[1..s.len() - 1], "").into_owned();
                    let msg = if let Some(d) = models.deprecated.get(&id) {
                        if d.retires == "-" {
                            format!("{id} is deprecated; use {}", d.replacement)
                        } else {
                            format!("{id} retires {}; use {}", d.retires, d.replacement)
                        }
                    } else if DATED.is_match(&id) && !models.known.contains(&id) {
                        format!(
                            "{id} is not a published Claude model id; use its alias (e.g. claude-opus-5-5)"
                        )
                    } else {
                        continue;
                    };
                    if seen.insert(id) {
                        r.findings
                            .push(Finding::error(&ctx.rule, msg).at(f.as_str(), Some(n)));
                    }
                }
            }
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_matches_the_rule_arguments() {
        let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
        let rule = set
            .packs
            .values()
            .flat_map(|p| p.rules.iter())
            .find(|r| r.id == "ai-models/deprecated-model-ids")
            .expect("rule");
        let run = rule
            .enforce
            .iter()
            .find_map(|t| match t {
                forge_packs::model::Target::Check(c) => Some(c.run.clone()),
                _ => None,
            })
            .expect("check");
        let parts: Vec<&str> = run.split('\'').collect();
        let from_rule = Models::from_args(parts[1], parts[3]);
        assert_eq!(from_rule.deprecated, EMBEDDED.deprecated);
        assert_eq!(from_rule.known, EMBEDDED.known);
    }
}
