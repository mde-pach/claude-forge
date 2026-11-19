//! Opt-in, end to end (§7.2): the sandbox entries `forge apply` writes for a project whose tests
//! use Testcontainers let Claude's test runs reach Docker on this machine. A project with a
//! Testcontainers Postgres test in Python (pytest) and in Node (`npm test`, node:test) gets
//! `forge apply --yes`; then each test command runs as Claude Code runs it with the
//! `.claude/settings.local.json` apply wrote, inside Claude Code's sandbox runtime (`srt`, from
//! `@anthropic-ai/sandbox-runtime`, the sandbox Claude Code runs commands in) with the entries'
//! `network` settings. On macOS each must pass, and fail without those entries, so the entries
//! are what makes it pass. On Linux apply writes nothing and no sandbox setting can do it (each
//! sandboxed command has a network namespace of its own): the test proves that limit, which
//! `forge apply` states in a note, by checking that each run fails, even with every Unix socket
//! allowed.
//!
//! The test runs use this machine's own Docker configuration (`~/.docker/config.json`): when it
//! names a credential helper (`credsStore`, `credHelpers`), Testcontainers runs it when it pulls
//! the image. The image (`postgres:17-alpine`, or `FORGE_TEST_SANDBOX_IMAGE`) is pulled by each
//! language's run when it is not on the machine, and removed again at the end.
//!
//! Run: `FORGE_TEST_SANDBOX=1 cargo test -p forge-cli --test sandbox_testcontainers -- --nocapture`
//!
//! Needs a running Docker daemon and the `docker` CLI, `srt` on PATH
//! (`npm install -g @anthropic-ai/sandbox-runtime`; on Linux also `bwrap`, `socat` and `rg`),
//! `uv`, `node` 22.12+ and `npm`, and network access to PyPI, the npm registry and Docker Hub.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const PYPROJECT: &str = r#"[project]
name = "orders"
version = "0.1.0"
requires-python = ">=3.10"

[dependency-groups]
dev = ["pytest>=8.3", "testcontainers[postgres]>=4.10", "pg8000>=1.31"]
"#;

const PY_TEST: &str = r#"import os

import pg8000.native
from testcontainers.postgres import PostgresContainer


def test_postgres_answers():
    with PostgresContainer(os.environ["FORGE_TEST_IMAGE"], driver=None) as pg:
        con = pg8000.native.Connection(
            user=pg.username,
            password=pg.password,
            host=pg.get_container_host_ip(),
            port=int(pg.get_exposed_port(5432)),
            database=pg.dbname,
        )
        assert con.run("select 41 + 1")[0][0] == 42
        con.close()
        print("PY-OK sandboxed=" + os.environ.get("SANDBOX_RUNTIME", "0"))
"#;

const PACKAGE_JSON: &str = r#"{
  "name": "orders",
  "private": true,
  "type": "module",
  "scripts": {"test": "node --test --test-reporter=spec"},
  "devDependencies": {"testcontainers": "^11.0.0", "@testcontainers/postgresql": "^11.0.0", "pg": "^8.13.0"}
}
"#;

const NODE_TEST: &str = r#"import { test } from "node:test";
import assert from "node:assert/strict";
import { PostgreSqlContainer } from "@testcontainers/postgresql";
import pg from "pg";

test("postgres answers", async () => {
  const c = await new PostgreSqlContainer(process.env.FORGE_TEST_IMAGE).start();
  const client = new pg.Client({ connectionString: c.getConnectionUri() });
  await client.connect();
  const r = await client.query("select 41 + 1 as n");
  assert.equal(r.rows[0].n, 42);
  await client.end();
  await c.stop();
  console.log("NODE-OK sandboxed=" + (process.env.SANDBOX_RUNTIME ?? "0"));
});
"#;

fn enabled() -> bool {
    std::env::var_os("FORGE_TEST_SANDBOX").is_some_and(|v| v == "1")
}

