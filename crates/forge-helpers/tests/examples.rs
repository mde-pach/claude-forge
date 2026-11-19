//! Every pack example of a rule that runs a helper (`forge_helper`, `forge_ratchet`), run through
//! the helper's Rust port: a `match` example must give findings, a `no_match` example none.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use forge_core::builtin::{BuiltinCtx, Scope};
use forge_helpers::{Options, builtins, name_for_script};
use forge_packs::model::{Example, Target};

const COOLDOWN: &str = "259200";

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git")
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn write(dir: &Path, files: &BTreeMap<String, String>) {
    for (p, text) in files {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// One helper call on one fixture.
#[derive(Debug, Clone)]
struct Case {
    name: String,
    rule: String,
    script: String,
    args: Vec<String>,
    head: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
    /// Some(true): must find something; Some(false): must find nothing.
    expect: Option<bool>,
}

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: PathBuf,
    changed: Vec<String>,
}

fn repo(case: &Case) -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    let mut head = case.head.clone();
    head.insert(".gitignore".into(), "node_modules/\n.venv/\n".into());
    write(&root, &head);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "head"]);
    write(&root, &case.files);
    let changed: Vec<String> = case
        .files
        .iter()
        .filter(|(p, t)| head.get(*p) != Some(*t))
        .map(|(p, _)| p.clone())
        .collect();
    // the base: the changed files as they are at HEAD
    let base = dir.path().join("base");
    std::fs::create_dir_all(&base).unwrap();
    for p in &changed {
        if let Some(t) = head.get(p) {
            let b = base.join(p);
            std::fs::create_dir_all(b.parent().unwrap()).unwrap();
            std::fs::write(b, t).unwrap();
        }
    }
    Repo {
        _dir: dir,
        root,
        base,
        changed,
    }
}

/// The accepted changes of forge.yaml for the rule.
fn accepted(case: &Case) -> Vec<String> {
    let Some(text) = case.files.get("forge.yaml") else {
        return vec![];
    };
    let v = forge_packs::yaml::parse_strict(text).unwrap().value;
    v.get("accept")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter(|e| e.get("rule").and_then(|r| r.as_str()) == Some(case.rule.as_str()))
        .filter_map(|e| e.get("change").and_then(|c| c.as_str()).map(str::to_owned))
        .collect()
}

fn port(case: &Case, r: &Repo) -> (Vec<String>, Vec<String>) {
    let name = name_for_script(&case.script).expect("a helper builtin");
    let all = builtins(&Options::default());
    let b = all.iter().find(|b| b.name() == name).expect("builtin");
    let ctx = BuiltinCtx {
        root: r.root.clone(),
        base: Some(r.base.clone()),
        changed: r.changed.clone(),
        rule: case.rule.clone(),
        scope: Scope::Changed,
        accepted: accepted(case),
        notes_file: None,
    };
    let mut args = vec![case.script.clone()];
    args.extend(case.args.iter().cloned());
    let report = forge_helpers::run(b.as_ref(), &ctx, &args);
    let lines: Vec<String> = report
        .findings
        .iter()
        .map(|f| f.agent_text())
        .filter(|l| l != forge_helpers::ACCEPT_HINT)
        .collect();
    (lines, report.notes.iter().map(|n| n.to_string()).collect())
}

fn helper_call(run: &str) -> Option<(String, String, Vec<String>)> {
    let re = regex::Regex::new(r"forge_(?:helper|ratchet) (\S+) (\S+)((?: [^|;&{}]+)?)").unwrap();
    let c = re.captures(run)?;
    let args = c[3]
        .split_whitespace()
        .map(|a| a.replace("$FORGE_COOLDOWN_SECONDS", COOLDOWN))
        .collect();
    Some((c[1].to_owned(), c[2].to_owned(), args))
}

fn pack_cases() -> Vec<Case> {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
    let mut out = vec![];
    for pack in set.packs.values() {
        for rule in &pack.rules {
            for t in &rule.enforce {
                let Target::Check(c) = t else { continue };
                let Some((_runtime, script, args)) = helper_call(&c.run) else {
                    continue;
                };
                let Some(ex) = &rule.examples else { continue };
                let kinds = [(true, &ex.match_), (false, &ex.no_match)];
                for (want, list) in kinds {
                    for (i, e) in list.iter().flatten().enumerate() {
                        let Example::Fixture(f) = e else { continue };
                        let mut args = args.clone();
                        // changed_run passes the changed files ({files})
                        if c.changed_run
                            .as_deref()
                            .is_some_and(|r| r.contains("{files}"))
                        {
                            let head = f.head.clone().unwrap_or_default();
                            args.extend(
                                f.files
                                    .iter()
                                    .filter(|(p, t)| head.get(*p) != Some(*t))
                                    .map(|(p, _)| p.clone()),
                            );
                        }
                        out.push(Case {
                            name: format!(
                                "{} {}[{i}]",
                                rule.id,
                                if want { "match" } else { "no_match" }
                            ),
                            rule: rule.id.clone(),
                            script: script.clone(),
                            args,
                            head: f.head.clone().unwrap_or_default().into_iter().collect(),
                            files: f.files.clone().into_iter().collect(),
                            // a run that decides more than the helper (a database dialect) is
                            // the checker's to test
                            expect: (!c.run.contains("forge_db_url")).then_some(want),
                        });
                    }
                }
            }
        }
    }
    out
}

#[test]
fn ports_behave_as_the_pack_examples_say() {
    let cases = pack_cases();
    assert!(cases.len() >= 40, "pack examples found: {}", cases.len());
    let mut failures = vec![];
    for case in &cases {
        let Some(want) = case.expect else { continue };
        let r = repo(case);
        let (port_lines, notes) = port(case, &r);
        // a registry lookup that could not run (offline) leaves a note instead of findings
        let networked = case.script.starts_with("new_deps") && !notes.is_empty();
        if !networked && want == port_lines.is_empty() {
            failures.push(format!(
                "{}: expected {} findings, port gave {port_lines:?} (notes {notes:?})",
                case.name,
                if want { "some" } else { "no" }
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
