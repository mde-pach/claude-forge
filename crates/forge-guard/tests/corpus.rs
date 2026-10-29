//! §14.3, the guard corpus: all guard rules of all packs together, against commands that must
//! and must not trigger, run from a repository whose `node_modules/.bin` holds `tsc` and
//! `vitest` and that has a `packages/app` directory. Plus the false positives of matching the
//! raw text (prose in arguments), and properties: quoted or heredoc text never triggers a rule.

mod common;

use common::*;
use forge_guard::{GuardSet, Outcome, PythonPm};

include!("common/corpus_data.rs");

fn decision(set: &GuardSet, cmd: &str, root: &std::path::Path) -> &'static str {
    set.evaluate(cmd, &ctx(root)).decision()
}

fn check_corpus(set: &GuardSet, extra_silent: &[&str]) -> Vec<String> {
    let project = corpus_repo();
    let mut failures = Vec::new();
    for (cmd, want) in MUST_TRIGGER {
        let got = decision(set, cmd, project.path());
        if got != *want {
            failures.push(format!("{cmd:?}: want {want}, got {got:?}"));
        }
    }
    for cmd in MUST_NOT_TRIGGER.iter().chain(extra_silent) {
        let out = set.evaluate(cmd, &ctx(project.path()));
        if out != Outcome::None {
            failures.push(format!(
                "{cmd:?}: want nothing, got {} ({})",
                out.decision(),
                out.verdict().map(|v| v.rule.as_str()).unwrap_or("")
            ));
        }
    }
    failures
}

