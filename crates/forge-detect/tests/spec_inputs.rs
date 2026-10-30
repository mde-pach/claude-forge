//! §5 Inputs: detection kinds, activation, workspaces, the package-manager facts and library
//! detection, on small repositories written to a temporary directory. Hermetic: the file list
//! is read from the directory, not from git.

use std::path::Path;

use forge_detect::facts::{is_npm_library, is_py_library};
use forge_detect::{FACT_JS_PM, FACT_PYTHON_PM, active_packs, read_project_facts};
use forge_packs::model::{Level, ProjectConfig};
use serde_json::json;

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else {
            out.push(
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

fn files_of(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// The active packs of a repository holding `files` (local packs under `.forge/packs/`
/// included), with an optional forge.yaml text; only the ids `keep` selects.
fn active(
    files: &[(&str, &str)],
    forge_yaml: Option<&str>,
    keep: impl Fn(&str) -> bool,
) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), files);
    if let Some(y) = forge_yaml {
        write(dir.path(), &[("forge.yaml", y)]);
    }
    let set = forge_packs::load(Some(dir.path()), forge_packs::Options::default()).unwrap();
    let config: Option<ProjectConfig> = forge_packs::load_project_config(dir.path()).unwrap();
    let facts = read_project_facts(dir.path(), &files_of(dir.path())).unwrap();
    active_packs(&set, Level::Project, config.as_ref(), Some(&facts))
        .packs
        .into_iter()
        .map(|p| p.id)
        .filter(|id| keep(id))
        .collect()
}

fn local(id: &str) -> bool {
    id.starts_with("acme")
}

fn pack(id: &str, detect: &str) -> String {
    format!("id: {id}\ndescription: A local pack.\nlevel: project\ndetect: {detect}\n")
}

/// A `detect` block, the files of the project, whether the pack is detected.
type DetectCase<'a> = (&'a str, &'a [(&'a str, &'a str)], bool);

#[test]
fn every_detect_kind() {
    let cases: &[DetectCase] = &[
        ("{always: true}", &[], true),
        (
            "{files_any: [\"**/*.proto\"]}",
            &[("api/x.proto", "")],
            true,
        ),
        ("{files_any: [\"**/*.proto\"]}", &[("api/x.txt", "")], false),
        // deps with a version range; the lock's resolved version wins over the declared one
        (
            "{deps_any: [{ecosystem: npm, name: next, version: \">=16.0.0\"}]}",
            &[(
                "package.json",
                "{\"dependencies\": {\"next\": \"^16.1.0\"}}",
            )],
            true,
        ),
        (
            "{deps_any: [{ecosystem: npm, name: next, version: \">=16.0.0\"}]}",
            &[("package.json", "{\"dependencies\": {\"next\": \"14.2.0\"}}")],
            false,
        ),
        (
            "{deps_any: [{ecosystem: pypi, name: sqlalchemy, version: \">=2.0.0\"}]}",
            &[
                (
                    "pyproject.toml",
                    "[project]\nname = \"a\"\ndependencies = [\"SQLAlchemy>=1.4\"]\n",
                ),
                (
                    "uv.lock",
                    "version = 1\n\n[[package]]\nname = \"sqlalchemy\"\nversion = \"2.0.36\"\n",
                ),
            ],
            true,
        ),
        (
            "{deps_any: [{ecosystem: pypi, name: requests}]}",
            &[("requirements.txt", "requests==2.32.3\n")],
            true,
        ),
        // content_any: a line of a matching file; another file does not count
        (
            "{content_any: [{files: [next.config.*], pattern: \"cacheComponents:\\\\s*true\"}]}",
            &[(
                "next.config.ts",
                "export default {\n  cacheComponents: true,\n};\n",
            )],
            true,
        ),
        (
            "{content_any: [{files: [next.config.*], pattern: \"cacheComponents:\\\\s*true\"}]}",
            &[("README.md", "cacheComponents: true\n")],
            false,
        ),
        // yaml_keys: the first document only, at its top level
        (
            "{yaml_keys: [apiVersion, kind]}",
            &[(
                "deploy/app.yaml",
                "# a comment\n---\napiVersion: apps/v1\nkind: Deployment\n",
            )],
            true,
        ),
        (
            "{yaml_keys: [apiVersion, kind]}",
            &[(
                "deploy/app.yaml",
                "name: x\n---\napiVersion: apps/v1\nkind: Deployment\n",
            )],
            false,
        ),
        (
            "{yaml_keys: [apiVersion, kind]}",
            &[("deploy/app.yaml", "spec:\n  apiVersion: v1\n  kind: Pod\n")],
            false,
        ),
        // sources_any: the project's own sources, not vendored, environment or generated ones
        (
            "{sources_any: [\"**/*.py\"]}",
            &[("navigation.py", "x = 1\n")],
            true,
        ),
        (
            "{sources_any: [\"**/*.py\"]}",
            &[
                (".venv/lib/x.py", ""),
                ("node_modules/a/b.py", ""),
                ("api/svc_pb2.py", ""),
                ("pkg.egg-info/x.py", ""),
                (".claude/hooks/x.py", ""),
            ],
            false,
        ),
        // facts_any: a named fact of the root or a package
        (
            "{facts_any: {python.pm: [poetry, pdm]}}",
            &[
                ("pdm.lock", ""),
                ("pyproject.toml", "[project]\nname = \"a\"\n"),
            ],
            true,
        ),
        (
            "{facts_any: {python.pm: [poetry, pdm]}}",
            &[
                ("uv.lock", ""),
                ("pyproject.toml", "[project]\nname = \"a\"\n"),
            ],
            false,
        ),
        (
            "{facts_any: {js.pm: pnpm}}",
            &[("package.json", "{}"), ("pnpm-lock.yaml", "")],
            true,
        ),
        // library_any: a published library
        (
            "{library_any: [npm]}",
            &[(
                "package.json",
                "{\"name\": \"acme\", \"exports\": \"./i.js\", \"publishConfig\": {\"access\": \"public\"}}",
            )],
            true,
        ),
        (
            "{library_any: [npm]}",
            &[(
                "package.json",
                "{\"name\": \"web\", \"private\": true, \"exports\": \"./i.js\"}",
            )],
            false,
        ),
    ];
    for (detect, files, want) in cases {
        let p = pack("acme", detect);
        let mut all: Vec<(&str, &str)> = vec![(".forge/packs/acme/pack.yaml", &p)];
        all.extend_from_slice(files);
        let got = active(&all, None, local);
        assert_eq!(!got.is_empty(), *want, "detect {detect} on {files:?}");
    }
}

