//! The CLAUDE_ENV_FILE line, sourced by a real POSIX shell.

use std::fs;
use std::path::Path;
use std::process::Command;

use forge_tools::append_path_export;

/// Sources `file` in `sh` with `PATH=<path>` and prints the resulting PATH.
fn path_after_sourcing(shell: &str, file: &Path, path: &str) -> String {
    let out = Command::new(shell)
        .args(["-c", ". \"$1\"; printf %s \"$PATH\"", "sh"])
        .arg(file)
        .env("PATH", path)
        .env_remove("ENV")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn appended_once_and_sourced_by_sh() {
    let t = tempfile::tempdir().unwrap();
    let env = t.path().join("env.sh");
    // A path with a space, a quote and a dollar sign: none may be interpreted.
    let dir = t.path().join("it's a $HOME dir/path");
    fs::write(&env, "export FOO=1").unwrap();

    assert!(append_path_export(&env, &dir).unwrap());
    assert!(!append_path_export(&env, &dir).unwrap());
    let text = fs::read_to_string(&env).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.starts_with("export FOO=1\nexport PATH="));

    let d = dir.to_str().unwrap();
    for shell in ["/bin/sh", "/bin/bash", "/bin/dash"] {
        if !Path::new(shell).exists() {
            continue;
        }
        assert_eq!(
            path_after_sourcing(shell, &env, "/usr/bin:/bin"),
            format!("{d}:/usr/bin:/bin"),
            "{shell}"
        );
        // An empty PATH gets no trailing ':' (which would mean the current directory).
        assert_eq!(path_after_sourcing(shell, &env, ""), d, "{shell}");
    }
}

#[test]
fn creates_the_file() {
    let t = tempfile::tempdir().unwrap();
    let env = t.path().join("new.sh");
    assert!(append_path_export(&env, Path::new("/c/path")).unwrap());
    assert_eq!(
        fs::read_to_string(&env).unwrap(),
        "export PATH='/c/path'\"${PATH:+:$PATH}\"\n"
    );
}
