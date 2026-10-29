//! Every guard rule's pack examples (`examples` in its YAML, as forge-packs loads them): a
//! `match` example gets one of the rule's decisions, a `no_match` example gets none, the rule
//! alone, run in a directory holding the example's `files`.
//!
//! A rule that also has a `grep` target shares its examples with it: a string example that
//! is file content (a JSON object, or text ending in a newline) is the grep target's, not a
//! command.

mod common;

use common::*;
use forge_guard::{GuardRule, GuardSet, Outcome, PythonPm};
use forge_packs::model::{Example, Rule, Target};

struct Case {
    command: String,
    files: Vec<(String, String)>,
    must_match: bool,
}

fn cases(rule: &Rule) -> Vec<Case> {
    let has_grep = rule.enforce.iter().any(|t| matches!(t, Target::Grep(_)));
    let mut out = Vec::new();
    let Some(ex) = &rule.examples else {
        return out;
    };
    for (list, must_match) in [(&ex.match_, true), (&ex.no_match, false)] {
        for e in list.iter().flatten() {
            match e {
                Example::Code(c) => {
                    if has_grep && (c.trim_start().starts_with('{') || c.ends_with('\n')) {
                        continue;
                    }
                    out.push(Case {
                        command: c.clone(),
                        files: vec![],
                        must_match,
                    })
                }
                Example::Command(c) => out.push(Case {
                    command: c.command.clone(),
                    files: c
                        .files
                        .iter()
                        .flatten()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    must_match,
                }),
                Example::Fixture(_) => {}
            }
        }
    }
    out
}

/// The example project's `python.pm`, from its files as forge-detect reads them.
fn python_pm(files: &[(String, String)]) -> Option<PythonPm> {
    let dir = repo(
        &files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect::<Vec<_>>(),
    );
    let listed: Vec<String> = files.iter().map(|(a, _)| a.clone()).collect();
    let facts = forge_detect::read_project_facts(dir.path(), &listed).ok()?;
    PythonPm::from_fact(facts.root.fact(forge_detect::FACT_PYTHON_PM)?)
}

/// The failures of `guard` on the rule's examples.
pub fn failures(rule: &Rule, guard: GuardRule) -> Vec<String> {
    let decisions: Vec<&str> = guard
        .targets
        .iter()
        .map(|t| match t.decision {
            forge_guard::Decision::Deny => "deny",
            forge_guard::Decision::Ask => "ask",
        })
        .collect();
    let set = GuardSet::new(vec![guard]).expect("compiles");
    let mut out = Vec::new();
    for c in cases(rule) {
        let files: Vec<(&str, &str)> = c
            .files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let dir = repo(&files);
        let got = set.evaluate(
            &c.command,
            &ctx(dir.path()).with_python_pm(python_pm(&c.files)),
        );
        let ok = if c.must_match {
            got != Outcome::None && decisions.contains(&got.decision())
        } else {
            got == Outcome::None
        };
        if !ok {
            out.push(format!(
                "{}: {} {:?} got {:?}",
                rule.id,
                if c.must_match { "match" } else { "no_match" },
                c.command,
                got.decision()
            ));
        }
    }
    out
}

#[test]
fn guard_rules_pass_their_pack_examples() {
    let n: usize = guard_rules().iter().map(|r| cases(r).len()).sum();
    assert!(n > 600, "{n} examples");
    let mut all = Vec::new();
    for r in guard_rules() {
        if let Some(g) = GuardRule::from_pack(r) {
            all.extend(failures(r, g));
        }
    }
    assert!(all.is_empty(), "{all:#?}");
}