#[test]
fn children_requires_add_and_remove() {
    let files: &[(&str, &str)] = &[
        (
            ".forge/packs/acme/pack.yaml",
            &pack("acme", "{always: true}"),
        ),
        (
            ".forge/packs/acme/web/pack.yaml",
            &pack("acme/web", "{files_any: [web/**]}"),
        ),
        (
            ".forge/packs/acme/web/ui/pack.yaml",
            &pack("acme/web/ui", "{always: true}"),
        ),
        (
            ".forge/packs/acmeops/pack.yaml",
            &pack("acmeops", "{files_any: [ops/**]}"),
        ),
        (
            ".forge/packs/acmeops/deploy/pack.yaml",
            "id: acmeops/deploy\ndescription: D.\nlevel: project\nrequires: [acme/web]\ndetect: {always: true}\n",
        ),
        ("README.md", "x\n"),
    ];
    let starts = |id: &str| id.starts_with("acme");
    // a child is active only under its active parent (acme/web/ui is "always", but acme/web is not detected)
    assert_eq!(active(files, None, starts), ["acme"]);
    // packs.add activates the pack and its ancestors; requires activates the required pack and its ancestors
    let got = active(
        files,
        Some("forge: \"0.6\"\npacks: {add: [acmeops/deploy]}\n"),
        starts,
    );
    assert_eq!(
        got,
        [
            "acme",
            "acme/web",
            "acme/web/ui",
            "acmeops",
            "acmeops/deploy"
        ]
    );
    // packs.remove takes out the pack, its subtree, and the packs requiring it
    let got = active(
        files,
        Some("forge: \"0.6\"\npacks: {add: [acmeops/deploy], remove: [acme/web]}\n"),
        starts,
    );
    assert_eq!(got, ["acme", "acmeops"]);
}

#[test]
fn unknown_and_user_level_packs_in_forge_yaml_are_warnings() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[(
            "forge.yaml",
            "forge: \"0.6\"\npacks: {add: [nope, safety], remove: [gone]}\n",
        )],
    );
    let set = forge_packs::load(Some(dir.path()), forge_packs::Options::default()).unwrap();
    let config = forge_packs::load_project_config(dir.path()).unwrap();
    let facts = read_project_facts(dir.path(), &files_of(dir.path())).unwrap();
    let a = active_packs(&set, Level::Project, config.as_ref(), Some(&facts));
    assert_eq!(
        a.warnings,
        [
            "forge.yaml: packs.add: unknown pack \"nope\"",
            "forge.yaml: packs.add: \"safety\" is a user-level pack (use forge apply --user)",
            "forge.yaml: packs.remove: unknown pack \"gone\"",
        ]
    );
}

