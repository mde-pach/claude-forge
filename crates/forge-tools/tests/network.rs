//! Real downloads, opt-in: `FORGE_TEST_NETWORK=1 cargo test -p forge-tools --test network`.
//! Installs the pinned uv from its GitHub release and ty with it, into a temporary cache.

use std::process::Command;

use forge_home::Home;
use forge_tools::catalog::UV_VERSION;
use forge_tools::Provisioner;

fn enabled() -> bool {
    if std::env::var_os("FORGE_TEST_NETWORK").is_some_and(|v| v == "1") {
        return true;
    }
    eprintln!("skipped: set FORGE_TEST_NETWORK=1 to run the real-network test");
    false
}

fn version_of(exe: &std::path::Path) -> String {
    let out = Command::new(exe).arg("--version").output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn installs_uv_and_ty() {
    if !enabled() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let home = Home::new(t.path().join("cache"), t.path().join("state"));
    let p = Provisioner::new(home);

    let uv = p.ensure_uv_on_path().unwrap_or_else(|n| panic!("{n}"));
    assert!(version_of(&uv).contains(UV_VERSION));
    assert!(version_of(&p.home().path_dir().join("uvx")).contains(UV_VERSION));

    let ty = p.ensure("ty").unwrap_or_else(|n| panic!("{n}"));
    let spec = p.catalog().get("ty").unwrap();
    assert!(
        version_of(&ty).contains(spec.version),
        "{}",
        version_of(&ty)
    );
    assert_eq!(p.resolve("ty"), Some(ty));
}

/// The other kinds of source: a repackaged binary checked against upstream (hadolint), an
/// npm-only tool through Bun (lockfile-lint), a release binary (actionlint).
#[test]
fn installs_one_of_each_other_kind() {
    if !enabled() {
        return;
    }
    if std::env::var_os("FORGE_TEST_NETWORK_ALL").is_none() {
        eprintln!("skipped: also set FORGE_TEST_NETWORK_ALL=1 (downloads Bun, about 40 MB)");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let p = Provisioner::new(Home::new(t.path().join("cache"), t.path().join("state")));
    for tool in ["hadolint", "lockfile-lint", "actionlint"] {
        let exe = p.ensure(tool).unwrap_or_else(|n| panic!("{n}"));
        let out = Command::new(&exe).arg("--version").output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{tool}: {text}");
        let v = p.catalog().get(tool).unwrap().version;
        assert!(text.contains(v), "{tool}: {text}");
    }
}
