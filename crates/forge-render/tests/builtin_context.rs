//! The built-in packs' context on small repositories: what each kind of project reads is true
//! for it (the package manager its files name, the versions it declares, what it holds).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use forge_detect::{Site, active_packs, read_project_facts};
use forge_packs::model::Level;
use forge_packs::{Options, PackSet};
use forge_render::context::{PackContext, pack_context, render_rules_files};

static PACKS: LazyLock<PackSet> =
    LazyLock::new(|| forge_packs::load_builtin(Options::default()).expect("built-in packs load"));

/// The rules files `forge apply` writes for a repository holding `files`, by pack id.
fn rules_files(files: &[(&str, &str)]) -> BTreeMap<String, String> {
    let t = tempfile::tempdir().unwrap();
    for (p, c) in files {
        let f = t.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    let listed: Vec<String> = files.iter().map(|(p, _)| p.to_string()).collect();
    render(t.path(), &listed)
}

fn render(repo: &Path, files: &[String]) -> BTreeMap<String, String> {
    let facts = read_project_facts(repo, files).unwrap();
    let active = active_packs(&PACKS, Level::Project, None, Some(&facts));
    let site = Site::new(Level::Project, repo, &PACKS, Some(&facts), &active);
    let contexts: Vec<PackContext> = active
        .packs
        .iter()
        .map(|p| PackContext {
            id: &p.id,
            items: pack_context(&PACKS.packs[&p.id]),
            dirs: site.pack_dirs(&p.id),
            places: Some(site.pack_places(&p.id)),
        })
        .collect();
    render_rules_files(&contexts, Some(&facts), &active)
        .into_iter()
        .map(|(path, text)| {
            let id = path
                .trim_start_matches(".claude/rules/forge-")
                .trim_end_matches(".md")
                .to_string();
            (id, text)
        })
        .collect()
}

fn all(files: &BTreeMap<String, String>) -> String {
    files.values().cloned().collect::<Vec<_>>().join("\n")
}

const UV_PYPROJECT: &str =
    "[project]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"httpx>=0.27\"]\n";

#[test]
fn python_package_managers() {
    // plain scripts: no packaging file
    let scripts = rules_files(&[("main.py", "print(1)\n")]);
    assert!(
        scripts["python"].contains("`uv init --bare`"),
        "{scripts:?}"
    );
    assert!(!scripts.contains_key("python-uv"));

    let uv = rules_files(&[
        ("pyproject.toml", UV_PYPROJECT),
        ("uv.lock", ""),
        ("app/main.py", ""),
    ]);
    assert!(uv["python-uv"].contains("`uv add`"));
    assert!(!all(&uv).contains("uv init"));

    // a pyproject declaring dependencies without a lock is uv's
    let no_lock = rules_files(&[("pyproject.toml", UV_PYPROJECT)]);
    assert!(no_lock.contains_key("python-uv"));

    let poetry = rules_files(&[
        (
            "pyproject.toml",
            "[tool.poetry]\nname = \"app\"\n\n[tool.poetry.dependencies]\npython = \"^3.12\"\n",
        ),
        ("poetry.lock", ""),
    ]);
    assert!(poetry["python-poetry"].contains("`poetry add <pkg>`"));
    assert!(!all(&poetry).contains("uv "), "{poetry:?}");

    let pdm = rules_files(&[("pyproject.toml", UV_PYPROJECT), ("pdm.lock", "")]);
    assert!(pdm["python-pdm"].contains("`pdm add <pkg>`"));
    let pipenv = rules_files(&[("Pipfile", "[packages]\nrequests = \"*\"\n")]);
    assert!(pipenv["python-pipenv"].contains("`pipenv install <pkg>`"));

    let pip = rules_files(&[("requirements.txt", "requests==2.32.5\n"), ("app.py", "")]);
    assert!(pip["python-pip"].contains("exact versions"));
    assert!(!all(&pip).contains("uv "), "{pip:?}");
}

#[test]
fn a_condition_is_judged_where_the_pack_applies() {
    // Python at the root (uv), a JavaScript workspace package without Python: the package's
    // python.pm (none) is not the Python pack's
    let files = rules_files(&[
        ("pyproject.toml", UV_PYPROJECT),
        ("uv.lock", ""),
        (
            "package.json",
            "{\"name\": \"root\", \"private\": true, \"workspaces\": [\"web\"]}",
        ),
        (
            "web/package.json",
            "{\"name\": \"web\", \"dependencies\": {\"react\": \"^19.0.0\"}}",
        ),
    ]);
    assert!(!files["python"].contains("uv init"), "{files:?}");
}

#[test]
fn versions_gate_their_text() {
    let zod = |v: &str| {
        rules_files(&[(
            "package.json",
            &format!(
                "{{\"name\": \"x\", \"dependencies\": {{\"zod\": \"{v}\", \"typescript\": \"^5.9.0\"}}}}"
            ),
        )])
    };
    assert!(!zod("^3.23.0")["typescript-zod"].contains("Zod 4"));
    assert!(zod("^4.1.0")["typescript-zod"].contains("Zod 4"));

    let express = |v: &str| {
        rules_files(&[(
            "package.json",
            &format!(
                "{{\"name\": \"x\", \"dependencies\": {{\"express\": \"{v}\", \"typescript\": \"^5.9.0\"}}}}"
            ),
        )])
    };
    assert!(!all(&express("^4.21.0")).contains("Express 5"));
    assert!(all(&express("^5.1.0")).contains("Express 5"));
}

#[test]
fn shared_norms_are_rendered_once() {
    let files = rules_files(&[(
        "package.json",
        "{\"name\": \"x\", \"devDependencies\": {\"typescript\": \"^5.9.0\", \"vitest\": \"^4.0.0\", \"@playwright/test\": \"^1.56.0\"}}",
    )]);
    let text = all(&files);
    assert_eq!(
        text.matches("update that test in place").count(),
        1,
        "{files:?}"
    );
    assert_eq!(
        text.matches("search the codebase for an existing helper")
            .count(),
        1
    );
    // none in a repository without tests or code
    let docs = rules_files(&[("README.md", "# x\n")]);
    assert!(!all(&docs).contains("update that test"));
}

#[test]
fn sqlalchemy_async_text_needs_async() {
    let sync = rules_files(&[(
        "pyproject.toml",
        "[project]\nname = \"x\"\ndependencies = [\"sqlalchemy>=2.0\", \"alembic>=1.13\"]\n",
    )]);
    let text = all(&sync);
    assert!(text.contains("SQLAlchemy 2.0"));
    assert!(
        !text.contains("async_sessionmaker") && !text.contains("run_sync"),
        "{sync:?}"
    );
    let asyncio = rules_files(&[(
        "pyproject.toml",
        "[project]\nname = \"x\"\ndependencies = [\"sqlalchemy[asyncio]>=2.0\", \"alembic>=1.13\", \"asyncpg>=0.30\"]\n",
    )]);
    let text = all(&asyncio);
    assert!(
        text.contains("async_sessionmaker") && text.contains("run_sync"),
        "{asyncio:?}"
    );
}

#[test]
fn kubernetes_needs_a_kubernetes_manifest() {
    let backstage = rules_files(&[(
        "catalog-info.yaml",
        "apiVersion: backstage.io/v1alpha1\nkind: Component\nmetadata:\n  name: x\n",
    )]);
    assert!(!backstage.contains_key("kubernetes"));
    let tekton = rules_files(&[(
        "ci/task.yaml",
        "apiVersion: tekton.dev/v1\nkind: Task\nmetadata:\n  name: x\n",
    )]);
    assert!(!tekton.contains_key("kubernetes"));
    let deployment = rules_files(&[(
        "deploy/app.yaml",
        "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: x\n",
    )]);
    assert!(deployment.contains_key("kubernetes"));
    let service = rules_files(&[(
        "k8s/svc.yml",
        "---\napiVersion: v1\nkind: \"Service\"\nmetadata:\n  name: x\n",
    )]);
    assert!(service.contains_key("kubernetes"));
}

#[test]
fn compose_only_repositories_get_no_dockerfile_norms() {
    let compose = rules_files(&[("compose.yaml", "services:\n  db:\n    image: postgres:17\n")]);
    assert!(compose.contains_key("docker-compose"), "{compose:?}");
    assert!(!all(&compose).contains("USER"));
    let dockerfile = rules_files(&[("Dockerfile", "FROM python:3.13\n")]);
    assert!(dockerfile["docker"].contains("non-root `USER`"));
}
