//! Context items: `when` (deps and versions, files, facts, packs), dedupe by id, `paths:`
//! frontmatter for a pack placed in some workspace packages only.

use std::collections::BTreeMap;

use forge_detect::{Activation, PackStatus, read_project_facts};
use forge_packs::model::{DepSpec, Ecosystem, OneOrMany};
use forge_render::context::{
    CONTEXT_INTRO, ContextItem, ContextWhen, PackContext, render_rules_files,
};

fn item(id: Option<&str>, text: &str, when: Option<ContextWhen>) -> ContextItem {
    ContextItem {
        id: id.map(str::to_owned),
        text: text.into(),
        when,
    }
}

#[test]
fn when_filters_items() {
    let t = tempfile::tempdir().unwrap();
    let files: Vec<String> = ["pyproject.toml", "uv.lock", "app/main.py"]
        .map(String::from)
        .to_vec();
    std::fs::create_dir_all(t.path().join("app")).unwrap();
    std::fs::write(
        t.path().join("pyproject.toml"),
        "[project]\nname = \"x\"\ndependencies = [\"pydantic>=2.5\"]\n",
    )
    .unwrap();
    std::fs::write(
        t.path().join("uv.lock"),
        "[[package]]\nname = \"pydantic\"\nversion = \"2.9.1\"\n",
    )
    .unwrap();
    std::fs::write(t.path().join("app/main.py"), "").unwrap();
    let facts = read_project_facts(t.path(), &files).unwrap();
    assert_eq!(facts.root.fact("python.pm"), Some("uv"));
    let active = Activation {
        packs: vec![PackStatus {
            id: "python".into(),
            why: "detected".into(),
        }],
        ..Activation::default()
    };
    let dep = |v: &str| DepSpec {
        ecosystem: Ecosystem::Pypi,
        name: "pydantic".into(),
        version: Some(v.into()),
    };
    let pm = |v: &str| {
        Some(ContextWhen {
            facts: BTreeMap::from([("python.pm".to_string(), OneOrMany::One(v.to_string()))]),
            ..ContextWhen::default()
        })
    };
    let items = vec![
        item(None, "always\n", None),
        item(None, "uv project\n", pm("uv")),
        item(None, "pip project\n", pm("pip")),
        item(
            None,
            "pydantic 2\n",
            Some(ContextWhen {
                deps: vec![dep(">=2")],
                ..Default::default()
            }),
        ),
        item(
            None,
            "pydantic 1\n",
            Some(ContextWhen {
                deps: vec![dep("<2")],
                ..Default::default()
            }),
        ),
        item(
            None,
            "has app\n",
            Some(ContextWhen {
                files: vec!["app/**".into()],
                ..Default::default()
            }),
        ),
        item(
            None,
            "has docs\n",
            Some(ContextWhen {
                files: vec!["docs/**".into()],
                ..Default::default()
            }),
        ),
        item(
            None,
            "with python\n",
            Some(ContextWhen {
                packs: vec!["python".into()],
                ..Default::default()
            }),
        ),
        item(
            None,
            "with django\n",
            Some(ContextWhen {
                packs: vec!["python/django".into()],
                ..Default::default()
            }),
        ),
        item(Some("tests"), "shared norm\n", None),
    ];
    let other = vec![
        item(Some("tests"), "shared norm\n", None),
        item(None, "own\n", None),
    ];
    let out = render_rules_files(
        &[
            PackContext {
                id: "python",
                items,
                dirs: None,
                places: None,
            },
            PackContext {
                id: "python/uv",
                items: other,
                dirs: None,
                places: None,
            },
        ],
        Some(&facts),
        &active,
    );
    assert_eq!(
        out[".claude/rules/forge-python.md"],
        format!(
            "{CONTEXT_INTRO}always\nuv project\npydantic 2\nhas app\nwith python\nshared norm\n"
        )
    );
    assert_eq!(
        out[".claude/rules/forge-python-uv.md"],
        format!("{CONTEXT_INTRO}own\n")
    );
}

#[test]
fn a_pack_in_some_packages_gets_paths() {
    let t = tempfile::tempdir().unwrap();
    let files: Vec<String> = vec![];
    let facts = read_project_facts(t.path(), &files).unwrap();
    let out = render_rules_files(
        &[PackContext {
            id: "typescript",
            items: vec![item(None, "x\n", None)],
            dirs: Some(vec!["packages/core".into()]),
            places: Some(vec!["packages/core".into()]),
        }],
        Some(&facts),
        &Activation::default(),
    );
    assert_eq!(
        out[".claude/rules/forge-typescript.md"],
        format!("---\npaths:\n  - \"packages/core/**\"\n---\n\n{CONTEXT_INTRO}x\n")
    );
}
