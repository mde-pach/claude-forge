//! Provisioning against a local HTTP server: download, sha256 check, unpacking, atomic
//! installs, the lock, uv-installed and npm tools (with fake uv and Bun).

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use common::{Route, Server, leak, names, sha256, tar_gz, zip};
use forge_core::NoteKind;
use forge_home::Home;
use forge_tools::catalog::{Asset, Catalog, Source, ToolSpec};
use forge_tools::{Http, Platform, Provisioner};

const SCRIPT: &[u8] = b"#!/bin/sh\necho hi \"$@\"\n";

fn platform() -> Platform {
    Platform::current().unwrap()
}

fn assets(name: &str, sha: &str) -> &'static [Asset] {
    let a = Asset {
        platform: leak(platform().lookup_keys()[0].clone()),
        name: leak(name.to_owned()),
        sha256: leak(sha.to_owned()),
    };
    Box::leak(Box::new([a]))
}

fn release(server: &Server, name: &str, asset: &str, body: &[u8], sha: Option<&str>) -> ToolSpec {
    let path = format!("/{name}/1.0.0/{asset}");
    server.put(&path, Route::Body(body.to_vec()));
    let sha = sha.map_or_else(|| sha256(body), str::to_owned);
    ToolSpec {
        name: leak(name.to_owned()),
        version: "1.0.0",
        source: Source::Release {
            base: leak(format!("{}/{name}/1.0.0/", server.base())),
            binary: leak(name.to_owned()),
            assets: assets(asset, &sha),
        },
    }
}

fn provisioner(home: &Path, tools: Vec<ToolSpec>) -> Provisioner {
    let home = Home::new(home.join("cache"), home.join("state"));
    Provisioner::with(
        home,
        Catalog { tools },
        Some(platform()),
        Http::allowing_http(),
    )
}

fn run(p: &Path) -> String {
    let out = Command::new(p).arg("x").output().unwrap();
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn release_tar_gz_is_downloaded_checked_and_found_again() {
    let server = Server::start();
    let body = tar_gz(&[
        ("pkg/README", b"readme", 0o644),
        ("pkg/mytool", SCRIPT, 0o755),
    ]);
    let spec = release(&server, "mytool", "mytool.tar.gz", &body, None);
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(t.path(), vec![spec]);

    assert_eq!(p.resolve("mytool"), None);
    let exe = p.ensure("mytool").unwrap();
    assert_eq!(
        exe,
        p.home().tool_dir("mytool", "1.0.0").unwrap().join("mytool")
    );
    assert_eq!(run(&exe), "hi x\n");
    assert_eq!(p.resolve("mytool"), Some(exe.clone()));
    // Only the binary lands; nothing temporary is left beside it.
    assert_eq!(names(exe.parent().unwrap()), ["mytool"]);
    assert_eq!(
        names(&p.home().tools_dir().join("mytool")),
        [".1.0.0.lock", "1.0.0"]
    );
    // Found again without a download.
    p.ensure("mytool").unwrap();
    assert_eq!(server.hits("/mytool/1.0.0/mytool.tar.gz"), 1);
}

#[test]
fn zip_and_raw_assets() {
    let server = Server::start();
    let z = release(
        &server,
        "ztool",
        "ztool.zip",
        &zip(&[("d/ztool", SCRIPT)]),
        None,
    );
    let r = release(&server, "rtool", "rtool_darwin_arm64", SCRIPT, None);
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(t.path(), vec![z, r]);
    assert_eq!(run(&p.ensure("ztool").unwrap()), "hi x\n");
    assert_eq!(run(&p.ensure("rtool").unwrap()), "hi x\n");
}

#[test]
fn checksum_mismatch_installs_nothing() {
    let server = Server::start();
    let body = tar_gz(&[("mytool", SCRIPT, 0o755)]);
    let spec = release(&server, "mytool", "m.tar.gz", &body, Some(&"0".repeat(64)));
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(t.path(), vec![spec]);
    let note = p.ensure("mytool").unwrap_err();
    assert_eq!(note.kind, NoteKind::Setup);
    assert!(
        note.text.contains("does not match its pinned sha256"),
        "{note}"
    );
    assert_eq!(p.resolve("mytool"), None);
    assert_eq!(names(&p.home().tools_dir().join("mytool")), [".1.0.0.lock"]);
}

#[test]
fn network_failures_are_notes() {
    let server = Server::start();
    let mut spec = release(&server, "mytool", "m.tar.gz", b"", None);
    server.put("/mytool/1.0.0/m.tar.gz", Route::Status(404));
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(t.path(), vec![spec]);
    let note = p.ensure("mytool").unwrap_err();
    assert_eq!(note.kind, NoteKind::Environment);
    assert!(note.text.contains("HTTP 404"), "{note}");
    assert!(note.text.contains("`forge apply` tries again"), "{note}");

    // Nothing listening.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    if let Source::Release { base, .. } = &mut spec.source {
        *base = leak(format!("http://127.0.0.1:{port}/"));
    }
    let p = provisioner(t.path(), vec![spec]);
    let note = p.ensure("mytool").unwrap_err();
    assert_eq!(note.kind, NoteKind::Environment);
    assert!(note.text.contains("network error"), "{note}");
}

