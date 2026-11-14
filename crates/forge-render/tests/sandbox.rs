//! `sandbox` targets with `os` (§7.2): an entry is added only where Claude Code's sandbox runs on
//! that system. `testcontainers/sandbox-docker` on each: macOS lets the sandbox reach the Docker
//! socket and the containers' ports; Linux, where no sandbox entry can, takes the test commands
//! out of the sandbox.

use std::sync::LazyLock;

use forge_detect::roles::{RoleInputs, resolve_roles};
use forge_detect::{Site, active_packs, read_project_facts};
use forge_packs::model::{Level, SandboxOs};
use forge_packs::{Options, PackSet};
use forge_render::plan::LOCAL_SETTINGS_FILE;
use forge_render::rules::{SettingsTarget, select};
use serde_json::Value;

static PACKS: LazyLock<PackSet> =
    LazyLock::new(|| forge_packs::load_builtin(Options::default()).expect("built-in packs load"));

/// The `sandbox.*` entries, as (file, path, value), and the warnings, for a project whose tests
/// use Testcontainers, planned as if Claude Code's sandbox ran on `os`.
fn sandbox_entries(os: Option<SandboxOs>) -> (Vec<(String, String, Value)>, Vec<String>) {
    let t = tempfile::tempdir().unwrap();
    let files: Vec<String> = ["pyproject.toml", "tests/test_db.py"]
        .map(String::from)
        .to_vec();
    std::fs::create_dir_all(t.path().join("tests")).unwrap();
    std::fs::write(
        t.path().join("pyproject.toml"),
        "[project]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependency-groups]\ndev = [\"pytest>=8.3\", \"testcontainers[postgres]>=4.10\"]\n",
    )
    .unwrap();
    std::fs::write(
        t.path().join("tests/test_db.py"),
        "from testcontainers.postgres import PostgresContainer\n",
    )
    .unwrap();
    let facts = read_project_facts(t.path(), &files).unwrap();
    let active = active_packs(&PACKS, Level::Project, None, Some(&facts));
    assert!(
        active.packs.iter().any(|p| p.id == "testcontainers"),
        "the testcontainers pack is active"
    );
    let site = Site::new(Level::Project, t.path(), &PACKS, Some(&facts), &active);
    let roles = resolve_roles(
        Some(&RoleInputs {
            repo: t.path(),
            files: &files,
        }),
        None,
    )
    .unwrap();
    let mut warnings = Vec::new();
    let selected = select(
        &PACKS,
        &site,
        Level::Project,
        None,
        &roles,
        SettingsTarget {
            file: ".claude/settings.json",
            label: ".claude/settings.json",
            os,
        },
        &mut warnings,
    )
    .unwrap();
    let entries = selected
        .settings
        .iter()
        .filter(|e| e.entry.path.starts_with("sandbox."))
        .map(|e| {
            (
                e.entry.file.clone(),
                e.entry.path.clone(),
                e.entry.value.clone(),
            )
        })
        .collect();
    (entries, warnings)
}

#[test]
fn on_linux_nothing_is_written() {
    let (entries, warnings) = sandbox_entries(Some(SandboxOs::Linux));
    assert!(entries.is_empty(), "{entries:?}");
    // no socket is looked for: no entry names one, so no warning says none was found
    assert!(
        !warnings.iter().any(|w| w.contains("Docker socket")),
        "{warnings:?}"
    );
}

#[test]
fn on_macos_the_sandbox_reaches_the_socket_and_the_ports() {
    let (entries, warnings) = sandbox_entries(Some(SandboxOs::Macos));
    assert!(
        entries.iter().all(|(f, _, _)| f == LOCAL_SETTINGS_FILE),
        "{entries:?}"
    );
    assert!(entries.contains(&(
        LOCAL_SETTINGS_FILE.to_string(),
        "sandbox.network.allowLocalBinding".to_string(),
        Value::Bool(true)
    )));
    // the socket this machine has, or the warning that it has none
    assert!(
        entries
            .iter()
            .any(|(_, p, _)| p == "sandbox.network.allowUnixSockets")
            || warnings
                .iter()
                .any(|w| w.contains("no Docker socket found")),
        "{entries:?} {warnings:?}"
    );
}

#[test]
fn elsewhere_no_entry_for_a_system_is_added() {
    let (entries, _) = sandbox_entries(None);
    assert!(entries.is_empty(), "{entries:?}");
}
