//! The packs' ast-grep and grep examples as golden tests: every `match` example must match and
//! every `no_match` example must not, as `ast-grep test` and `grep -E` decide today. A rule with
//! a `where` (types engine) only proves its `match` examples here, as the TypeScript self-test
//! did with ast-grep alone.

use forge_check::scope::Roles;
use forge_check::{astgrep, grep};
use forge_packs::model::{Example, Rule};

fn rules() -> Vec<Rule> {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs load");
    set.packs.values().flat_map(|p| p.rules.clone()).collect()
}

fn code(rule: &Rule, kind: &str) -> Vec<String> {
    let ex = rule.examples.as_ref();
    let list = match kind {
        "match" => ex.and_then(|e| e.match_.as_ref()),
        _ => ex.and_then(|e| e.no_match.as_ref()),
    };
    list.into_iter()
        .flatten()
        .filter_map(|e| match e {
            Example::Code(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn ast_grep_examples() {
    let roles = Roles::defaults();
    let mut failures = Vec::new();
    let (mut rules_n, mut examples) = (0, 0);
    for r in rules() {
        let Some(t) = astgrep::target_of(&r) else {
            continue;
        };
        rules_n += 1;
        let c = match astgrep::compile(&r, t, &roles, None) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{}: does not compile: {e}", r.id));
                continue;
            }
        };
        for (i, ex) in code(&r, "match").iter().enumerate() {
            examples += 1;
            if !astgrep::example_matches(&c, ex) {
                failures.push(format!(
                    "{}: match example {} is missing: {ex:?}",
                    r.id,
                    i + 1
                ));
            }
        }
        if t.where_.is_none() {
            for (i, ex) in code(&r, "no_match").iter().enumerate() {
                examples += 1;
                if astgrep::example_matches(&c, ex) {
                    failures.push(format!(
                        "{}: no_match example {} is noisy: {ex:?}",
                        r.id,
                        i + 1
                    ));
                }
            }
        }
    }
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
    let mut failures = Vec::new();
    let (mut rules_n, mut examples) = (0, 0);
    for r in rules() {
        let Some(t) = grep::target_of(&r) else {
            continue;
        };
        rules_n += 1;
        let g = match grep::compile(&r, t, &roles, None) {
            Ok(g) => g,
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        for (i, ex) in code(&r, "match").iter().enumerate() {
            examples += 1;
            if g.count(ex) == 0 {
                failures.push(format!(
                    "{}: grep does not match match example {}: {ex:?}",
                    r.id,
                    i + 1
                ));
            }
        }
        for (i, ex) in code(&r, "no_match").iter().enumerate() {
            examples += 1;
            if g.count(ex) > 0 {
                failures.push(format!(
                    "{}: grep matches no_match example {}: {ex:?}",
                    r.id,
                    i + 1
                ));
            }
        }
    }
    eprintln!("grep: {rules_n} rules, {examples} examples");
    assert!(rules_n > 40);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