#[test]
fn truncated_download_leaves_nothing_and_a_retry_works() {
    let server = Server::start();
    let body = tar_gz(&[("mytool", SCRIPT, 0o755)]);
    let spec = release(&server, "mytool", "m.tar.gz", &body, None);
    server.put(
        "/mytool/1.0.0/m.tar.gz",
        Route::Truncated(body.clone(), body.len() / 2),
    );
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(t.path(), vec![spec]);
    assert!(p.ensure("mytool").is_err());
    assert_eq!(names(&p.home().tools_dir().join("mytool")), [".1.0.0.lock"]);

    // An incomplete directory from an older crash is replaced.
    let dir = p.home().tool_dir("mytool", "1.0.0").unwrap();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("junk"), "").unwrap();
    server.put("/mytool/1.0.0/m.tar.gz", Route::Body(body));
    let exe = p.ensure("mytool").unwrap();
    assert_eq!(names(&dir), ["mytool"]);
    assert_eq!(run(&exe), "hi x\n");
}

#[test]
fn concurrent_ensures_download_once() {
    let server = Server::start();
    let body = tar_gz(&[("mytool", SCRIPT, 0o755)]);
    let spec = release(&server, "mytool", "m.tar.gz", &body, None);
    server.put(
        "/mytool/1.0.0/m.tar.gz",
        Route::Slow(body, Duration::from_millis(300)),
    );
    let t = tempfile::tempdir().unwrap();
    let p = Arc::new(provisioner(t.path(), vec![spec]));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let p = p.clone();
            std::thread::spawn(move || p.ensure("mytool").unwrap())
        })
        .collect();
    let paths: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(paths.windows(2).all(|w| w[0] == w[1]));
    assert_eq!(server.hits("/mytool/1.0.0/m.tar.gz"), 1);
}

#[test]
fn unknown_tool_and_missing_platform() {
    let server = Server::start();
    let mut spec = release(&server, "mytool", "m.tar.gz", b"x", None);
    if let Source::Release { assets, .. } = &mut spec.source {
        *assets = &[];
    }
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(t.path(), vec![spec]);
    assert_eq!(p.ensure("nope").unwrap_err().kind, NoteKind::ForgeFailure);
    let n = p.ensure("mytool").unwrap_err();
    assert_eq!(n.kind, NoteKind::MissingTool);
    assert!(n.text.contains("no pinned mytool 1.0.0 for"), "{n}");
}

/// A fake uv: `uv tool install ... <pkg>==<ver>` writes `$UV_TOOL_BIN_DIR/<binary>` (the
/// package name minus `-cli`) and logs its arguments and environment.
const FAKE_UV: &str = r#"#!/bin/sh
log="$UV_TOOL_DIR/../uv.log"
mkdir -p "$UV_TOOL_DIR" "$UV_TOOL_BIN_DIR"
printf '%s\n' "$*" > "$log"
printf 'cwd=%s\ncache=%s\npython=%s\n' "$(pwd -P)" "$UV_CACHE_DIR" "$UV_PYTHON_INSTALL_DIR" >> "$log"
for last; do :; done
pkg=${last%%==*}
case $pkg in
  broken) echo "error: No solution found when resolving tool dependencies" >&2; exit 1 ;;
  nobin) exit 0 ;;
