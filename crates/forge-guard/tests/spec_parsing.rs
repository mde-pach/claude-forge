//! §9.1 command parsing, as the guard sees it: nested shells (`sh|bash|zsh|dash|ksh -c`), the
//! directory `cd` / `pushd` / `env -C` move to, what `popd` and `cd -` leave unknown, and
//! `xargs`. Run with every pack's guard rules, from the corpus repository (§14.3).

mod common;

use common::*;
use forge_guard::parse::{self, Dir, Start};

fn verdict(cmd: &str) -> (String, String) {
    let set = guard_set();
    let project = corpus_repo();
    let out = set.evaluate(cmd, &ctx(project.path()));
    (
        out.decision().to_string(),
        out.verdict().map(|v| v.reason.clone()).unwrap_or_default(),
    )
}

#[test]
fn every_shell_dash_c_is_parsed() {
    for sh in ["sh", "bash", "zsh", "dash", "ksh"] {
        assert_eq!(
            verdict(&format!("{sh} -c 'git push --force'")).0,
            "deny",
            "{sh}"
        );
        assert_eq!(verdict(&format!("{sh} -c 'npx cowsay'")).0, "ask", "{sh}");
        // a tool installed in the repository runs, whatever shell runs it
        assert_eq!(
            verdict(&format!("{sh} -c 'npx tsc --noEmit'")).0,
            "",
            "{sh}"
        );
    }
    // nested twice
    assert_eq!(verdict("bash -c \"zsh -c 'git push -f'\"").0, "deny");
}

#[test]
fn pushd_moves_the_directory_like_cd() {
    let (d, reason) = verdict("pushd packages/app && npx cowsay");
    assert_eq!(d, "ask");
    assert!(reason.contains("from packages/app up to"), "{reason}");
    // vitest is installed at the root, which the lookup reaches from packages/app
    assert_eq!(verdict("pushd packages/app && npx vitest run").0, "");
    let (d, reason) = verdict("env -C packages/app npx cowsay");
    assert_eq!(d, "ask");
    assert!(reason.contains("from packages/app up to"), "{reason}");
}

#[test]
fn popd_and_cd_dash_leave_the_directory_unknown() {
    for cmd in [
        "pushd packages/app && popd && npx tsc --noEmit",
        "cd - && npx tsc --noEmit",
        "cd \"$DIR\" && npx tsc --noEmit",
    ] {
        let (d, reason) = verdict(cmd);
        assert_eq!(d, "ask", "{cmd}");
        assert!(reason.contains("not known in advance"), "{cmd}: {reason}");
    }
    let p = parse::parse(
        "pushd a; popd; ls",
        &Start {
            cwd: Dir::Known("/r".into()),
            home: None,
        },
    );
    assert_eq!(p.commands.last().unwrap().dir, Dir::Unknown);
}

#[test]
fn xargs_runs_its_command() {
    assert_eq!(verdict("ls | xargs npx cowsay").0, "ask");
    assert_eq!(verdict("ls | xargs -n 1 npx tsc --noEmit").0, "");
    assert_eq!(
        verdict("echo x | xargs -I{} git push --force origin {}").0,
        "deny"
    );
}
