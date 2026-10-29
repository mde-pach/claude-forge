//! What the pack examples do not show: a Dockerfile hadolint cannot parse, missing tools, tools
//! that cannot run, --all.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::{Case, DirTools, ctx, fake_bin, repo};
use forge_builtins_infra::{Extra, Options, PathTools};
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
    Options {
        pinned: Arc::new(DirTools(bin.to_path_buf())),
        project: Arc::new(PathTools {
            path: Some(bin.to_string_lossy().into_owned()),
        }),
        cache_dir: None,
    }
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
    let extra = || Extra {
        files: Some(vec!["Dockerfile".into()]),
        tmp: Some(tmp.path().to_path_buf()),
        ..Extra::default()
    };
    let args = ["DL3045".to_string()];
    let first = forge_builtins_infra::run(&o, "hadolint", &ctx(&c, &r), extra(), &args).unwrap();
    assert_eq!(
        first.agent_text(10),
        "Dockerfile:2 this Dockerfile does not parse (column 4: missing whitespace); fix its syntax"
    );
    // the next rule of the same run: hadolint's output is reused, the error not repeated
    let second = forge_builtins_infra::run(&o, "hadolint", &ctx(&c, &r), extra(), &args).unwrap();
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
        let extra = Extra {
            files: Some(vec![path.to_string()]),
            ..Extra::default()
        };
        let rep = forge_builtins_infra::run(&o, b, &ctx(&c, &r), extra, &[]).unwrap();
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
    let extra = Extra {
        files: Some(vec!["deploy/a.yaml".into()]),
        ..Extra::default()
    };
    let rep = forge_builtins_infra::run(
        &o,
        "kube_linter",
        &ctx(&c, &r),
        extra,
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
        let rep = forge_builtins_infra::run(&o, b, &ctx, Extra::default(), &[]).unwrap();
        assert!(
            rep.findings.is_empty() && rep.notes.is_empty(),
            "{b}: {rep:?}"
        );
    }
}
