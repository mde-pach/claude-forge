//! The command written into `.claude/settings.json` hook entries.
//!
//! It calls `claude-forge` (not `forge`, which Foundry also installs) and
//! must behave when Forge is not installed on a teammate's machine:
//!
//! * `session-start` prints a `systemMessage` telling the person how to
//!   install Forge, and exits 0;
//! * every other event exits 0 silently, so the agent sees nothing.
//!
//! The entry is `sh -c '<script>'` so it means the same whatever shell
//! Claude Code uses. The script is POSIX (tested with sh, dash and
//! `bash --posix`), uses only builtins on the missing path, and contains no
//! single quotes or `\\`, so it is also safe inside fish single quotes.

/// The shell script, without the `sh -c` wrapper.
///
/// # Panics
/// If `event` is not lower-case kebab (`[a-z][a-z0-9-]*`). Events are
/// constants in the CLI, so a bad one is a programming error.
pub fn hook_script(event: &str) -> String {
    let ok = event.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
        && event
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
    assert!(ok, "invalid hook event name: {event:?}");
    let probe = "command -v claude-forge >/dev/null 2>&1";
    if event == "session-start" {
        // \047 is printf's octal escape for the apostrophe in "project's".
        format!(
            "{probe} || {{ printf \"{}\\n\"; exit 0; }}; exec claude-forge hook {event}",
            NOT_INSTALLED_JSON
                .replace('"', "\\\"")
                .replace('\'', "\\047")
        )
    } else {
        format!("{probe} || exit 0; exec claude-forge hook {event}")
    }
}

/// The exact command string for a hook entry: `sh -c '<script>'`.
pub fn hook_wrapper(event: &str) -> String {
    format!("sh -c '{}'", hook_script(event))
}

/// What `session-start` prints when Forge is not installed.
pub const NOT_INSTALLED_JSON: &str = r#"{"systemMessage":"Forge is not installed, so this project's checks are off. Install it with: curl -LsSf https://github.com/mde-pach/claude-forge/releases/latest/download/install.sh | sh"}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_strings() {
        assert_eq!(
            hook_wrapper("pre-tool-use"),
            "sh -c 'command -v claude-forge >/dev/null 2>&1 || exit 0; exec claude-forge hook pre-tool-use'"
        );
        let s = hook_wrapper("session-start");
        assert!(s.starts_with(
            "sh -c 'command -v claude-forge >/dev/null 2>&1 || { printf \"{\\\"systemMessage\\\":"
        ));
        assert!(s.ends_with("; exit 0; }; exec claude-forge hook session-start'"));
        // Only the two wrapping quotes; no backslash-backslash.
        assert_eq!(s.matches('\'').count(), 2);
        assert!(!s.contains("\\\\"));
    }

    #[test]
    #[should_panic(expected = "invalid hook event")]
    fn rejects_unsafe_event() {
        hook_wrapper("x; rm -rf ~");
    }
}
