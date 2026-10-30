//! A guard rule's examples (`examples` in its YAML): a `match` example gets one of the rule's
//! decisions, a `no_match` example gets none, the rule alone, run in a directory holding the
//! example's `files` (with the `python.pm` fact forge-detect reads from them).
//!
//! A rule that also has a `grep` target shares its examples with it: a string example that is
//! file content (a JSON object, or text ending in a newline) is the grep target's, not a
//! command. Fixtures are the check target's.

use std::path::Path;

use forge_core::examples::{ExampleResult, ExampleVerdict};
use forge_packs::model::{Example, Rule, Target};

use crate::{Decision, GuardCtx, GuardRule, GuardSet, Outcome, PythonPm};

/// An example a guard decides on: its list, its place, the command, the files around it.
type Case = (
    forge_core::examples::ExampleKind,
    usize,
    String,
    Vec<(String, String)>,
);

/// The commands of a rule's examples a guard decides on, with their files.
fn cases(rule: &Rule) -> Vec<Case> {
    let has_grep = rule.enforce.iter().any(|t| matches!(t, Target::Grep(_)));
    let Some(ex) = &rule.examples else {
        return Vec::new();
    };
    ex.listed()
        .into_iter()
        .filter_map(|(kind, n, e)| match e {
            Example::Code(c)
                if has_grep && (c.trim_start().starts_with('{') || c.ends_with('\n')) =>
            {
                None
            }
            Example::Code(c) => Some((kind, n, c.clone(), Vec::new())),
            Example::Command(c) => Some((
                kind,
                n,
                c.command.clone(),
                c.files
                    .iter()
                    .flatten()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )),
            Example::Fixture(_) => None,
        })
        .collect()
}

/// The example project's `python.pm`, from its files as forge-detect reads them.
fn python_pm(root: &Path, files: &[(String, String)]) -> Option<PythonPm> {
    let listed: Vec<String> = files.iter().map(|(a, _)| a.clone()).collect();
    let facts = forge_detect::read_project_facts(root, &listed).ok()?;
    PythonPm::from_fact(facts.root.fact(forge_detect::FACT_PYTHON_PM)?)
}

fn write(root: &Path, files: &[(String, String)]) -> std::io::Result<()> {
    std::fs::create_dir_all(root.join(".git"))?;
    for (p, c) in files {
        let f = root.join(p);
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&f, c)?;
        #[cfg(unix)]
        if p.contains("/bin/") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

/// The results of the rule's guard targets on its examples; none when it has no guard target.
pub fn run(rule: &Rule) -> Vec<ExampleResult> {
    let Some(guard) = GuardRule::from_pack(rule) else {
        return Vec::new();
    };
    let decisions: Vec<&str> = guard
        .targets
        .iter()
        .map(|t| match t.decision {
            Decision::Deny => "deny",
            Decision::Ask => "ask",
        })
        .collect();
    let fail_all = |why: String| -> Vec<ExampleResult> {
        cases(rule)
            .into_iter()
            .map(|(kind, n, _, _)| {
                ExampleResult::new(
                    &rule.id,
                    "guard",
                    kind,
                    n,
                    ExampleVerdict::Fail { why: why.clone() },
                )
            })
            .collect()
    };
    let set = match GuardSet::new(vec![guard]) {
        Ok(s) => s,
        Err(e) => return fail_all(format!("the guard does not compile: {e}")),
    };
    let mut out = Vec::new();
    for (kind, n, command, files) in cases(rule) {
        let verdict = match tempfile::tempdir() {
            Err(e) => ExampleVerdict::Fail {
                why: format!("no temporary directory: {e}"),
            },
            Ok(dir) => match write(dir.path(), &files).and_then(|()| dir.path().canonicalize()) {
                Err(e) => ExampleVerdict::Fail {
                    why: format!("its files could not be written: {e}"),
                },
                Ok(root) => {
                    let ctx = GuardCtx::new(root.clone(), root.clone())
                        .with_home(Some("/home/u".into()))
                        .with_python_pm(python_pm(&root, &files));
                    let got = set.evaluate(&command, &ctx);
                    let caught = got != Outcome::None && decisions.contains(&got.decision());
                    let ok = match kind {
                        forge_core::examples::ExampleKind::Match => caught,
                        forge_core::examples::ExampleKind::NoMatch => got == Outcome::None,
                    };
                    if ok {
                        ExampleVerdict::Pass
                    } else {
                        let got = match got.decision() {
                            "" => "no decision".to_string(),
                            d => format!("decision {d}"),
                        };
                        ExampleVerdict::Fail {
                            why: format!("{command:?} got {got}"),
                        }
                    }
                }
            },
        };
        out.push(ExampleResult::new(&rule.id, "guard", kind, n, verdict));
    }
    out
}