fn run(cmd: &mut Command) -> Output {
    let out = cmd.stdin(Stdio::null()).output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// srt's settings for Claude Code's `sandbox` settings: the `network` entries as they are (a
/// `~/` path under the home directory), no domain allowed (images are pulled by the daemon), and
/// writes to the project and the temporary directory, as Claude Code allows them.
fn srt_settings(sandbox: &Value, project: &Path) -> Value {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut network = json!({"allowedDomains": [], "deniedDomains": []});
    if let Some(n) = sandbox.get("network").and_then(Value::as_object) {
        for (k, v) in n {
            let v = match v {
                Value::Array(items) => Value::Array(
                    items
                        .iter()
                        .map(|i| match i.as_str().and_then(|s| s.strip_prefix("~/")) {
                            Some(rest) => Value::String(format!("{home}/{rest}")),
                            None => i.clone(),
                        })
                        .collect(),
                ),
                v => v.clone(),
            };
            network[k] = v;
        }
    }
    json!({
        "network": network,
        "filesystem": {
            "denyRead": [],
            "allowWrite": [project, std::env::temp_dir(), "/tmp"],
            "denyWrite": [],
        },
    })
}

fn image_present(image: &str) -> bool {
    Command::new("docker")
        .args(["image", "inspect", image])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn remove_image(image: &str) {
    let _ = Command::new("docker")
        .args(["rmi", "-f", image])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn in_srt(cmd: &str, sandbox: &Value, project: &Path, env: &[(&str, String)]) -> Output {
    let settings = project.join(".srt-settings.json");
    std::fs::write(
        &settings,
        serde_json::to_string_pretty(&srt_settings(sandbox, project)).unwrap(),
    )
    .unwrap();
    eprintln!(
        "[sandbox e2e] {cmd}: in srt with {}",
        serde_json::to_string(&srt_settings(sandbox, project)["network"]).unwrap()
    );
    Command::new("srt")
        .arg("-s")
        .arg(&settings)
        .args(["-c", cmd])
        .current_dir(project)
        .envs(env.iter().map(|(k, v)| (*k, v)))
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn testcontainers_tests_pass_with_the_sandbox_entries_apply_writes() {
    if !enabled() {
        eprintln!("skipped: set FORGE_TEST_SANDBOX=1 to run (see the file's doc comment)");
        return;
    }
    let image = std::env::var("FORGE_TEST_SANDBOX_IMAGE")
        .unwrap_or_else(|_| "postgres:17-alpine".to_string());
    let had_image = image_present(&image);
    let socket = forge_core::docker::socket(&forge_core::docker::Env::current())
        .expect("a Docker socket on this machine");
    // srt's commands get TMPDIR=/tmp/claude, which Claude Code creates
    std::fs::create_dir_all("/tmp/claude").unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("orders");
    std::fs::create_dir_all(project.join("tests")).unwrap();
    std::fs::create_dir_all(project.join("test")).unwrap();
    std::fs::write(project.join("pyproject.toml"), PYPROJECT).unwrap();
    std::fs::write(project.join("tests/test_db.py"), PY_TEST).unwrap();
    std::fs::write(project.join("package.json"), PACKAGE_JSON).unwrap();
    std::fs::write(project.join("test/db.test.mjs"), NODE_TEST).unwrap();
    run(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&project)
        .env("GIT_CONFIG_NOSYSTEM", "1"));

    // forge apply, with private homes; the socket this machine's tests use, by DOCKER_HOST
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("claude"),
        "#!/bin/sh\necho \"2.1.323 (Claude Code)\"\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let apply = run(Command::new(env!("CARGO_BIN_EXE_forge"))
        .args(["apply", "--yes"])
        .current_dir(&project)
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("HOME", &home)
        .env("FORGE_HOME", tmp.path().join("forge-home"))
        .env("FORGE_NO_DOWNLOAD", "1")
        .env("DOCKER_HOST", format!("unix://{}", socket.path.display()))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_PROJECT_DIR"));
    eprintln!("[sandbox e2e] forge apply:\n{}", text(&apply));
    let local: Value = serde_json::from_str(
        &std::fs::read_to_string(project.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    let sandbox = local["sandbox"].clone();
    eprintln!("[sandbox e2e] .claude/settings.local.json sandbox: {sandbox}");

    // the project's own dependencies, outside any sandbox
    run(Command::new("uv")
        .args(["sync", "-q"])
        .current_dir(&project));
    run(Command::new("npm")
        .args(["install", "--no-audit", "--no-fund", "--loglevel=error"])
        .current_dir(&project));

    let path = format!(
        "{}:{}",
        project.join(".venv/bin").display(),
        std::env::var("PATH").unwrap()
    );
    let env = [("PATH", path), ("FORGE_TEST_IMAGE", image.clone())];
    let bare = json!({"enabled": true});
    for (cmd, ok) in [("pytest -q -s tests", "PY-OK"), ("npm test", "NODE-OK")] {
        if !had_image {
            remove_image(&image);
        }
        if cfg!(target_os = "linux") {
            // no sandbox setting reaches the ports the containers publish on the host's
            // loopback: not the entries apply wrote (none), not every Unix socket allowed
            for (what, settings) in [
                ("apply's settings", sandbox.clone()),
                (
                    "every Unix socket allowed",
                    json!({"network": {"allowAllUnixSockets": true}}),
                ),
            ] {
                let out = in_srt(cmd, &settings, &project, &env);
                assert!(
                    !out.status.success(),
                    "{cmd} passed in the Linux sandbox with {what}:\n{}",
                    text(&out)
                );
                eprintln!(
                    "[sandbox e2e] {cmd}: fails in the Linux sandbox with {what}, as expected"
                );
            }
            continue;
        }
        let out = in_srt(cmd, &sandbox, &project, &env);
        let t = text(&out);
        assert!(out.status.success() && t.contains(ok), "{cmd}:\n{t}");
        let line = t
            .lines()
            .find(|l| l.contains(ok))
            .unwrap_or_default()
            .trim();
        eprintln!("[sandbox e2e] {cmd}: passed ({line})");
        // the same command in the sandbox without those entries fails
        let out = in_srt(cmd, &bare, &project, &env);
        assert!(
            !out.status.success(),
            "{cmd} passed in the sandbox without the entries:\n{}",
            text(&out)
        );
        eprintln!("[sandbox e2e] {cmd}: fails in the sandbox without the entries, as expected");
    }
    if !had_image {
        remove_image(&image);
    }
}
