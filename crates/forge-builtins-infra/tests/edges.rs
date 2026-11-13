//! What the pack examples do not show: a Dockerfile hadolint cannot parse, missing tools, tools
//! that cannot run, --all.

mod common;

use std::collections::BTreeMap;

use common::{Case, check, ctx, fake_bin, path_tools, repo};
use forge_builtins_infra::Options;
use forge_check::builtins::CheckCtx;
use forge_core::NoteKind;
use forge_core::builtin::Scope;

fn case(rule: &str, files: &[(&str, &str)]) -> Case {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
    let rule = set
        .packs
        .values()
        .flat_map(|p| p.rules.iter())
        .find(|r| r.id == rule)
        .expect("rule")
        .clone();
    Case {
        name: rule.id.clone(),
        rule,
        builtin: String::new(),
        args: vec![],
        when_changed: None,
        head: BTreeMap::new(),
        files: files
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        expect: true,
    }
}

fn opts(bin: &std::path::Path) -> Options {
    path_tools(bin.to_string_lossy().into_owned())
}

#[test]
fn a_dockerfile_hadolint_cannot_parse_is_reported_once_per_run_at_its_line() {
    let bin = fake_bin();
    let o = opts(bin.path());
    let c = case(
        "docker/workdir",
        &[("Dockerfile", "FROM python:3.13-slim\nRUNN x\n")],
    );
    let r = repo(&c);
    let tmp = tempfile::tempdir().unwrap();
    let extra = || CheckCtx {
        tmp: Some(tmp.path().to_path_buf()),
        ..check(None, vec!["Dockerfile".into()])
    };
    let args = ["DL3045".to_string()];
    let first = forge_builtins_infra::run(&o, "hadolint", &ctx(&c, &r), &extra(), &args).unwrap();
    assert_eq!(
        first.agent_text(10),
        "Dockerfile:2 this Dockerfile does not parse (column 4: missing whitespace); fix its syntax"
    );
    // the next rule of the same run: hadolint's output is reused, the error not repeated
    let second = forge_builtins_infra::run(&o, "hadolint", &ctx(&c, &r), &extra(), &args).unwrap();
    assert!(second.findings.is_empty() && second.notes.is_empty());
}

#[test]
fn a_missing_tool_is_a_note_and_never_a_finding() {
    let empty = tempfile::tempdir().unwrap();
    let o = opts(empty.path());
    let calls: &[(&str, &str, &str, &str)] = &[
        (
            "docker/pin-base-image",
            "hadolint",
            "Dockerfile",
            "FROM node\n",
        ),
        (
            "kubernetes/kube-linter",
            "kube_linter",
            "deploy/a.yaml",
            "apiVersion: v1\nkind: Service\n",
        ),
        (
            "terraform/tflint",
            "tflint",
            "main.tf",
            "variable \"x\" {}\n",
        ),
        (
            "terraform/validate",
            "tf_validate",
            "main.tf",
            "variable \"x\" {}\n",
        ),
        (
            "github-actions/zizmor",
            "zizmor",
            ".github/workflows/a.yml",
            "on: push\n",
        ),
        (
            "github-actions/actionlint",
            "actionlint",
            ".github/workflows/a.yml",
            "on: push\n",
        ),
        (
            "openapi/lint",
            "openapi_lint",
            "openapi.yaml",
            "openapi: 3.1.0\n",
        ),
    ];
    for (rule, b, path, text) in calls {
        let c = case(rule, &[(path, text)]);
        let r = repo(&c);
        let extra = check(None, vec![path.to_string()]);
        let rep = forge_builtins_infra::run(&o, b, &ctx(&c, &r), &extra, &[]).unwrap();
        assert!(rep.findings.is_empty(), "{b}: {:?}", rep.findings);
        assert!(
            rep.notes
                .iter()
                .any(|n| n.kind == NoteKind::MissingTool && n.rule.as_deref() == Some(*rule)),
            "{b}: {:?}",
            rep.notes
        );
    }
}

#[test]
fn a_tool_that_cannot_run_is_a_note() {
    let bin = fake_bin();
    let o = opts(bin.path());
    let c = case(
        "kubernetes/kube-linter",
        &[(
            "deploy/a.yaml",
            "apiVersion: v1\nkind: Service\nmetadata:\n  name: a\n",
        )],
    );
    let r = repo(&c);
    let extra = check(None, vec!["deploy/a.yaml".into()]);
    let rep = forge_builtins_infra::run(
        &o,
        "kube_linter",
        &ctx(&c, &r),
        &extra,
        &["no-such-check".into()],
    )
    .unwrap();
    assert!(rep.findings.is_empty());
    assert_eq!(rep.notes.len(), 1);
    assert_eq!(rep.notes[0].kind, NoteKind::ForgeFailure);
    assert!(
        rep.notes[0]
            .text
            .contains("check \"no-such-check\" not found"),
        "{:?}",
        rep.notes
    );
}

