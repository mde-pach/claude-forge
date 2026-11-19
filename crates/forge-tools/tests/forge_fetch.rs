//! The launcher's fetcher against a fake release served locally.

mod common;

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use common::{Route, Server, sha256, tar_gz};
use forge_home::Home;
use forge_launcher::Fetcher;
use forge_tools::forge_release::{self, ReleaseFetcher, asset_name};
use forge_tools::fsutil::FileLock;
use forge_tools::{Http, Platform};

const BIN: &[u8] = b"#!/bin/sh\necho forge-fake \"$@\"\n";

/// Serves release `v<version>` for this platform; returns the archive's URL path.
fn serve_release(server: &Server, version: &str, sums_sha: Option<String>) -> String {
    let platform = Platform::current().unwrap();
    let asset = asset_name(version, &platform);
    let dir = asset.trim_end_matches(".tar.gz").to_owned();
    let body = tar_gz(&[
        (&format!("{dir}/claude-forge"), BIN, 0o755),
        (&format!("{dir}/LICENSE"), b"MIT", 0o644),
    ]);
    let sha = sums_sha.unwrap_or_else(|| sha256(&body));
    let sums = format!("{}  other.tar.gz\n{sha}  {asset}\n", "0".repeat(64));
    server.put(
        &format!("/v{version}/SHA256SUMS"),
        Route::Body(sums.into_bytes()),
    );
    let path = format!("/v{version}/{asset}");
    server.put(&path, Route::Body(body));
    path
}

fn fetcher(server: &Server, background: Option<(PathBuf, Vec<OsString>)>) -> ReleaseFetcher {
    ReleaseFetcher::with(
        server.base(),
        Platform::current(),
        Http::allowing_http(),
        background,
    )
}

fn home(t: &tempfile::TempDir) -> Home {
    Home::new(t.path().join("cache"), t.path().join("state"))
}

#[test]
fn fetch_verifies_and_installs() {
    let server = Server::start();
    let path = serve_release(&server, "1.2.3", None);
    let t = tempfile::tempdir().unwrap();
    let h = home(&t);
    let f = fetcher(&server, None);
    let exe = f.fetch(&h, "1.2.3").unwrap();
    assert_eq!(exe, h.forge_binary("1.2.3").unwrap());
    let out = Command::new(&exe).arg("--version").output().unwrap();
    assert_eq!(out.stdout, b"forge-fake --version\n");
    // Cached: no second download.
    f.fetch(&h, "1.2.3").unwrap();
    assert_eq!(server.hits(&path), 1);
    let left: Vec<_> = common::names(&h.bin_dir());
    assert_eq!(left, [".forge-1.2.3.lock", "forge-1.2.3"]);
}

#[test]
fn a_wrong_sum_or_missing_line_installs_nothing() {
    let server = Server::start();
    serve_release(&server, "1.2.3", Some("a".repeat(64)));
    let t = tempfile::tempdir().unwrap();
    let h = home(&t);
    let e = fetcher(&server, None).fetch(&h, "1.2.3").unwrap_err();
    assert!(
        e.to_string().contains("does not match its SHA256SUMS line"),
        "{e}"
    );
    assert!(!h.forge_binary("1.2.3").unwrap().exists());
    assert_eq!(common::names(&h.bin_dir()), [".forge-1.2.3.lock"]);

    server.put("/v2.0.0/SHA256SUMS", Route::Body(b"".to_vec()));
    let e = fetcher(&server, None).fetch(&h, "2.0.0").unwrap_err();
    assert!(
        e.to_string().contains("lists no claude-forge-2.0.0-"),
        "{e}"
    );

    let e = fetcher(&server, None).fetch(&h, "3.0.0").unwrap_err();
    assert!(e.to_string().contains("HTTP 404"), "{e}");

    let e = fetcher(&server, None).fetch(&h, "../x").unwrap_err();
    assert!(e.to_string().contains("invalid version"), "{e}");
}

#[test]
fn busy_lock_skips_the_background_fetch() {
    let server = Server::start();
    serve_release(&server, "1.2.3", None);
    let t = tempfile::tempdir().unwrap();
    let h = home(&t);
    fs::create_dir_all(h.bin_dir()).unwrap();
    let held = FileLock::acquire(&h.bin_dir().join(".forge-1.2.3.lock")).unwrap();
    assert_eq!(
        fetcher(&server, None)
            .fetch_unless_busy(&h, "1.2.3")
            .unwrap(),
        None
    );
    drop(held);
    // A process another test thread is forking can hold a copy of the lock's descriptor
    // until it execs (the descriptor is close-on-exec), so allow a moment.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut got = None;
    while got.is_none() && Instant::now() < deadline {
        got = fetcher(&server, None)
            .fetch_unless_busy(&h, "1.2.3")
            .unwrap();
        if got.is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    assert!(got.is_some());
}

/// The background child: this test binary re-run with only this test selected. It does
/// nothing unless the parent set the fetch environment.
#[test]
fn background_child_entry() {
    if std::env::var_os("FORGE_FETCH_VERSION").is_some() {
        let _ = forge_release::run_background_fetch_with(Http::allowing_http());
    }
}

#[test]
fn background_fetch_returns_at_once_and_installs() {
    let server = Server::start();
    serve_release(&server, "4.5.6", None);
    // Delay the archive so "returns at once" is observable.
    let platform = Platform::current().unwrap();
    let asset = asset_name("4.5.6", &platform);
    let dir = asset.trim_end_matches(".tar.gz").to_owned();
    let body = tar_gz(&[(&format!("{dir}/claude-forge"), BIN, 0o755)]);
    let sums = format!("{}  {asset}\n", sha256(&body));
    server.put("/v4.5.6/SHA256SUMS", Route::Body(sums.into_bytes()));
    server.put(
        &format!("/v4.5.6/{asset}"),
        Route::Slow(body, Duration::from_millis(700)),
    );

    let t = tempfile::tempdir().unwrap();
    let h = home(&t);
    let me = std::env::current_exe().unwrap();
    let args = [
        "background_child_entry",
        "--exact",
        "--test-threads=1",
        "--quiet",
    ]
    .map(OsString::from)
    .to_vec();
    let f = fetcher(&server, Some((me, args)));
    let start = Instant::now();
    f.fetch_in_background(&h, "4.5.6").unwrap();
    assert!(start.elapsed() < Duration::from_millis(500));
    let exe = h.forge_binary("4.5.6").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !exe.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        exe.exists(),
        "the background fetch did not install the binary"
    );
    let out = Command::new(&exe).output().unwrap();
    assert_eq!(out.stdout, b"forge-fake\n");
}

#[test]
fn background_rejects_a_bad_version_before_spawning() {
    let server = Server::start();
    let t = tempfile::tempdir().unwrap();
    let f = fetcher(&server, Some(("/nonexistent".into(), vec![])));
    assert!(f.fetch_in_background(&home(&t), "1.0; rm").is_err());
}