esac
bin=${pkg%-cli}
printf '#!/bin/sh\necho %s "$@"\n' "$bin" > "$UV_TOOL_DIR/$bin"
chmod 755 "$UV_TOOL_DIR/$bin"
ln -s "$UV_TOOL_DIR/$bin" "$UV_TOOL_BIN_DIR/$bin"
"#;

fn uv_spec(server: &Server) -> ToolSpec {
    let body = tar_gz(&[
        ("uv-x/uv", FAKE_UV.as_bytes(), 0o755),
        ("uv-x/uvx", b"#!/bin/sh\necho uvx\n", 0o755),
    ]);
    server.put("/uv/uv.tar.gz", Route::Body(body.clone()));
    ToolSpec {
        name: "uv",
        version: "0.13.1",
        source: Source::Uv {
            base: leak(format!("{}/uv/", server.base())),
            assets: assets("uv.tar.gz", &sha256(&body)),
        },
    }
}

fn uv_tool(
    name: &'static str,
    package: &'static str,
    sha: &'static [(&'static str, &'static str)],
) -> ToolSpec {
    ToolSpec {
        name,
        version: "2.0.0",
        source: Source::UvTool {
            package,
            binary: name,
            binary_sha256: sha,
        },
    }
}

#[test]
fn uv_goes_on_path_and_installs_python_tools() {
    let server = Server::start();
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(
        t.path(),
        vec![
            uv_spec(&server),
            uv_tool("squawk", "squawk-cli", &[]),
            uv_tool("broken", "broken", &[]),
            uv_tool("nobin", "nobin", &[]),
        ],
    );
    let path_dir = p.home().path_dir();
    fs::create_dir_all(&path_dir).unwrap();
    fs::write(path_dir.join("python"), "stray").unwrap();

    let uv = p.ensure_uv_on_path().unwrap();
    assert_eq!(names(&path_dir), ["uv", "uvx"]);
    assert_eq!(fs::read_link(path_dir.join("uv")).unwrap(), uv);
    assert_eq!(
        fs::read_link(path_dir.join("uvx")).unwrap(),
        uv.with_file_name("uvx")
    );
    assert_eq!(p.resolve_uvx(), Some(uv.with_file_name("uvx")));

    let exe = p.ensure("squawk").unwrap();
    let dir = p.home().tool_dir("squawk", "2.0.0").unwrap();
    assert_eq!(exe, dir.join("bin/squawk"));
    assert_eq!(run(&exe), "squawk x\n");
    assert!(dir.join(".forge-installed").is_file());
    assert_eq!(p.resolve("squawk"), Some(exe));
    let log = fs::read_to_string(dir.join("uv.log")).unwrap();
    assert!(
        log.starts_with("tool install --quiet --managed-python --python 3.12 squawk-cli==2.0.0\n"),
        "{log}"
    );
    assert!(
        log.contains(&format!("cwd={}", dir.canonicalize().unwrap().display())),
        "{log}"
    );
    assert!(log.contains(&format!(
        "cache={}",
        p.home().tools_dir().join(".uv-cache").display()
    )));

    let n = p.ensure("broken").unwrap_err();
    assert_eq!(n.kind, NoteKind::Environment);
    assert!(n.text.contains("No solution found"), "{n}");
    assert_eq!(p.resolve("broken"), None);

    let n = p.ensure("nobin").unwrap_err();
    assert!(n.text.contains("has no `nobin` executable"), "{n}");
    assert!(!p.home().tool_dir("nobin", "2.0.0").unwrap().exists());
}

#[test]
fn an_install_without_its_marker_is_redone() {
    let server = Server::start();
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(
        t.path(),
        vec![uv_spec(&server), uv_tool("squawk", "squawk-cli", &[])],
    );
    let exe = p.ensure("squawk").unwrap();
    let dir = p.home().tool_dir("squawk", "2.0.0").unwrap();
    fs::remove_file(dir.join(".forge-installed")).unwrap();
    assert_eq!(p.resolve("squawk"), None);
    assert_eq!(p.ensure("squawk").unwrap(), exe);
    assert!(dir.join(".forge-installed").is_file());
}

