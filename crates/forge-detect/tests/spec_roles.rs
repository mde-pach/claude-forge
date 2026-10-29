//! §15.4 File roles: Forge's defaults, the tool's declarative config, forge.yaml, and roles in
//! rules. Hermetic: temporary repositories, no tools run.

use std::path::Path;

use forge_detect::roles::{RoleInputs, Roles, apply_roles, expand_globs, resolve_roles};
use forge_packs::model::{RoleChange, Rule, Target};
use forge_packs::roles::roles_of_rule;
use indexmap::IndexMap;
use serde_json::json;

/// The roles of a repository holding `files` (path, content).
fn roles_of(files: &[(&str, &str)], config: Option<&IndexMap<String, RoleChange>>) -> Roles {
    let t = tempfile::tempdir().unwrap();
    let mut list = Vec::new();
    for (p, text) in files {
        let path = t.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        list.push((*p).to_string());
    }
    resolve_roles(
        Some(&RoleInputs {
            repo: t.path(),
            files: &list,
        }),
        config,
    )
    .unwrap()
}

fn globs(roles: &Roles, name: &str) -> Vec<String> {
    roles[name].globs()
}

fn sources(roles: &Roles, name: &str) -> Vec<String> {
    roles[name]
        .patterns
        .iter()
        .map(|p| p.source.clone())
        .collect()
}

#[test]
fn pytest_testpaths_and_python_files_replace_the_defaults() {
    let r = roles_of(
        &[(
            "pyproject.toml",
            "[tool.pytest.ini_options]\ntestpaths = [\"tests\", \"integration\"]\npython_files = [\"check_*.py\"]\n",
        )],
        None,
    );
    assert_eq!(
        globs(&r, "python-tests"),
        [
            "tests/**/check_*.py",
            "integration/**/check_*.py",
            "**/conftest.py"
        ]
    );
    assert_eq!(sources(&r, "python-tests")[0], "pyproject.toml");
    // pytest.ini wins over pyproject.toml; testpaths alone keeps pytest's file names
    let r = roles_of(
        &[
            ("pytest.ini", "[pytest]\ntestpaths = t\n"),
            ("pyproject.toml", "[tool.pytest.ini_options]\ntestpaths = [\"x\"]\n"),
        ],
        None,
    );
    assert_eq!(
        globs(&r, "python-tests"),
        ["t/**/test_*.py", "t/**/*_test.py", "**/conftest.py"]
    );
}

#[test]
fn alembic_config_moves_the_migrations_and_a_package_resource_is_a_note() {
    let r = roles_of(
        &[(
            "alembic.ini",
            "[alembic]\nscript_location = db\nversion_locations = db/a db/b\n",
        )],
        None,
    );
    assert_eq!(globs(&r, "alembic-migrations"), ["db/a/**", "db/b/**"]);
    let r = roles_of(&[("alembic.ini", "[alembic]\nscript_location = app:migrations\n")], None);
    assert_eq!(
        globs(&r, "alembic-migrations"),
        ["alembic/versions/**", "migrations/versions/**"],
        "a package resource leaves Forge's defaults"
    );
    assert!(
        r["alembic-migrations"].notes[0].contains("is not a path in the repository"),
        "{:?}",
        r["alembic-migrations"].notes
    );
    let r = roles_of(
        &[("pyproject.toml", "[tool.alembic]\nscript_location = \"migrations\"\n")],
        None,
    );
    assert_eq!(globs(&r, "alembic-migrations"), ["migrations/versions/**"]);
}

