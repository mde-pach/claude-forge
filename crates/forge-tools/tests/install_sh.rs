//! `install.sh` against a fake release served over HTTP.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{Route, Server, sha256, tar_gz};
use forge_tools::Platform;
use forge_tools::forge_release::asset_name;

const BIN: &[u8] = b"#!/bin/sh\necho real-claude-forge \"$@\"\n";
const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../install.sh")
}

fn serve(server: &Server, version: &str, good_sum: bool) {
    let asset = asset_name(version, &Platform::current().unwrap());
    let dir = asset.trim_end_matches(".tar.gz").to_owned();
    let body = tar_gz(&[(&format!("{dir}/claude-forge"), BIN, 0o755)]);
    let sha = if good_sum {
        sha256(&body)
    } else {
        "f".repeat(64)
    };
    let sums = format!("{}  x.tar.gz\n{sha}  {asset}\n", "0".repeat(64));
    server.put(
        &format!("/v{version}/SHA256SUMS"),
        Route::Body(sums.into_bytes()),
    );
    server.put(&format!("/v{version}/{asset}"), Route::Body(body));
}

fn install(shell: &str, server: &Server, home: &Path, version: Option<&str>, path: &str) -> Output {
    let mut cmd = Command::new(shell);
    cmd.arg(script())
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .env("FORGE_DOWNLOAD_URL", server.base());
    if let Some(v) = version {
        cmd.env("FORGE_VERSION", v);
    }
    cmd.output().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn shells() -> Vec<&'static str> {
    ["/bin/sh", "/bin/dash", "/bin/bash"]
        .into_iter()
        .filter(|s| Path::new(s).exists())
        .collect()
}

#[test]
fn installs_claude_forge_and_forge() {
    let server = Server::start();
    serve(&server, "1.2.3", true);
    for shell in shells() {
        let t = tempfile::tempdir().unwrap();
        let out = install(shell, &server, t.path(), Some("1.2.3"), SYSTEM_PATH);
        let err = stderr(&out);
        assert!(out.status.success(), "{shell}: {err}");
        let bin = t.path().join(".local/bin");
        let run = Command::new(bin.join("forge")).arg("-V").output().unwrap();
        assert_eq!(run.stdout, b"real-claude-forge -V\n");
        assert_eq!(
            fs::read_link(bin.join("forge")).unwrap(),
            Path::new("claude-forge")
        );
        assert!(err.contains("add "), "{shell}: PATH hint expected: {err}");
        let left: Vec<_> = fs::read_dir(&bin)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");

        // Again, with the install dir on PATH: an upgrade in place, no warnings.
        let path = format!("{}:{SYSTEM_PATH}", bin.display());
        let out = install(shell, &server, t.path(), Some("1.2.3"), &path);
        let err = stderr(&out);
        assert!(out.status.success(), "{err}");
        assert!(!err.contains("warning") && !err.contains("add "), "{err}");
    }
}

#[test]
fn leaves_another_forge_alone() {
    let server = Server::start();
    serve(&server, "1.2.3", true);
    let t = tempfile::tempdir().unwrap();
    let foundry = t.path().join("foundry/bin");
    fs::create_dir_all(&foundry).unwrap();
    fs::write(foundry.join("forge"), "#!/bin/sh\necho foundry\n").unwrap();
    forge_tools::archive::set_executable(&foundry.join("forge")).unwrap();
    let path = format!("{}:{SYSTEM_PATH}", foundry.display());
    let out = install("/bin/sh", &server, t.path(), Some("1.2.3"), &path);
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("is another program named forge"), "{err}");
    let bin = t.path().join(".local/bin");
    assert!(bin.join("claude-forge").is_file());
    assert!(!bin.join("forge").exists());

    // A foreign forge inside the install dir itself is not overwritten either.
    fs::write(bin.join("forge"), "foreign").unwrap();
    let out = install("/bin/sh", &server, t.path(), Some("1.2.3"), SYSTEM_PATH);
    assert!(stderr(&out).contains("was left alone"), "{}", stderr(&out));
    assert_eq!(fs::read_to_string(bin.join("forge")).unwrap(), "foreign");
}

#[test]
fn refuses_a_bad_checksum_or_no_version() {
    let server = Server::start();
    serve(&server, "6.6.6", false);
    let t = tempfile::tempdir().unwrap();
    let out = install("/bin/sh", &server, t.path(), Some("6.6.6"), SYSTEM_PATH);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("does not match SHA256SUMS"),
        "{}",
        stderr(&out)
    );
    assert!(!t.path().join(".local/bin/claude-forge").exists());

    // The script as committed has no version until the release workflow fills it in.
    let out = install("/bin/sh", &server, t.path(), None, SYSTEM_PATH);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("set FORGE_VERSION"),
        "{}",
        stderr(&out)
    );

    let out = install("/bin/sh", &server, t.path(), Some("1;rm"), SYSTEM_PATH);
    assert!(
        stderr(&out).contains("invalid FORGE_VERSION"),
        "{}",
        stderr(&out)
    );

    let out = install("/bin/sh", &server, t.path(), Some("9.9.9"), SYSTEM_PATH);
    assert!(stderr(&out).contains("cannot download"), "{}", stderr(&out));
}
