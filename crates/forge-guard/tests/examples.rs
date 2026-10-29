//! Every guard rule's pack examples (`examples` in its YAML, as forge-packs loads them): a
//! `match` example gets one of the rule's decisions, a `no_match` example gets none, the rule
//! alone, run in a directory holding the example's `files`.
//!
//! A rule that also has a `grep` target shares its examples with it: a string example that
//! is file content (a JSON object, or text ending in a newline) is the grep target's, not a
//! command.

mod common;

use common::*;
use forge_guard::{GuardRule, GuardSet, Outcome, PythonPm, converted};
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
    let pm = rule.pack.starts_with("python/uv").then_some(PythonPm::Uv);
    let mut out = Vec::new();
    for c in cases(rule) {
        let files: Vec<(&str, &str)> = c
            .files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let dir = repo(&files);
        let got = set.evaluate(&c.command, &ctx(dir.path()).with_python_pm(pm));
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
fn every_guard_rule_is_converted() {
    let ids: Vec<&str> = guard_rules().iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), 58);
    let conv = converted::all().expect("converted.yaml loads");
    let missing: Vec<&&str> = ids.iter().filter(|i| !conv.contains_key(**i)).collect();
    let unknown: Vec<&String> = conv.keys().filter(|k| !ids.contains(&k.as_str())).collect();
    assert!(
        missing.is_empty() && unknown.is_empty(),
        "{missing:?} {unknown:?}"
    );
}

#[test]
fn converted_rules_pass_their_pack_examples() {
    let n: usize = guard_rules().iter().map(|r| cases(r).len()).sum();
    assert!(n > 600, "{n} examples");
    let mut all = Vec::new();
    for r in guard_rules() {
        if let Some(g) = converted::rule(r) {
            all.extend(failures(r, g));
        }
    }
    assert!(all.is_empty(), "{all:#?}");
}

#[test]
fn pack_patterns_per_simple_command_on_their_examples() {
    // the pack patterns, matched per simple command, on the same examples: what fails is in
    // CONVERSION.md ("works unchanged")
    let mut all = Vec::new();
    for r in guard_rules() {
        if let Some(g) = GuardRule::from_pack(r) {
            all.extend(failures(r, g));
        }
    }
    let expected: Vec<&str> = include_str!("legacy-failures.txt")
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(
        all, expected,
        "update tests/legacy-failures.txt and CONVERSION.md"
    );
}