#[test]
fn workspaces_are_judged_package_by_package() {
    let next16 = pack(
        "acme",
        "{deps_any: [{ecosystem: npm, name: next, version: \">=16.0.0\"}]}",
    );
    // next@14 at the root, next@16 in a package: detected (versions are never merged)
    let got = active(
        &[
            (".forge/packs/acme/pack.yaml", &next16),
            (
                "package.json",
                "{\"name\": \"mono\", \"workspaces\": [\"apps/*\"], \"dependencies\": {\"next\": \"14.2.0\"}}",
            ),
            (
                "apps/web/package.json",
                "{\"dependencies\": {\"next\": \"16.0.1\"}}",
            ),
        ],
        None,
        local,
    );
    assert_eq!(got, ["acme"]);
    // a package outside the declared globs is not read
    let got = active(
        &[
            (".forge/packs/acme/pack.yaml", &next16),
            (
                "package.json",
                "{\"name\": \"mono\", \"workspaces\": [\"apps/*\"]}",
            ),
            (
                "other/web/package.json",
                "{\"dependencies\": {\"next\": \"16.0.1\"}}",
            ),
        ],
        None,
        local,
    );
    assert!(got.is_empty());
    // pnpm-workspace.yaml, uv members with exclude, and lerna's default packages/*
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            (
                "pnpm-workspace.yaml",
                "packages:\n  - 'libs/*'\n  - '!libs/skip'\n",
            ),
            ("libs/core/package.json", "{}"),
            ("libs/skip/package.json", "{}"),
        ],
    );
    assert_eq!(
        forge_detect::facts::workspace_dirs(dir.path(), &files_of(dir.path())),
        ["libs/core"]
    );
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            (
                "pyproject.toml",
                "[project]\nname = \"m\"\n[tool.uv.workspace]\nmembers = [\"services/*\"]\nexclude = [\"services/old\"]\n",
            ),
            ("services/api/pyproject.toml", "[project]\nname = \"api\"\n"),
            ("services/old/pyproject.toml", "[project]\nname = \"old\"\n"),
        ],
    );
    assert_eq!(
        forge_detect::facts::workspace_dirs(dir.path(), &files_of(dir.path())),
        ["services/api"]
    );
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            ("lerna.json", "{}"),
            ("packages/lib/package.json", "{}"),
            ("modules/x/package.json", "{}"),
        ],
    );
    assert_eq!(
        forge_detect::facts::workspace_dirs(dir.path(), &files_of(dir.path())),
        ["packages/lib"]
    );
    // an exclusion like !**/test/** also excludes the directory named test
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            (
                "package.json",
                "{\"workspaces\": [\"packages/*\", \"!**/test/**\"]}",
            ),
            ("packages/test/package.json", "{}"),
            ("packages/core/package.json", "{}"),
        ],
    );
    assert_eq!(
        forge_detect::facts::workspace_dirs(dir.path(), &files_of(dir.path())),
        ["packages/core"]
    );
    // without any declaration there are no workspace packages
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[("package.json", "{}"), ("packages/core/package.json", "{}")],
    );
    assert!(forge_detect::facts::workspace_dirs(dir.path(), &files_of(dir.path())).is_empty());
}

