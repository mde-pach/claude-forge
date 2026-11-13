//! §15.2, §15.3: which rules get a types engine, and how `forge.yaml`'s `python:
//! {untyped-packages}` configures it. A fake ty (a shell script) stands in for the pinned one.

use std::path::{Path, PathBuf};

use forge_check::ActiveRule;
use forge_check::engine::{config, engine_languages};
use forge_core::tools::{ToolCmd, ToolResolver};
use forge_engine::Lang;
use forge_packs::model::Rule;
use serde_json::json;

struct FakeTy(PathBuf);

impl ToolResolver for FakeTy {
    fn pinned(&self, _name: &str, _version: &str) -> Result<ToolCmd, String> {
        Ok(ToolCmd::exe(self.0.clone()))
    }
}

fn rule(v: serde_json::Value) -> ActiveRule {
    ActiveRule::new(serde_json::from_value::<Rule>(v).expect("rule"))
}

fn typecheck_only() -> Vec<ActiveRule> {
    vec![rule(json!({
        "id": "python/typecheck", "description": "The project passes ty.",
        "scope": {"language": "python"},
        "enforce": [{"target": "check", "stage": "typecheck", "run": "true"}],
    }))]
}

fn put(root: &Path, path: &str, text: &str) {
    let f = root.join(path);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

/// A project with a `.venv` holding one installed package without typing, and a fake ty that
/// records each run in `<root>/../ty-ran`.
fn project() -> (tempfile::TempDir, PathBuf, FakeTy) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("repo");
    put(&root, "app.py", "x = 1\n");
    put(&root, ".venv/bin/python", "");
    put(
        &root,
        ".venv/lib/python3.12/site-packages/magic/__init__.py",
        "",
    );
    let ty = d.path().join("ty");
    let ran = d.path().join("ty-ran");
    std::fs::write(
        &ty,
        format!("#!/bin/sh\necho run >> '{}'\nexit 0\n", ran.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ty, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (d, root, FakeTy(ty))
}

#[test]
fn engines_for_type_checks_and_untyped_packages_from_forge_yaml() {
    // SAFETY: this test binary has this one test; no other thread reads the environment.
    unsafe { std::env::remove_var("VIRTUAL_ENV") };
    let rules = typecheck_only();
    assert!(engine_languages(&rules).contains(&Lang::Python));

    // default (`any`): the untyped packages are listed (ty's probe runs) and given to the engine
    let (d, root, ty) = project();
    let (cfg, notes) = config(&root, &rules, &ty, false, "0.0.0");
    let py = cfg.expect("an engine for a type-checked language").python;
    let py = py.expect("Python's engine");
    assert_eq!(py.untyped, vec!["magic".to_string()]);
    assert!(notes.is_empty(), "{notes:?}");
    assert!(d.path().join("ty-ran").is_file());

    // `source`: ty infers them from their source; nothing is listed, the probe never runs
    let (d, root, ty) = project();
    put(
        &root,
        "forge.yaml",
        "forge: \"0.6\"\npython:\n  untyped-packages: source\n",
    );
    let (cfg, notes) = config(&root, &rules, &ty, false, "0.0.0");
    let py = cfg.unwrap().python.unwrap();
    assert!(py.untyped.is_empty() && py.untyped_all.is_none(), "{py:?}");
    assert!(notes.is_empty(), "{notes:?}");
    assert!(!d.path().join("ty-ran").exists());

    // `any`, said explicitly, is the default
    let (_d, root, ty) = project();
    put(
        &root,
        "forge.yaml",
        "forge: \"0.6\"\npython:\n  untyped-packages: any\n",
    );
    let (cfg, _) = config(&root, &rules, &ty, false, "0.0.0");
    assert_eq!(
        cfg.unwrap().python.unwrap().untyped,
        vec!["magic".to_string()]
    );

    // a TypeScript typecheck in a project without node_modules/typescript: the check stage
    // still runs; only the edit-time diagnostics are lost, which is no rule left unchecked
    let rules = vec![rule(json!({
        "id": "typescript/typecheck", "description": "The project passes tsc.",
        "scope": {"language": "typescript"},
        "enforce": [{"target": "check", "stage": "typecheck", "run": "true"}],
    }))];
    let d = tempfile::tempdir().unwrap();
    let (cfg, notes) = config(d.path(), &rules, &FakeTy("/bin/false".into()), false, "0");
    assert!(cfg.is_none());
    assert!(notes.is_empty(), "{notes:?}");
}
