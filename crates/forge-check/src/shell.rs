//! Running a check's shell command: `sh -c` with a preamble that defines each builtin as a shell
//! function calling `<forge executable> internal <name> "$@"`, the project tools the command
//! names as wrappers of `internal exec_tool`, and the context in the environment.
//!
//! A command runs in its own process group with its output (stdout and stderr, interleaved as
//! they come) in one file; past its time limit the whole group is stopped.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

/// Every builtin the packs' check commands call (`grep -rhoE "forge_[a-z_]+" packs | sort -u`),
/// and the ones the engine itself provides to commands.
pub const BUILTIN_NAMES: &[&str] = &[
    "accept_hint",
    "actionlint",
    "adr_edits",
    "advise",
    "alembic_batch",
    "alembic_data_downgrade",
    "alembic_files",
    "alembic_fk_index",
    "applied_migrations",
    "artifacts",
    "bun_cooldown",
    "chart_version",
    "ci_jobs",
    "credential_files",
    "db_url",
    "dep_major_match",
    "do",
    "docker_copy_order",
    "docker_ignore",
    "docker_locked",
    "docker_nonroot",
    "docker_secrets",
    "docs_mentions",
    "drizzle_applied",
    "drizzle_migration_present",
    "drizzle_squawk",
    "editorconfig",
    "env_example",
    "env_files",
    "exec_tool",
    "generated_edits",
    "gitleaks",
    "hadolint",
    "helm",
    "helper",
    "kube_linter",
    "kustomize",
    "large_files",
    "local_db",
    "lockfile_lint",
    "lockfile_updated",
    "model_ids",
    "models_registered",
    "oasdiff",
    "once",
    "openapi_lint",
    "openapi_ratchet",
    "openapi_sync",
    "osv",
    "pinned",
    "pkg_deps",
    "prisma_drops",
    "ratchet",
    "related_py_tests",
    "release_notes",
    "role",
    "role_files",
    "scope_files",
    "snapshot_stale",
    "sqlfluff",
    "squawk_alembic",
    "squawk_files",
    "squawk_role",
    "state_file",
    "stub_sync",
    "terraform",
    "test_edits",
    "tf_validate",
    "tflint",
    "toml_get",
    "trivy_config",
    "trusted_deps",
    "ty",
    "typeorm_pending",
    "untested_change",
    "uv_cooldown",
    "version_changed",
    "zizmor",
];

/// Forge's one release cooldown (3 days, in seconds), which commands pass to helpers.
pub const COOLDOWN_SECONDS: u64 = 259_200;

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The preamble of a check command: builtins, tool wrappers, the check's id.
pub fn preamble(rule: &str, tools: &[String]) -> String {
    let mut out = String::from("IFS='\n'\nset -f\n");
    for n in BUILTIN_NAMES {
        out.push_str(&format!(
            "forge_{n}() {{ \"$FORGE_EXE\" internal {n} \"$@\"; }}\n"
        ));
    }
    for t in tools {
        // a POSIX function name has no "-": those tools are run as `forge_exec_tool <name>`
        if t.contains('-') {
            continue;
        }
        match t.as_str() {
            "ty" => out.push_str("ty() { forge_ty \"$@\"; }\n"),
            "terraform" => out.push_str("terraform() { forge_terraform \"$@\"; }\n"),
            "kustomize" => out.push_str("kustomize() { forge_kustomize \"$@\"; }\n"),
            t if crate::tools::PROJECT_TOOLS.contains(&t) => {
                out.push_str(&format!("{t}() {{ forge_exec_tool {t} \"$@\"; }}\n"))
            }
            _ => {}
        }
    }
    out.push_str(&format!("c_id={}\n", sh_quote(rule)));
    out.push_str(&format!("FORGE_COOLDOWN_SECONDS={COOLDOWN_SECONDS}\n"));
    out
}

/// A check command with `{files}` and `{file}` filled (`$FORGE_FILES`, `"$FORGE_FILE"`).
pub fn fill(cmd: &str) -> String {
    cmd.replace("{files}", "$FORGE_FILES")
        .replace("{file}", "\"$FORGE_FILE\"")
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    Status(i32),
    /// Stopped at its own time limit (seconds).
    TimedOut(u64),
    /// Stopped because the run's time budget ran out (seconds it had).
    Budget(u64),
    /// Could not start.
    Failed(String),
}

impl Exit {
    pub fn ok(&self) -> bool {
        *self == Exit::Status(0)
    }
}

#[derive(Debug, Clone)]
pub struct Run {
    pub exit: Exit,
    /// stdout and stderr, as they came.
    pub output: String,
    pub took: Duration,
}

