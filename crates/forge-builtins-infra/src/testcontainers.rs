//! Tests and the third-party services they need: `forge_testcontainers_advice`
//! (docker/compose/tests-use-testcontainers, under `forge_advise`) and `forge_tests_docker`, the
//! test checks' Docker check (§10): a test that did not run is not a passing test.

use std::path::Path;

use forge_core::Report;
use forge_core::docker::{Env, daemon_answers};
use forge_detect::compose::{
    ComposeService, TestLanguage, declares_testcontainers, imports_testcontainers,
    is_ts_test_setup, testcontainers_declared_in, third_party_services,
};

use crate::cx::Cx;

/// The compose services `forge.yaml` says are not dependencies (`docker.compose.not-dependencies`).
fn not_dependencies(root: &Path) -> Vec<String> {
    forge_packs::load_project_config(root)
        .ok()
        .flatten()
        .and_then(|c| c.docker)
        .and_then(|d| d.compose)
        .and_then(|c| c.not_dependencies)
        .unwrap_or_default()
}

fn is_manifest(path: &str, lang: TestLanguage) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    match lang {
        TestLanguage::Python => {
            base == "pyproject.toml"
                || base == "Pipfile"
                || base.ends_with(".txt")
                    && (base.starts_with("requirements") || path.contains("requirements/"))
        }
        TestLanguage::TypeScript => base == "package.json",
    }
}

/// `forge_testcontainers_advice`: where the repository has tests and a compose file runs a
/// third-party service, each language whose tests exist declares Testcontainers as a dependency
/// and its test suite imports it (a test file, `conftest.py`, or a vitest/jest/bun setup file).
/// One line per language that does not, at the first of the check's files (a test file the turn
/// touched).
pub fn advice(cx: &Cx<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    let files = cx.repo_files(&[]);
    let files: Vec<&String> = files
        .iter()
        .filter(|f| !forge_detect::facts::is_vendored(f))
        .collect();
    let excluded = not_dependencies(cx.root());
    let mut services: Vec<ComposeService> = Vec::new();
    for f in files.iter().filter(|f| cx.role("compose", f)) {
        for s in third_party_services(&cx.now(f).unwrap_or_default(), &excluded) {
            if !services.iter().any(|x| x.name == s.name) {
                services.push(s);
            }
        }
    }
    if services.is_empty() {
        return r;
    }
    let at = cx.change_files().into_iter().next();
    for (lang, role) in [
        (TestLanguage::Python, "python-tests"),
        (TestLanguage::TypeScript, "ts-tests"),
    ] {
        let tests: Vec<&&String> = files.iter().filter(|f| cx.role(role, f)).collect();
        if tests.is_empty() {
            continue;
        }
        let declared = files.iter().filter(|f| is_manifest(f, lang)).any(|f| {
            let base = f.rsplit('/').next().unwrap_or(f);
            declares_testcontainers(base, &cx.now(f).unwrap_or_default(), lang)
        });
        let suite = tests.iter().map(|f| f.as_str()).chain(
            files
                .iter()
                .filter(|f| lang == TestLanguage::TypeScript && is_ts_test_setup(f))
                .map(|f| f.as_str()),
        );
        let imported = declared
            && suite
                .into_iter()
                .any(|f| imports_testcontainers(&cx.now(f).unwrap_or_default(), lang));
        if declared && imported {
            continue;
        }
        let list = services
            .iter()
            .map(|s| format!("{} ({})", s.name, s.image))
            .collect::<Vec<_>>()
            .join(", ");
        let missing = if declared {
            "no test, conftest.py or test setup file imports it"
        } else {
            "it is not a declared test dependency"
        };
        let lang_name = match lang {
            TestLanguage::Python => "Python",
            TestLanguage::TypeScript => "JS/TS",
        };
        let msg = format!(
            "The {lang_name} tests run beside the compose services {list} without Testcontainers ({}: {missing}). Start each service a test needs with Testcontainers from the tests (a fixture or setup file) instead of relying on a running compose stack, and declare it as a test dependency",
            lang.package()
        );
        r.findings.push(match &at {
            Some(p) => cx.at(p, 0, msg),
            None => cx.error(msg),
        });
    }
    r
}

/// `forge_tests_docker <python|typescript>`: before a test check runs its tests, where they use
/// Testcontainers (declared for that language in the check's directory or above it), a Docker
/// daemon answers; else the check fails with what to do, and the tests are not run.
pub fn tests_docker(cx: &Cx<'_>, args: &[String]) -> Report {
    let mut r = Report::default();
    let Some(lang) = args.first().and_then(|a| TestLanguage::parse(a)) else {
        return r;
    };
    let dir = if cx.check.cwd.as_os_str().is_empty() {
        cx.root().to_path_buf()
    } else {
        cx.check.cwd.clone()
    };
    if !testcontainers_declared_in(cx.root(), &dir, lang) {
        return r;
    }
    if daemon_answers(&Env::current()) {
        return r;
    }
    r.findings.push(cx.error(
        "The tests did not run: they use Testcontainers, which needs Docker, and no Docker daemon answers. Start it (`colima start`, `open -a Docker`, `orb start`, `podman machine start`, `systemctl start docker`) or ask the user to, then run the tests again",
    ));
    r
}