#[test]
fn prisma_generator_outputs_and_package_json_schema() {
    let r = roles_of(
        &[(
            "prisma/schema.prisma",
            "generator client {\n  provider = \"prisma-client\"\n  output = \"../src/generated/prisma\"\n}\n",
        )],
        None,
    );
    assert_eq!(globs(&r, "generated-clients"), ["src/generated/prisma/**"]);
    assert_eq!(
        globs(&r, "prisma-migrations"),
        ["prisma/migrations/**"],
        "the default schema place keeps the default migrations"
    );
    let r = roles_of(
        &[
            ("package.json", r#"{"prisma": {"schema": "db/schema.prisma"}}"#),
            ("db/schema.prisma", "datasource db {\n  provider = \"postgresql\"\n}\n"),
        ],
        None,
    );
    assert_eq!(globs(&r, "prisma-migrations"), ["db/migrations/**"]);
    assert!(globs(&r, "generated-clients").is_empty());
}

#[test]
fn angular_django_and_adr_configs() {
    let r = roles_of(
        &[(
            "angular.json",
            r#"{"projects": {"a": {"architect": {"build": {"options": {"tsConfig": "apps/a/tsconfig.app.json"}}}}, "b": {"targets": {"build": {"options": {"tsConfig": "apps/b/tsconfig.json"}}}}}}"#,
        )],
        None,
    );
    assert_eq!(
        globs(&r, "angular-tsconfig"),
        ["apps/a/tsconfig.app.json", "apps/b/tsconfig.json"]
    );
    assert_eq!(
        globs(&roles_of(&[], None), "angular-tsconfig"),
        ["**/tsconfig.app.json"]
    );
    let r = roles_of(
        &[
            ("pytest.ini", "[pytest]\nDJANGO_SETTINGS_MODULE = conf.settings\n"),
            (".adr-dir", "architecture/decisions\n"),
        ],
        None,
    );
    assert_eq!(
        globs(&r, "django-settings"),
        ["conf/settings.py", "conf/settings/*.py"]
    );
    assert_eq!(globs(&r, "adr"), ["architecture/decisions/**"]);
}

#[test]
fn sql_migrations_per_tool() {
    let d = roles_of(&[], None);
    assert_eq!(
        globs(&d, "sql-migrations"),
        [
            "db/migration/**/*.sql",
            "supabase/migrations/*.sql",
            "db/migrations/*.sql"
        ]
    );
    let r = roles_of(
        &[
            (
                "flyway.conf",
                "flyway.locations=filesystem:sql/versioned,classpath:db/migration\n",
            ),
            ("sqitch.plan", "%project=x\n"),
            ("sqitch.conf", "[core]\n\ttop_dir = migrations\n"),
            ("atlas.hcl", "env \"dev\" {\n  migration {\n    dir = \"file://schema/migrations\"\n  }\n}\n"),
        ],
        None,
    );
    let g = globs(&r, "sql-migrations");
    assert_eq!(
        g,
        [
            "sql/versioned/**/*.sql",
            "supabase/migrations/*.sql",
            "db/migrations/*.sql",
            "migrations/deploy/**/*.sql",
            "schema/migrations/*.sql"
        ]
    );
    assert!(
        r["sql-migrations"]
            .notes
            .iter()
            .any(|n| n.contains("classpath:db/migration")),
        "a classpath location is reported: {:?}",
        r["sql-migrations"].notes
    );
    // Atlas without a dir: its default
    let r = roles_of(&[("atlas.hcl", "env \"dev\" {}\n")], None);
    assert!(globs(&r, "sql-migrations").contains(&"migrations/*.sql".to_string()));
}

#[test]
fn esm_modules_follow_package_json_type() {
    let r = roles_of(
        &[
            ("package.json", r#"{"type": "module"}"#),
            ("legacy/package.json", r#"{"name": "legacy"}"#),
            ("node_modules/x/package.json", r#"{"type": "module"}"#),
        ],
        None,
    );
    assert_eq!(
        globs(&r, "esm-modules"),
        [
            "**/*.mts",
            "**/*.mjs",
            "**/*.{ts,tsx,js,jsx}",
            "!legacy/**/*.{ts,tsx,js,jsx}"
        ]
    );
}

#[test]
fn a_code_config_is_never_evaluated() {
    let r = roles_of(
        &[(
            "vitest.config.ts",
            "export default { test: { include: ['spec/**/*.ts'] } }\n",
        )],
        None,
    );
    assert_eq!(
        globs(&r, "ts-tests"),
        globs(&roles_of(&[], None), "ts-tests"),
        "@ts-tests stays at Forge's defaults"
    );
    assert_eq!(sources(&r, "ts-tests")[0], "forge default");
}

#[test]
fn forge_yaml_adds_or_sets() {
    let mut cfg = IndexMap::new();
    cfg.insert(
        "python-tests".to_string(),
        RoleChange {
            add: Some(vec!["integration/**/check_*.py".into()]),
            set: None,
        },
    );
    cfg.insert(
        "alembic-migrations".to_string(),
        RoleChange {
            add: None,
            set: Some(vec!["db/revisions/**".into()]),
        },
    );
    let r = roles_of(
        &[("alembic.ini", "[alembic]\nscript_location = db\n")],
        Some(&cfg),
    );
    let py = globs(&r, "python-tests");
    assert_eq!(py.last().unwrap(), "integration/**/check_*.py");
    assert_eq!(py[0], "**/test_*.py", "add keeps the defaults");
    assert_eq!(
        globs(&r, "alembic-migrations"),
        ["db/revisions/**"],
        "set replaces the tool's config too"
    );
    assert_eq!(sources(&r, "alembic-migrations"), ["forge.yaml"]);
}

#[test]
fn unknown_roles_and_cycles_are_rejected() {
    let mut cfg = IndexMap::new();
    cfg.insert(
        "tests".to_string(),
        RoleChange {
            add: Some(vec!["x/**".into()]),
            set: None,
        },
    );
    let e = resolve_roles(None, Some(&cfg)).unwrap_err();
    assert!(e.0.contains("unknown file role \"tests\""), "{e}");
    let mut cfg = IndexMap::new();
    cfg.insert(
        "env-template".to_string(),
        RoleChange {
            add: Some(vec!["@env-files".into()]),
            set: None,
        },
    );
    let e = resolve_roles(None, Some(&cfg)).unwrap_err();
    assert!(e.0.contains("cycle"), "{e}");
    let mut cfg = IndexMap::new();
    cfg.insert(
        "docs".to_string(),
        RoleChange {
            add: Some(vec!["@nope".into()]),
            set: None,
        },
    );
    assert!(resolve_roles(None, Some(&cfg)).is_err());
}

#[test]
fn a_role_includes_another_with_its_exclusions() {
    let r = resolve_roles(None, None).unwrap();
    assert_eq!(
        globs(&r, "env-files"),
        [
            "**/.env",
            "**/.env.*",
            "!**/.env.example",
            "!**/.env.sample",
            "!**/.env.template",
            "!**/.env.dist"
        ]
    );
    // !@role excludes its files; a role with exclusions cannot be negated
    let (g, _) = expand_globs(&["**/*.py".into(), "!@python-tests".into()], &r).unwrap();
    assert_eq!(
        g,
        ["**/*.py", "!**/test_*.py", "!**/*_test.py", "!**/conftest.py"]
    );
    assert!(expand_globs(&["!@env-files".into()], &r).is_err());
}

fn rule(v: serde_json::Value) -> Rule {
    serde_json::from_value(v).unwrap()
}

#[test]
fn roles_in_rules_become_their_patterns() {
    let r = resolve_roles(None, None).unwrap();
    let ru = rule(json!({
        "id": "x/y",
        "description": "d",
        "scope": {"language": "python", "files": ["**/*.py", "!@alembic-migrations"]},
        "enforce": [{"target": "check", "stage": "lint", "run": "true", "when_changed": ["@compose"]}],
    }));
    let (out, empty) = apply_roles(&ru, &r).unwrap();
    assert!(empty.is_empty());
    assert_eq!(
        out.scope.files.unwrap(),
        [
            "**/*.py",
            "!alembic/versions/**",
            "!migrations/versions/**"
        ]
    );
    let Target::Check(c) = &out.enforce[0] else {
        panic!()
    };
    assert_eq!(c.when_changed.as_ref().unwrap()[0], "**/compose*.yaml");
}

#[test]
fn a_rule_on_a_role_without_patterns_is_reported_empty() {
    let r = resolve_roles(None, None).unwrap();
    let ru = rule(json!({
        "id": "x/y",
        "description": "d",
        "scope": {"language": "typescript", "files": ["@typeorm-datasource"]},
        "enforce": [{"target": "check", "stage": "migration", "run": "true"}],
    }));
    let (out, empty) = apply_roles(&ru, &r).unwrap();
    assert_eq!(empty, ["typeorm-datasource"]);
    assert!(
        out.scope.files.unwrap().is_empty(),
        "it never runs on other files"
    );
}

#[test]
fn a_quoted_at_word_in_a_check_command_is_text() {
    let ru = rule(json!({
        "id": "x/y",
        "description": "d",
        "scope": {"language": "any"},
        "enforce": [{"target": "check", "stage": "lint", "run": "grep -q '@apply' x.css && forge_role @dockerfile Dockerfile"}],
    }));
    assert_eq!(roles_of_rule(&ru), ["dockerfile"]);
}

#[test]
fn a_where_module_naming_a_role_is_its_files_modules() {
    let mut cfg = IndexMap::new();
    cfg.insert(
        "wrangler-types".to_string(),
        RoleChange {
            add: None,
            set: Some(vec!["src/worker-configuration.d.ts".into()]),
        },
    );
    let r = resolve_roles(None, Some(&cfg)).unwrap();
    let ru = rule(json!({
        "id": "x/y",
        "description": "d",
        "scope": {"language": "typescript"},
        "enforce": [{"target": "ast-grep", "mode": "forbid", "rule": {"pattern": "$E.x"},
                     "where": {"E": {"type": {"module": "@wrangler-types", "name": "Env"}}}}],
    }));
    let (out, _) = apply_roles(&ru, &r).unwrap();
    let Target::AstGrep(a) = &out.enforce[0] else {
        panic!()
    };
    let w = serde_json::to_value(a.where_.as_ref().unwrap()).unwrap();
    assert_eq!(
        w["E"]["type"]["module"],
        json!(["src/worker-configuration"]),
        "TypeScript: the path without its extension"
    );
    // a role of globs cannot be a where module
    let ru = rule(json!({
        "id": "x/y",
        "description": "d",
        "scope": {"language": "python"},
        "enforce": [{"target": "ast-grep", "mode": "forbid", "rule": {"pattern": "$E.x"},
                     "where": {"E": {"type": {"module": "@python-tests"}}}}],
    }));
    let e = apply_roles(&ru, &resolve_roles(None, None).unwrap()).unwrap_err();
    assert!(e.0.contains("must be files, not globs"), "{e}");
}

#[test]
fn a_rule_naming_an_unknown_role_is_rejected_when_loaded() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join(".forge/rules");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("no-x.yaml"),
        "id: local/no-x\ndescription: No x.\nscope: {language: python, files: [\"@nope\"]}\nenforce:\n  - target: grep\n    mode: forbid\n    pattern: \"x\"\nexamples:\n  match: [\"x\"]\n  no_match: [\"y\"]\n",
    )
    .unwrap();
    let cache = t.path().join("cache");
    let e = forge_packs::load(
        Some(t.path()),
        forge_packs::Options {
            cache_dir: Some(Path::new(&cache)),
        },
    )
    .unwrap_err();
    assert!(e.to_string().contains("unknown file role @nope"), "{e}");
}
