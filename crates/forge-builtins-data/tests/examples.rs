//! Every pack example of a rule whose check calls one of this crate's builtins holds when the
//! builtin runs on it as the checker would run it (pre-commit: the change against HEAD, the
//! check's files the in-scope changed ones): a `match` example finds something, a `no_match`
//! one nothing. Agent text never names Forge's internals.
//!
//! A tool the machine lacks (uvx for Squawk and sqlfluff) skips the examples that need it.

mod common;

use std::collections::BTreeMap;

use common::*;
use forge_builtins_data::{Mode, builtins, names};
use forge_packs::model::{Example, Target};

struct Call {
    name: String,
    args: Vec<String>,
}

/// The builtin a check command calls, with its literal arguments (`{files}` expanded).
fn call(cmd: &str, files: &[String]) -> Option<Call> {
    let ours = names();
    let cmd = cmd.replace("forge_advise ", "");
    let re = regex::Regex::new(r"forge_([a-z_]+)((?: [^|;&$()\n]+)?)").unwrap();
    for c in re.captures_iter(&cmd) {
        let name = c[1].to_owned();
        if !ours.contains(&name.as_str()) {
            continue;
        }
        let mut args = vec![];
        for w in c[2].split_whitespace() {
            if w == "{files}" {
                args.extend(files.iter().cloned());
            } else {
                args.push(w.to_owned());
            }
        }
        return Some(Call { name, args });
    }
    None
}

#[test]
fn pack_examples_hold() {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
    let uvx = have("uvx", &["--version"]);
    let (mut ran, mut skipped) = (0, vec![]);
    let mut failures = vec![];
    for pack in set.packs.values() {
        for rule in &pack.rules {
            for t in &rule.enforce {
                let Target::Check(c) = t else { continue };
                let cmd = match &c.changed_run {
                    Some(r) if r.contains("{files}") => r.clone(),
                    _ => c.run.clone(),
                };
                if call(&cmd, &[]).is_none() {
                    continue;
                }
                let Some(ex) = &rule.examples else { continue };
                for (want, list) in [(true, &ex.match_), (false, &ex.no_match)] {
                    for (i, e) in list.iter().flatten().enumerate() {
                        let Example::Fixture(fx) = e else { continue };
                        let name = format!(
                            "{} {}[{i}]",
                            rule.id,
                            if want { "match" } else { "no_match" }
                        );
                        let head: BTreeMap<String, String> =
                            fx.head.clone().unwrap_or_default().into_iter().collect();
                        let files: BTreeMap<String, String> =
                            fx.files.clone().into_iter().collect();
                        let needs_uvx = cmd.contains("squawk") || cmd.contains("sqlfluff");
                        if needs_uvx && !uvx {
                            skipped.push(name);
                            continue;
                        }
                        let r = repo(&head, &files);
                        // the check's files: the in-scope changed files that exist
                        let check_files: Vec<String> = r
                            .existing()
                            .into_iter()
                            .filter(|p| in_scope(rule, p))
                            .collect();
                        let triggered = match &c.when_changed {
                            Some(w) => r.changed.iter().any(|p| when_changed(w, p)),
                            None => !check_files.is_empty(),
                        };
                        let mut found: Vec<String> = vec![];
                        let mut notes: Vec<String> = vec![];
                        if triggered {
                            let cl = call(&cmd, &check_files).unwrap();
                            let mut args = cl.args;
                            let prisma = cmd.contains("$_dm_files");
                            if prisma {
                                args = check_files.clone();
                            }
                            let all = builtins(options(facts(Mode::Changed, check_files.clone())));
                            let b = all.iter().find(|b| b.name() == cl.name).unwrap();
                            let mut accepted = accepted(&files, &rule.id);
                            accepted.extend(common::accepted(&head, &rule.id));
                            let ctx = r.ctx(&rule.id, accepted.clone());
                            let report = b.run(&ctx, &args);
                            for f in &report.findings {
                                let text = f.agent_text();
                                let internals = forge_core::internals::find(&text);
                                assert!(internals.is_empty(), "{name}: {text}");
                                assert!(!text.contains("skipped"), "{name}: {text}");
                            }
                            found = report
                                .findings
                                .iter()
                                .map(|f| f.agent_text())
                                .filter(|t| !accepted.contains(t))
                                .collect();
                            if prisma {
                                // the pack command drops the lines forge.yaml accepts
                                let yaml = std::fs::read_to_string(r.root.join("forge.yaml"))
                                    .unwrap_or_default();
                                found.retain(|l| !yaml.contains(l.as_str()));
                            }
                            notes = report.notes.iter().map(|n| n.to_string()).collect();
                            // a lint check fails on what names the change: an added line, or
                            // a changed file as a whole (the checker's attribution)
                            if c.stage == forge_packs::model::Stage::Lint
                                && !report.findings.is_empty()
                                && report.findings.iter().any(|f| f.path.is_some())
                                && !report.findings.iter().any(|f| r.names_change(f))
                            {
                                found.clear();
                            }
                        }
                        ran += 1;
                        if want == found.is_empty() {
                            failures.push(format!(
                                "{name}: expected {}, got {found:?} (notes {notes:?})",
                                if want { "a finding" } else { "nothing" }
                            ));
                        }
                    }
                }
            }
        }
    }
    eprintln!("{ran} examples ran; skipped (no uvx): {skipped:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(ran >= 70, "only {ran} examples ran");
}
