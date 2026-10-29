//! The packs' ast-grep and grep examples as golden tests, through `forge_check::examples` (what
//! `forge test` runs): every `match` example must match and every `no_match` example must not.
//! A rule with a `where` (types engine) only proves its `match` examples here.

use forge_check::examples;
use forge_check::scope::Roles;
use forge_core::examples::{ExampleResult, ExampleVerdict};
use forge_packs::model::Rule;

fn rules() -> Vec<Rule> {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs load");
    set.packs.values().flat_map(|p| p.rules.clone()).collect()
}

/// The rules that ran, the examples that ran, and the failures.
fn tally(results: impl Iterator<Item = Vec<ExampleResult>>) -> (usize, usize, Vec<String>) {
    let (mut rules_n, mut examples, mut failures) = (0, 0, Vec::new());
    for rs in results {
        if rs.is_empty() {
            continue;
        }
        rules_n += 1;
        for r in rs {
            match &r.verdict {
                ExampleVerdict::Pass => examples += 1,
                ExampleVerdict::Fail { why } => {
                    examples += 1;
                    failures.push(format!("{}: {why}", r.label()));
                }
                ExampleVerdict::Skip { .. } => {}
            }
        }
    }
    (rules_n, examples, failures)
}

#[test]
fn ast_grep_examples() {
    let roles = Roles::defaults();
    let all = rules();
    let (rules_n, examples, failures) = tally(all.iter().map(|r| examples::ast_grep(r, &roles)));
    eprintln!("ast-grep: {rules_n} rules, {examples} examples");
    assert!(rules_n > 250, "{rules_n} ast-grep rules");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn grep_examples() {
    let roles = Roles::defaults();
    let all = rules();
    let (rules_n, examples, failures) = tally(all.iter().map(|r| examples::grep(r, &roles)));
    eprintln!("grep: {rules_n} rules, {examples} examples");
    assert!(rules_n > 40);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
