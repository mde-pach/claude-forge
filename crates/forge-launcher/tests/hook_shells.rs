//! Runs the hook entry command under every available POSIX shell, with and
//! without `claude-forge` on PATH.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use forge_launcher::hook::{NOT_INSTALLED_JSON, hook_script, hook_wrapper};

const BASE_PATH: &str = "/usr/bin:/bin";

fn shells() -> Vec<Vec<&'static str>> {
    let mut v = vec![vec!["/bin/sh"]];
    for (bin, extra) in [("dash", None), ("bash", Some("--posix"))] {
        for dir in ["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
            let p: &'static str = Box::leak(format!("{dir}/{bin}").into_boxed_str());
            if Path::new(p).exists() {
                let mut s = vec![p];
                s.extend(extra);
                v.push(s);
                break;
            }
        }
    }
    v
}

fn run(shell: &[&str], script: &str, path: &str) -> (i32, String, String) {
    let mut child = Command::new(shell[0])
        .args(&shell[1..])
        .arg("-c")
        .arg(script)
        .env_clear()
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Claude Code writes the event JSON to stdin.
    let _ = child.stdin.take().unwrap().write_all(b"{\"hook\":1}");
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn hook_entries_in_each_shell() {
    let t = tempfile::tempdir().unwrap();
    let fake = t.path().join("claude-forge");
    std::fs::write(&fake, "#!/bin/sh\nprintf 'ran %s\\n' \"$*\"; cat; exit 3\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let with = format!("{}:{BASE_PATH}", t.path().display());
    assert!(
        run(&["/bin/sh"], "command -v claude-forge", BASE_PATH).0 != 0,
        "claude-forge must not be installed in {BASE_PATH} for this test"
    );

    let shells = shells();
    assert!(
        shells.len() >= 2,
        "need at least two shells, found {shells:?}"
    );
    for shell in &shells {
        // Both the full `sh -c '...'` entry (parsed by this shell) and the
        // inner script (executed by this shell).
        for event in ["session-start", "pre-tool-use", "post-tool-use", "stop"] {
            for script in [hook_wrapper(event), hook_script(event)] {
                let ctx = format!("{shell:?} {script}");

                let (code, out, err) = run(shell, &script, BASE_PATH);
                assert_eq!(code, 0, "{ctx}: {err}");
                assert_eq!(err, "", "{ctx}");
                if event == "session-start" {
                    assert_eq!(out, format!("{NOT_INSTALLED_JSON}\n"), "{ctx}");
                } else {
                    assert_eq!(out, "", "{ctx}");
                }

                let (code, out, err) = run(shell, &script, &with);
                assert_eq!(code, 3, "{ctx}: exit status passes through; {err}");
                assert_eq!(out, format!("ran hook {event}\n{{\"hook\":1}}"), "{ctx}");
            }
        }
    }
}
