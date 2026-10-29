//! Downloads wired into the CLI: the launcher's fetcher, and the background child that a
//! session start leaves running to download the pinned Forge version.

use std::process::ExitCode;

use forge_tools::forge_release::{BACKGROUND_ARG, ReleaseFetcher, run_background_fetch};

/// When this process is the background download child (`claude-forge __forge-fetch`), runs
/// it and returns its exit code. Checked before the launcher, so the child never execs
/// another version that would not know the command.
pub fn intercept() -> Option<ExitCode> {
    std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == BACKGROUND_ARG)
        .then(run_background_fetch)
}

/// The fetcher the launcher downloads pinned versions with (GitHub releases, sha256-checked).
pub fn fetcher() -> ReleaseFetcher {
    ReleaseFetcher::new()
}