/// What a command runs with.
#[derive(Debug, Clone, Default)]
pub struct Spec {
    pub script: String,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, OsString>,
    /// Bytes for stdin (a format filter's input); None: no stdin.
    pub stdin: Option<Vec<u8>>,
    /// The check's own limit.
    pub timeout: Option<Duration>,
    /// What is left of the run's budget.
    pub budget: Option<Duration>,
}

/// Runs `sh -c <script>`.
pub fn run(spec: &Spec) -> Run {
    let started = Instant::now();
    let (limit, by_budget) = match (spec.timeout, spec.budget) {
        (Some(t), Some(b)) if b < t => (Some(b), true),
        (None, Some(b)) => (Some(b), true),
        (t, _) => (t, false),
    };
    if limit.is_some_and(|l| l.is_zero()) {
        return Run {
            exit: Exit::Budget(0),
            output: String::new(),
            took: Duration::ZERO,
        };
    }
    let out = match tempfile::tempfile() {
        Ok(f) => f,
        Err(e) => {
            return Run {
                exit: Exit::Failed(e.to_string()),
                output: String::new(),
                took: Duration::ZERO,
            };
        }
    };
    let err = match out.try_clone() {
        Ok(f) => f,
        Err(e) => {
            return Run {
                exit: Exit::Failed(e.to_string()),
                output: String::new(),
                took: Duration::ZERO,
            };
        }
    };
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(&spec.script)
        .current_dir(&spec.cwd)
        .envs(&spec.env)
        .stdout(Stdio::from(out.try_clone().expect("clone")))
        .stderr(Stdio::from(err))
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    // its own process group, so a time limit stops the whole tree
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Run {
                exit: Exit::Failed(e.to_string()),
                output: String::new(),
                took: Duration::ZERO,
            };
        }
    };
    if let (Some(input), Some(mut sin)) = (spec.stdin.clone(), child.stdin.take()) {
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = sin.write_all(&input);
        });
    }
    let pid = child.id() as i32;
    let exit = match limit {
        Some(l) => match child.wait_timeout(l) {
            Ok(Some(st)) => Exit::Status(status_code(st)),
            Ok(None) => {
                kill_group(pid);
                let _ = child.wait();
                if by_budget {
                    Exit::Budget(l.as_secs())
                } else {
                    Exit::TimedOut(l.as_secs())
                }
            }
            Err(e) => Exit::Failed(e.to_string()),
        },
        None => match child.wait() {
            Ok(st) => Exit::Status(status_code(st)),
            Err(e) => Exit::Failed(e.to_string()),
        },
    };
    // what is left of the group (a background job of the command) goes too
    kill_group(pid);
    let mut text = Vec::new();
    let mut f = out;
    use std::io::Seek;
    let _ = f.seek(std::io::SeekFrom::Start(0));
    let _ = f.read_to_end(&mut text);
    Run {
        exit,
        output: String::from_utf8_lossy(&text).into_owned(),
        took: started.elapsed(),
    }
}

fn status_code(st: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(9))
}

fn kill_group(pid: i32) {
    unsafe {
        libc::killpg(pid, libc::SIGTERM);
    }
    std::thread::sleep(Duration::from_millis(5));
    unsafe {
        libc::killpg(pid, libc::SIGKILL);
    }
}

/// The environment of commands: the context variables and the Forge executable.
pub fn base_env(exe: &Path) -> BTreeMap<String, OsString> {
    let mut env = BTreeMap::new();
    env.insert(crate::ENV_EXE.into(), exe.as_os_str().to_owned());
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_times_out() {
        let r = run(&Spec {
            script: "echo out; echo err >&2; exit 3".into(),
            cwd: std::env::temp_dir(),
            ..Spec::default()
        });
        assert_eq!(r.exit, Exit::Status(3));
        assert_eq!(r.output, "out\nerr\n");
        let r = run(&Spec {
            script: "sleep 5 & sleep 5".into(),
            cwd: std::env::temp_dir(),
            timeout: Some(Duration::from_millis(200)),
            ..Spec::default()
        });
        assert_eq!(r.exit, Exit::TimedOut(0));
        assert!(r.took < Duration::from_secs(2));
        let r = run(&Spec {
            script: "sleep 5".into(),
            cwd: std::env::temp_dir(),
            timeout: Some(Duration::from_secs(10)),
            budget: Some(Duration::from_millis(100)),
            ..Spec::default()
        });
        assert_eq!(r.exit, Exit::Budget(0));
    }

    #[test]
    fn preamble_defines_builtins() {
        let p = preamble("x/y", &["ruff".into(), "vue-tsc".into()]);
        assert!(p.contains("forge_do() { \"$FORGE_EXE\" internal do \"$@\"; }"));
        assert!(p.contains("ruff() { forge_exec_tool ruff \"$@\"; }"));
        assert!(!p.contains("vue-tsc()"));
        assert!(p.contains("c_id='x/y'"));
    }
}
