//! Every pack rule whose check calls one of this crate's builtins: its examples hold when the
//! builtin runs on them, in a fixture repository with the context the checker builds (a turn:
//! the changed files, their base copies, the added lines; lint attribution to added lines).
//!
//! A builtin that needs a pinned tool which is not installed writes a note: its examples are
//! counted as not run, never as passed. `FORGE_TEST_INSTALL_TOOLS=1` installs the pinned tools
//! into Forge's cache first.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{Repo, attribute, words};
use forge_builtins_code::{Options, PinnedTools, Provisioned, code_builtins};
use forge_core::{Note, NoteKind};
use forge_packs::model::{Example, Target};

/// The tools on PATH, then Forge's cache (installing when asked).
struct TestTools(Provisioned<forge_tools::Provisioner>);

impl PinnedTools for TestTools {
    fn pinned(&self, name: &str) -> Result<PathBuf, Note> {
        forge_builtins_code::OnPath::default()
            .pinned(name)
            .or_else(|_| self.0.pinned(name))
    }
}

pub fn options() -> Options {
    let home = forge_home::Home::from_env().expect("home");
    Options {
        tools: Arc::new(TestTools(Provisioned {
            resolver: forge_tools::Provisioner::new(home),
            install: std::env::var("FORGE_TEST_INSTALL_TOOLS").is_ok_and(|v| v == "1"),
        })),
        ..Options::default()
    }
}

#[test]
fn pack_examples_hold() {
    let opts = options();
    let names: Vec<&str> = code_builtins(&opts).iter().map(|b| b.name()).collect();
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
    let (mut rules, mut ran, mut not_run) = (0, 0, vec![]);
    let mut failures = vec![];
    let call = regex::Regex::new(r"forge_([a-z_]+)").unwrap();
    for rule in set.packs.values().flat_map(|p| p.rules.iter()) {
        for t in &rule.enforce {
            let Target::Check(c) = t else { continue };
            // the call of one of this crate's builtins, and whether advice wraps it
            let Some((name, args, advised)) = c.commands().find_map(|cmd| {
                call.find_iter(cmd).find_map(|m| {
                    let n = &cmd[m.start() + 6..m.end()];
                    names.contains(&n).then(|| {
                        let w = words(&cmd[m.end()..]);
                        let advised = cmd[..m.start()].contains("forge_advise");
                        (n.to_string(), w, advised)
                    })
                })
            }) else {
                continue;
            };
            rules += 1;
            let Some(ex) = &rule.examples else {
                failures.push(format!("{}: no examples", rule.id));
                continue;
            };
            for (want, list) in [(true, &ex.match_), (false, &ex.no_match)] {
                for (i, e) in list.iter().flatten().enumerate() {
                    let Example::Fixture(f) = e else { continue };
                    let head = f.head.clone().unwrap_or_default().into_iter().collect();
                    let files = f.files.clone().into_iter().collect();
                    let repo = Repo::new(&head, &files);
                    let ctx = repo.ctx(&rule.id);
                    let x = repo.extra(rule);
                    let report =
                        forge_builtins_code::run(&opts, &name, &ctx, &x, &args).expect("builtin");
                    let label = format!(
                        "{} {}[{i}]",
                        rule.id,
                        if want { "match" } else { "no_match" }
                    );
                    if report.notes.iter().any(|n| {
                        n.kind == NoteKind::MissingTool || n.kind == NoteKind::ForgeFailure
                    }) {
                        not_run.push(format!("{label}: {}", report.notes[0].text));
                        continue;
                    }
                    ran += 1;
                    let lines: Vec<String> =
                        report.findings.iter().map(|f| f.agent_text()).collect();
                    for l in &lines {
                        assert!(
                            forge_core::internals::find(l).is_empty(),
                            "{label}: internals in {l:?}"
                        );
                    }
                    let counted = if advised {
                        lines.clone()
                    } else {
                        attribute(&lines, &repo)
                    };
                    if counted.is_empty() == want {
                        failures.push(format!(
                            "{label}: {} (findings {lines:?}, notes {:?})",
                            if want { "nothing found" } else { "found" },
                            report.notes
                        ));
                    }
                }
            }
        }
    }
    eprintln!(
        "{rules} rules, {ran} examples run, {} not run:\n  {}",
        not_run.len(),
        not_run.join("\n  ")
    );
    assert!(rules >= 13, "{rules} rules use this crate's builtins");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
