//! What to do when the locked Forge version is not in the cache.
//!
//! Downloads are slow and can fail, so they only happen where a person is
//! waiting anyway: at session start (in the background, so the hook still
//! returns at once) and in `forge apply`. Every other hook runs on the hot
//! path of the agent's tool calls; there we never download and instead run
//! the installed version, leaving a note for the person. Running the
//! installed version beats doing nothing: the checks stay on, possibly with
//! a slightly different pack set until the download lands.

use std::ffi::OsString;

/// One invocation of the binary, as far as the launcher cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// `forge hook <event>`, run by Claude Code.
    Hook(String),
    /// `forge <command> ...`, run by a person or CI. Empty when no argument.
    Command(String),
}

impl Invocation {
    /// Classifies a full argv (`argv[0]` included).
    pub fn from_args(args: &[OsString]) -> Invocation {
        let word = |i: usize| {
            args.get(i)
                .map(|a| a.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        match word(1).as_str() {
            "hook" => Invocation::Hook(word(2)),
            other => Invocation::Command(other.to_owned()),
        }
    }
}

/// The action for a locked version that is not in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingAction {
    /// Start a detached download, then run the installed version now.
    FetchInBackground,
    /// Download now (a person is waiting for it), then exec the locked version.
    FetchThenExec,
    /// Run the installed version and leave a note; never download.
    RunInstalled,
}

/// Pure policy: which invocations may download.
pub fn on_missing(inv: &Invocation) -> MissingAction {
    match inv {
        Invocation::Hook(e) if e == "session-start" => MissingAction::FetchInBackground,
        Invocation::Command(c) if c == "apply" => MissingAction::FetchThenExec,
        _ => MissingAction::RunInstalled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv(args: &[&str]) -> Invocation {
        let v: Vec<OsString> = args.iter().map(OsString::from).collect();
        Invocation::from_args(&v)
    }

    #[test]
    fn classifies_argv() {
        assert_eq!(
            inv(&["claude-forge", "hook", "session-start"]),
            Invocation::Hook("session-start".into())
        );
        assert_eq!(inv(&["forge", "hook"]), Invocation::Hook(String::new()));
        assert_eq!(
            inv(&["forge", "apply", "--yes"]),
            Invocation::Command("apply".into())
        );
        assert_eq!(inv(&["forge"]), Invocation::Command(String::new()));
    }

    #[test]
    fn only_session_start_and_apply_download() {
        use MissingAction::*;
        let cases = [
            (&["f", "hook", "session-start"][..], FetchInBackground),
            (&["f", "apply"], FetchThenExec),
            (&["f", "hook", "pre-tool-use"], RunInstalled),
            (&["f", "hook", "post-tool-use"], RunInstalled),
            (&["f", "hook", "stop"], RunInstalled),
            (&["f", "hook", "apply"], RunInstalled),
            (&["f", "session-start"], RunInstalled),
            (&["f", "status"], RunInstalled),
            (&["f"], RunInstalled),
        ];
        for (args, want) in cases {
            assert_eq!(on_missing(&inv(args)), want, "{args:?}");
        }
    }
}
