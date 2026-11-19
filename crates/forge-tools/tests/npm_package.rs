//! The npm packaging: `npm/stage.mjs` builds the packages from release archives, and the
//! main package's launcher runs this platform's binary. Skipped without `node`.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::tar_gz;

const TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
];

fn node_platform() -> Option<String> {
    let out = Command::new("node")
        .args(["-p", "process.platform + '-' + process.arch"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[test]
fn staged_packages_run_the_platform_binary() {
    let Some(plat) = node_platform() else {
        eprintln!("skipped: node is not installed");
        return;
    };
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let t = tempfile::tempdir().unwrap();
    let dist = t.path().join("dist");
    fs::create_dir(&dist).unwrap();
    for target in TARGETS {
        let script = format!("#!/bin/sh\necho {target} \"$@\"\nexit 7\n");
        let body = tar_gz(&[(
            &format!("claude-forge-1.2.3-{target}/claude-forge"),
            script.as_bytes(),
            0o755,
        )]);
        fs::write(
            dist.join(format!("claude-forge-1.2.3-{target}.tar.gz")),
            body,
        )
        .unwrap();
    }
    let out = t.path().join("out");
    let st = Command::new("node")
        .arg(repo.join("npm/stage.mjs"))
        .args(["1.2.3".as_ref(), dist.as_os_str(), out.as_os_str()])
        .status()
        .unwrap();
    assert!(st.success());

    let main = fs::read_to_string(out.join("claude-forge/package.json")).unwrap();
    assert!(main.contains("\"version\": \"1.2.3\""));
    assert!(main.contains("\"@maximedepachtere/claude-forge-linux-x64\": \"1.2.3\""));
    let linux = fs::read_to_string(out.join("linux-x64/package.json")).unwrap();
    assert!(linux.contains("\"os\": [\n    \"linux\"\n  ]"), "{linux}");
    assert!(linux.contains("\"cpu\": [\n    \"x64\"\n  ]"), "{linux}");

    // node_modules as npm lays it out for this platform.
    let nm = t.path().join("proj/node_modules/@maximedepachtere");
    fs::create_dir_all(&nm).unwrap();
    fs::rename(out.join("claude-forge"), nm.join("claude-forge")).unwrap();
    let shim = nm.join("claude-forge/bin/claude-forge.js");
    // Without the platform package: a clear error.
    let r = Command::new("node").arg(&shim).output().unwrap();
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("is not installed"));

    fs::rename(out.join(&plat), nm.join(format!("claude-forge-{plat}"))).unwrap();
    let r = Command::new("node")
        .arg(&shim)
        .args(["hook", "a b"])
        .output()
        .unwrap();
    let want_target = match plat.as_str() {
        "darwin-arm64" => "aarch64-apple-darwin",
        "darwin-x64" => "x86_64-apple-darwin",
        "linux-arm64" => "aarch64-unknown-linux-musl",
        _ => "x86_64-unknown-linux-musl",
    };
    assert_eq!(
        String::from_utf8_lossy(&r.stdout),
        format!("{want_target} hook a b\n")
    );
    assert_eq!(r.status.code(), Some(7));
}