/// `python.pm` and `js.pm` for each packaging case, read from the repository's files.
#[test]
fn package_manager_facts() {
    let py: &[(&[(&str, &str)], &str)] = &[
        (
            &[
                ("uv.lock", ""),
                ("pyproject.toml", "[project]\nname = \"a\"\n"),
            ],
            "uv",
        ),
        (
            &[
                ("poetry.lock", ""),
                ("pyproject.toml", "[tool.poetry]\nname = \"a\"\n"),
            ],
            "poetry",
        ),
        (
            &[(
                "pyproject.toml",
                "[tool.poetry.dependencies]\npython = \"^3.12\"\nhttpx = \"*\"\n",
            )],
            "poetry",
        ),
        (
            &[
                ("pdm.lock", ""),
                ("pyproject.toml", "[project]\nname = \"a\"\n"),
            ],
            "pdm",
        ),
        (&[("Pipfile", "[packages]\nrequests = \"*\"\n")], "pipenv"),
        (
            &[(
                "pyproject.toml",
                "[project]\nname = \"a\"\ndependencies = [\"httpx\"]\n",
            )],
            "uv",
        ),
        (
            &[
                ("requirements.txt", "httpx==0.28.1\n"),
                ("pyproject.toml", "[tool.ruff]\nline-length = 100\n"),
            ],
            "pip",
        ),
        (&[("requirements-dev.txt", "pytest\n")], "pip"),
        (&[("setup.py", "from setuptools import setup\n")], "pip"),
        (&[("main.py", "print(1)\n")], "none"),
    ];
    for (files, want) in py {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), files);
        let f = read_project_facts(dir.path(), &files_of(dir.path())).unwrap();
        assert_eq!(f.root.fact(FACT_PYTHON_PM), Some(*want), "{files:?}");
    }
    // a workspace package sees the root's lock
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            (
                "pyproject.toml",
                "[project]\nname = \"m\"\n[tool.uv.workspace]\nmembers = [\"pkgs/*\"]\n",
            ),
            ("uv.lock", ""),
            ("pkgs/a/pyproject.toml", "[project]\nname = \"a\"\n"),
        ],
    );
    let f = read_project_facts(dir.path(), &files_of(dir.path())).unwrap();
    assert_eq!(f.workspaces[0].fact(FACT_PYTHON_PM), Some("uv"));

    let js: &[(&[(&str, &str)], &str)] = &[
        (
            &[
                ("package.json", "{\"packageManager\": \"pnpm@10.2.0\"}"),
                ("yarn.lock", ""),
            ],
            "pnpm",
        ),
        (&[("package.json", "{}"), ("bun.lock", "")], "bun"),
        (&[("package.json", "{}"), ("yarn.lock", "")], "yarn"),
        (
            &[("package.json", "{}"), ("package-lock.json", "{}")],
            "npm",
        ),
        (&[("package.json", "{}")], "npm"),
        (&[("main.py", "")], "none"),
    ];
    for (files, want) in js {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), files);
        let f = read_project_facts(dir.path(), &files_of(dir.path())).unwrap();
        assert_eq!(f.root.fact(FACT_JS_PM), Some(*want), "{files:?}");
    }
}

/// The root's package.json and pyproject.toml must parse; a package's broken one is skipped.
#[test]
fn malformed_root_manifests_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &[("package.json", "{ not json")]);
    let err = read_project_facts(dir.path(), &files_of(dir.path())).unwrap_err();
    assert_eq!(err.file, "package.json");
    assert!(err.message.starts_with("not valid JSON"), "{err}");
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &[("pyproject.toml", "[project\n")]);
    let err = read_project_facts(dir.path(), &files_of(dir.path())).unwrap_err();
    assert!(err.message.starts_with("not valid TOML"), "{err}");
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            ("package.json", "{\"workspaces\": [\"p/*\"]}"),
            ("p/a/package.json", "{ broken"),
        ],
    );
    assert!(read_project_facts(dir.path(), &files_of(dir.path())).is_ok());
}

/// §13.4 libraries: what counts as a published Python or npm library.
#[test]
fn library_detection() {
    let lib = json!({"project": {"name": "acme", "classifiers": ["License :: OSI Approved"]}, "build-system": {"build-backend": "hatchling.build"}});
    assert!(is_py_library(&lib, false, false));
    let bare =
        json!({"project": {"name": "acme"}, "build-system": {"build-backend": "hatchling.build"}});
    // a build backend alone is not evidence; CI that uploads is
    assert!(!is_py_library(&bare, false, false));
    assert!(is_py_library(&bare, false, true));
    // applications: no build backend, console scripts, a Private classifier, a manage.py
    assert!(!is_py_library(
        &json!({"project": {"name": "app"}}),
        false,
        true
    ));
    let mut scripts = lib.clone();
    scripts["project"]["scripts"] = json!({"acme": "acme.cli:main"});
    assert!(!is_py_library(&scripts, false, true));
    let mut private = lib.clone();
    private["project"]["classifiers"] = json!(["Private :: Do Not Upload"]);
    assert!(!is_py_library(&private, false, true));
    assert!(!is_py_library(&lib, true, true));
    // malformed metadata is not a library and does not crash
    assert!(!is_py_library(
        &json!({"project": "acme", "build-system": {}}),
        false,
        true
    ));
    let mut odd = bare.clone();
    odd["project"]["classifiers"] = json!("Private :: Do Not Upload");
    assert!(!is_py_library(&odd, false, false));

    let pubc = json!({"access": "public"});
    assert!(is_npm_library(
        &json!({"name": "acme", "exports": {".": "./dist/index.js"}, "publishConfig": pubc}),
        false
    ));
    assert!(is_npm_library(
        &json!({"name": "acme", "main": "dist/index.js", "types": "dist/index.d.ts"}),
        true
    ));
    assert!(!is_npm_library(
        &json!({"name": "web", "private": true, "exports": "./i.js", "publishConfig": pubc}),
        true
    ));
    assert!(!is_npm_library(
        &json!({"exports": "./i.js", "publishConfig": pubc}),
        true
    ));
    assert!(!is_npm_library(
        &json!({"name": "acme", "main": "index.js"}),
        true
    ));
    // an internal workspace package that exports, with nothing publishing it
    assert!(!is_npm_library(
        &json!({"name": "@acme/lib", "exports": "./dist/index.js"}),
        false
    ));
}