#[test]
fn comparisons_with_the_base_do_nothing_with_all() {
    let bin = fake_bin();
    let o = opts(bin.path());
    let c = case(
        "openapi/no-breaking",
        &[(
            "openapi.yaml",
            "openapi: 3.1.0\npaths:\n  /a:\n    get: {}\n",
        )],
    );
    let r = repo(&c);
    for b in ["oasdiff", "trivy_config", "openapi_sync", "chart_version"] {
        let mut ctx = ctx(&c, &r);
        ctx.scope = Scope::All;
        ctx.base = None;
        ctx.changed.clear();
        let rep = forge_builtins_infra::run(&o, b, &ctx, &CheckCtx::default(), &[]).unwrap();
        assert!(
            rep.findings.is_empty() && rep.notes.is_empty(),
            "{b}: {rep:?}"
        );
    }
}

/// trivy compares the directory with the base itself: its findings are marked as the change's,
/// so one in an unchanged file of the directory is not dropped.
#[test]
fn trivy_findings_are_marked_as_the_changes() {
    let bin = fake_bin();
    let o = opts(bin.path());
    let mut c = case(
        "terraform/misconfig",
        &[(
            "infra/network.tf",
            "resource \"aws_security_group\" \"web\" {\n  ingress {\n    from_port   = 22\n    cidr_blocks = [\"0.0.0.0/0\"]\n  }\n}\n",
        )],
    );
    c.head.insert(
        "infra/network.tf".into(),
        "resource \"aws_security_group\" \"web\" {\n}\n".into(),
    );
    let r = repo(&c);
    let tmp = tempfile::tempdir().unwrap();
    let attributed = tmp.path().join("attributed");
    let extra = CheckCtx {
        attributed: Some(attributed.clone()),
        tmp: Some(tmp.path().to_path_buf()),
        ..check(None, vec!["infra/network.tf".into()])
    };
    let rep = forge_builtins_infra::run(&o, "trivy_config", &ctx(&c, &r), &extra, &[]).unwrap();
    assert!(!rep.findings.is_empty(), "{rep:?}");
    assert!(attributed.exists());
}

/// A renamed workflow keeps its jobs; a deleted one loses them, each finding saying the user
/// may accept it (the hint is part of the finding: a line of its own names no location).
#[test]
fn a_renamed_workflow_removes_no_job() {
    let wf = "name: ci\non: [push, pull_request]\njobs:\n  lint:\n    runs-on: ubuntu-latest\n    steps:\n      - run: npm run lint\n";
    let bin = fake_bin();
    let o = opts(bin.path());
    for renamed in [true, false] {
        let mut files = vec![(".github/workflows/ci.yml", "gone\n")];
        if renamed {
            files.push((".github/workflows/checks.yml", wf));
        }
        let mut c = case("github-actions/no-removed-jobs", &files);
        c.head.insert(".github/workflows/ci.yml".into(), wf.into());
        let r = repo(&c);
        std::fs::remove_file(r.root.join(".github/workflows/ci.yml")).unwrap();
        let extra = check(None, vec![]);
        let rep = forge_builtins_infra::run(&o, "ci_jobs", &ctx(&c, &r), &extra, &[]).unwrap();
        if renamed {
            assert!(rep.findings.is_empty(), "{rep:?}");
        } else {
            assert_eq!(
                rep.agent_text(10),
                ".github/workflows/ci.yml:0 job lint was removed from a workflow that runs on push or pull_request. Restore it, or ask the user to accept this change."
            );
        }
    }
}

/// In a change, a per-file tool checks the check's files only: when only a `when_changed` file
/// changed there is nothing of its own to check, not every workflow of the repository.
#[test]
fn a_change_with_no_files_of_its_own_checks_nothing() {
    let bin = fake_bin();
    let o = opts(bin.path());
    let mut c = case(
        "github-actions/zizmor-advice",
        &[(".github/actions/setup/install.sh", "npm ci\n")],
    );
    c.head.insert(
        ".github/workflows/lint.yml".into(),
        "name: lint\non: push\npermissions: {}\njobs:\n  lint:\n    runs-on: ubuntu-latest\n    permissions: {}\n    steps:\n      - run: npm install -g prettier\n".into(),
    );
    let r = repo(&c);
    let args = ["adhoc-packages".to_string()];
    let rule = Some(c.rule.clone());
    let run = |files: Vec<String>| {
        let extra = check(rule.clone(), files);
        forge_builtins_infra::run(&o, "zizmor", &ctx(&c, &r), &extra, &args).unwrap()
    };
    let rep = run(vec![]);
    assert!(rep.findings.is_empty() && rep.notes.is_empty(), "{rep:?}");
    // the same workflow as a file of the change is checked
    let rep = run(vec![".github/workflows/lint.yml".into()]);
    assert!(!rep.findings.is_empty(), "{rep:?}");
}