#[test]
fn a_repackaged_binary_must_match_upstream() {
    let server = Server::start();
    let t = tempfile::tempdir().unwrap();
    let key = leak(platform().lookup_keys()[0].clone());
    let want = sha256(b"#!/bin/sh\necho hadolint \"$@\"\n");
    let good: &'static [(&'static str, &'static str)] = Box::leak(Box::new([(key, leak(want))]));
    let bad: &'static [(&'static str, &'static str)] =
        Box::leak(Box::new([(key, leak("1".repeat(64)))]));
    let p = provisioner(
        t.path(),
        vec![uv_spec(&server), uv_tool("hadolint", "hadolint", good)],
    );
    assert_eq!(run(&p.ensure("hadolint").unwrap()), "hadolint x\n");

    let t = tempfile::tempdir().unwrap();
    let p = provisioner(
        t.path(),
        vec![uv_spec(&server), uv_tool("hadolint", "hadolint", bad)],
    );
    let n = p.ensure("hadolint").unwrap_err();
    assert_eq!(n.kind, NoteKind::Setup);
    assert!(
        n.text.contains("does not match the upstream release"),
        "{n}"
    );
    assert!(!p.home().tool_dir("hadolint", "2.0.0").unwrap().exists());
}

/// A fake Bun: `bun install` creates `node_modules/.bin/fakecli`; `bun --bun <script>`
/// runs the script with sh.
const FAKE_BUN: &str = r#"#!/bin/sh
case $1 in
  install)
    grep -q '"fake-cli":"3.0.0"' package.json || exit 1
    mkdir -p node_modules/fake-cli node_modules/.bin
    printf 'echo fakecli "$@"\n' > node_modules/fake-cli/cli.js
    ln -s ../fake-cli/cli.js node_modules/.bin/fakecli
    printf '%s\n' "$*" > install.log ;;
  --bun) shift; exec sh "$@" ;;
  *) exit 2 ;;
esac
"#;

#[test]
fn npm_tools_run_through_a_provisioned_bun() {
    let server = Server::start();
    let body = zip(&[("bun-x/bun", FAKE_BUN.as_bytes())]);
    server.put("/bun/bun.zip", Route::Body(body.clone()));
    let bun = ToolSpec {
        name: "bun",
        version: "1.4.3",
        source: Source::Bun {
            base: leak(format!("{}/bun/", server.base())),
            assets: assets("bun.zip", &sha256(&body)),
        },
    };
    let cli = ToolSpec {
        name: "fakecli",
        version: "3.0.0",
        source: Source::Npm {
            package: "fake-cli",
            binary: "fakecli",
        },
    };
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(t.path(), vec![bun, cli]);
    let exe = p.ensure("fakecli").unwrap();
    assert_eq!(run(&exe), "fakecli x\n");
    let dir = p.home().tool_dir("fakecli", "3.0.0").unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("install.log")).unwrap(),
        "install --production --ignore-scripts --no-progress --no-summary\n"
    );
    assert!(p.resolve("bun").is_some());
    assert_eq!(server.hits("/bun/bun.zip"), 1);
}

#[test]
fn ensure_all_reports_each_tool() {
    let server = Server::start();
    let body = tar_gz(&[("mytool", SCRIPT, 0o755)]);
    let ok = release(&server, "mytool", "m.tar.gz", &body, None);
    let t = tempfile::tempdir().unwrap();
    let p = provisioner(t.path(), vec![uv_spec(&server), ok]);
    let r = p.ensure_all(["uv", "mytool", "ghost"]);
    assert_eq!(r.tools.len(), 2);
    assert!(r.tools.iter().all(|t| t.installed_now));
    assert_eq!(r.notes.len(), 1);
    assert!(p.home().path_dir().join("uv").exists());
    let again = p.ensure_all(["mytool"]);
    assert!(!again.tools[0].installed_now);
}