/// Release set-ups that publish: Changesets, or a CI workflow running a publish command.
#[test]
fn publishing_evidence_comes_from_ci_and_changesets() {
    let pkg = "{\"name\": \"acme\", \"exports\": \"./i.js\"}";
    let py = "[project]\nname = \"acme\"\n[build-system]\nbuild-backend = \"hatchling.build\"\n";
    let lib_of = |files: &[(&str, &str)]| {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), files);
        let f = read_project_facts(dir.path(), &files_of(dir.path())).unwrap();
        f.root
            .library
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
    };
    assert!(lib_of(&[("package.json", pkg)]).is_empty());
    assert_eq!(
        lib_of(&[("package.json", pkg), (".changeset/config.json", "{}")]),
        ["Npm"]
    );
    assert_eq!(
        lib_of(&[
            ("package.json", pkg),
            (
                ".github/workflows/release.yml",
                "jobs:\n  r:\n    steps:\n      - run: pnpm publish\n"
            )
        ]),
        ["Npm"]
    );
    assert_eq!(
        lib_of(&[
            ("pyproject.toml", py),
            (
                ".github/workflows/release.yml",
                "jobs:\n  r:\n    steps:\n      - run: uv build && uv publish\n"
            )
        ]),
        ["Pypi"]
    );
}

/// Built-in packs on small repositories: the detection the spec's packs chapter relies on.
#[test]
fn builtin_detection_examples() {
    let builtin = |id: &str| !id.starts_with("acme");
    // a Kubernetes manifest activates kubernetes whatever its file is called; a workflow or compose file does not
    let got = active(
        &[(
            "ops/web.yaml",
            "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n",
        )],
        None,
        builtin,
    );
    assert!(got.iter().any(|p| p == "kubernetes"), "{got:?}");
    let got = active(
        &[
            (".github/workflows/ci.yml", "name: ci\non: push\njobs: {}\n"),
            ("compose.yaml", "services:\n  db:\n    image: postgres:17\n"),
        ],
        None,
        builtin,
    );
    assert!(!got.iter().any(|p| p == "kubernetes"), "{got:?}");
    // plain scripts activate python; .py only in vendored or environment directories does not
    let got = active(
        &[
            ("navigation.py", "def route() -> None:\n    pass\n"),
            ("data/flights.json", "[]\n"),
        ],
        None,
        builtin,
    );
    assert!(got.iter().any(|p| p == "python"), "{got:?}");
    let got = active(
        &[(".venv/lib/x.py", ""), ("docs/index.md", "# x\n")],
        None,
        builtin,
    );
    assert!(!got.iter().any(|p| p == "python"), "{got:?}");
    // react sits under Next.js in a Next.js app
    let got = active(
        &[
            (
                "package.json",
                "{\"dependencies\": {\"next\": \"16.2.0\", \"react\": \"19.2.0\"}}",
            ),
            ("tsconfig.json", "{}"),
        ],
        None,
        builtin,
    );
    assert!(
        got.iter().any(|p| p == "typescript/nextjs/react"),
        "{got:?}"
    );
    assert!(
        !got.iter().any(|p| p == "typescript/astro/react"),
        "{got:?}"
    );
    // typescript/library for a published library, not for a private app
    let lib = "{\"name\": \"acme\", \"exports\": \"./dist/index.js\", \"publishConfig\": {\"access\": \"public\"}, \"devDependencies\": {\"typescript\": \"5.9.3\"}}";
    let got = active(
        &[("package.json", lib), ("tsconfig.json", "{}")],
        None,
        builtin,
    );
    assert!(got.iter().any(|p| p == "typescript/library"), "{got:?}");
    let app = lib.replace("\"name\": \"acme\"", "\"name\": \"web\", \"private\": true");
    let got = active(
        &[("package.json", &app), ("tsconfig.json", "{}")],
        None,
        builtin,
    );
    assert!(!got.iter().any(|p| p == "typescript/library"), "{got:?}");
}