#[test]
fn corpus_with_the_converted_rules() {
    let failures = check_corpus(&converted_set(), FALSE_POSITIVES);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn corpus_with_the_pack_patterns_per_simple_command() {
    // the pack patterns keep the corpus; the false positives below are those still left
    // (a pattern reading flags anywhere in its command: pip install --dry-run)
    let failures = check_corpus(&legacy_set(), &[]);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn prose_left_to_the_pack_patterns() {
    let set = legacy_set();
    let project = corpus_repo();
    let left: Vec<&str> = FALSE_POSITIVES
        .iter()
        .copied()
        .filter(|c| !decision(&set, c, project.path()).is_empty())
        .collect();
    // matching per simple command already fixes most; these need the structural form: a
    // flag read anywhere in a command (pip's --dry-run is not seen), a flag pattern with no
    // command (`rg -- --env-mode=loose`), `[^;&|]*` running into a heredoc body
    assert_eq!(
        left,
        [
            "pip install --dry-run -r requirements.txt",
            "pip install --dry-run requests",
            "uv pip install --dry-run httpx",
            "rg -- --env-mode=loose src",
            "cat > notes.md <<'EOF'\ngit push --force\nrm -rf /\ncat .env\nkubectl delete ns prod\nEOF",
        ]
    );
}

/// Quoted text and heredoc bodies are data: a command that triggers on its own triggers
/// nothing as the argument of another command.
#[test]
fn quoted_and_heredoc_text_never_triggers() {
    let set = converted_set();
    let project = corpus_repo();
    let mut failures = Vec::new();
    for (cmd, _) in MUST_TRIGGER {
        let mut wrapped = vec![
            format!("cat > notes.md <<'EOF'\n{cmd}\nEOF"),
            format!("cat <<EOF | tee notes.md\n{cmd}\nEOF"),
            format!("git commit -F - <<'EOF'\nfix\n\n{cmd}\nEOF"),
        ];
        if !cmd.contains('\'') {
            wrapped.push(format!("echo '{cmd}'"));
            wrapped.push(format!("git commit -m '{cmd}'"));
            wrapped.push(format!("gh pr create --body 'Before: {cmd}'"));
            wrapped.push(format!("grep -rn -- '{cmd}' docs"));
        }
        if !cmd.contains('"') && !cmd.contains('$') {
            wrapped.push(format!("printf '%s\\n' \"{cmd}\" >> notes.md"));
        }
        for w in wrapped {
            let out = set.evaluate(&w, &ctx(project.path()));
            if out != Outcome::None {
                failures.push(format!(
                    "{w:?}: {} ({})",
                    out.decision(),
                    out.verdict().map(|v| v.rule.as_str()).unwrap_or("")
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// ...while a shell fed the same text runs it.
#[test]
fn text_fed_to_a_shell_is_parsed() {
    let set = converted_set();
    let project = corpus_repo();
    for cmd in [
        "sh <<'EOF'\ngit push --force\nEOF",
        "true && sh <<'EOF'\ngit push --force\nEOF",
        "cat <<'EOF' | bash\ngit push --force\nEOF",
        "bash -c 'git push --force'",
        "echo 'git push --force' | sh",
        "bash <<< 'git push --force'",
        "sudo sh -c 'cd /tmp && git push --force'",
        "x=$(git push --force)",
        "(cd sub; git push --force)",
        "if true; then git push --force; fi",
        "for b in a; do git push --force origin $b; done",
        "su -c 'git push --force'",
    ] {
        assert_eq!(decision(&set, cmd, project.path()), "deny", "{cmd}");
    }
}

#[test]
fn reasons_are_neutral() {
    let set = converted_set();
    let project = corpus_repo();
    for (cmd, _) in MUST_TRIGGER {
        let out = set.evaluate(cmd, &ctx(project.path()));
        let v = out.verdict().expect("a verdict");
        assert!(!v.reason.contains(&v.rule), "{cmd}: {}", v.reason);
        assert!(!v.reason.contains("forge"), "{cmd}: {}", v.reason);
        assert!(
            forge_core::internals::find(&v.reason).is_empty(),
            "{cmd}: {}",
            v.reason
        );
    }
    // every description and target reason of the converted rules
    for r in set.rules() {
        assert!(
            forge_core::internals::find(&r.description).is_empty(),
            "{}: {}",
            r.id,
            r.description
        );
        for t in &r.targets {
            if let Some(reason) = &t.reason {
                assert!(forge_core::internals::find(reason).is_empty(), "{}", r.id);
            }
        }
    }
}

#[test]
fn check_findings_in_reasons() {
    let set = converted_set();
    let project = corpus_repo();
    let reason = |cmd: &str| {
        set.evaluate(cmd, &ctx(project.path()))
            .verdict()
            .map(|v| v.reason.clone())
            .unwrap_or_default()
    };
    assert!(
        reason("npx some-unknown-tool").ends_with(
            "Here: some-unknown-tool is not installed where the command runs (no node_modules/.bin/some-unknown-tool in the repository root), so npx would fetch it from a registry and run it."
        ),
        "{}",
        reason("npx some-unknown-tool")
    );
    assert!(reason("N=npx; $N cowsay").contains("Here: the command name $N is expanded"));
    assert!(reason("uv add requests flask").ends_with("Here: it adds requests and flask."));
    assert!(reason("cd packages/app && npx cowsay").contains("from packages/app up to"));
}

#[test]
fn pip_in_a_uv_project_is_denied() {
    let set = converted_set();
    let project = repo(&[("pyproject.toml", "[project]\nname='x'\n"), ("uv.lock", "")]);
    let uv = |cmd: &str| set.evaluate(cmd, &ctx(project.path()).with_python_pm(Some(PythonPm::Uv)));
    let deny = |cmd: &str| match uv(cmd) {
        Outcome::Deny(v) => v.reason,
        other => panic!("{cmd}: {other:?}"),
    };
    assert_eq!(
        deny("pip install requests"),
        "This project uses uv: `uv add <pkg>` (`--dev` for tooling). Here: `uv add requests`."
    );
    assert_eq!(
        deny("python3 -m pip install -r requirements.txt"),
        "This project uses uv: `uv add <pkg>` (`--dev` for tooling). Here: `uv add -r requirements.txt`."
    );
    assert!(deny("uv pip install 'httpx>=0.27'").ends_with("Here: `uv add 'httpx>=0.27'`."));
    assert!(deny("pip install -e .").ends_with("Here: `uv sync` installs the project itself."));
    assert!(deny(".venv/bin/pip install rich").contains("`uv add rich`"));
    // read-only pip, and pip into another environment
    for cmd in [
        "pip show requests",
        "pip list",
        "pip freeze",
        "pip download requests",
        "pip install --dry-run requests",
        "uv pip list",
        "uv pip tree",
    ] {
        assert_eq!(uv(cmd), Outcome::None, "{cmd}");
    }
    for cmd in [
        "pip install --target /tmp/vendor requests",
        "/tmp/venv/bin/pip install requests",
        "uv pip install --python /opt/tools/bin/python requests",
        "VIRTUAL_ENV=/opt/tools uv pip install requests",
        "cd /tmp && pip install requests",
    ] {
        assert_eq!(uv(cmd).decision(), "ask", "{cmd}");
    }
    // a plain-scripts project
    let scripts = set.evaluate(
        "pip install requests",
        &ctx(project.path()).with_python_pm(Some(PythonPm::Scripts)),
    );
    assert_eq!(
        scripts.verdict().map(|v| v.reason.as_str()),
        Some(
            "This project has no Python packaging yet: create it with `uv init --bare`, then `uv add <pkg>` (`--dev` for tooling). Here: `uv init --bare && uv add requests`."
        )
    );
    // a Poetry project: pip asks as a package addition, it is not denied
    let poetry = set.evaluate(
        "pip install requests",
        &ctx(project.path()).with_python_pm(Some(PythonPm::Poetry)),
    );
    assert_eq!(poetry.decision(), "ask");
}

#[test]
fn unparseable_input_is_handled_conservatively() {
    let set = converted_set();
    let project = corpus_repo();
    // an unterminated quote: the tokenizer still sees the push
    assert_eq!(
        decision(&set, "git push --force; echo 'unterminated", project.path()),
        "deny"
    );
    for junk in [
        "", ")", "((", "'", "\"", "<<", "|||", "\\", "$(", "`", "\0", "{ ; }",
    ] {
        let _ = set.evaluate(junk, &ctx(project.path()));
    }
    let deep = "sh -c ".repeat(50) + "'git push --force'";
    let _ = set.evaluate(&deep, &ctx(project.path()));
}
