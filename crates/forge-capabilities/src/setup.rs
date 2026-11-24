//! This machine's setup of the capability engine (§20.3): the person's consent and the key, in
//! the user-level state (`<state>/user/capabilities/`), never in a repository and never read from
//! the session's environment. The key file is created with mode 0600 in a directory of mode 0700;
//! the agent cannot read it (§9.3 refuses the state root).

use std::path::{Path, PathBuf};

use forge_home::Home;

/// What the person consents to, asked once per machine at `forge apply --user`.
pub const DISCLOSURE: &str = "On each prompt that has capabilities to offer, Forge asks TypeSafe's capability engine (api.typesafe.ai) which ones the task needs. Each request sends the prompt's first 8,000 characters, Claude's last answer's first 2,000, the newest 30 tool calls of the last turn (each as the tool and its file, or a command's program and subcommand) and the capabilities' relevance statements; never a file's content, a command's arguments or a tool's output.";

/// The version of [`DISCLOSURE`] a consent was given to: a new text asks again.
pub const DISCLOSURE_VERSION: u32 = 1;

/// `FORGE_CAPABILITIES=off` turns the whole thing off.
pub const OFF_VAR: &str = "FORGE_CAPABILITIES";

fn dir(home: &Home) -> PathBuf {
    home.state_root().join("user").join("capabilities")
}

fn key_file(home: &Home) -> PathBuf {
    dir(home).join("key")
}

fn consent_file(home: &Home) -> PathBuf {
    dir(home).join("consent")
}

/// This machine's state of the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Consented, with a key: the engine is asked.
    On,
    /// The person did not consent here (or to an older text).
    NoConsent,
    NoKey,
}

impl State {
    /// Why capabilities are off here, as `forge status` says it.
    pub fn why_off(&self) -> Option<&'static str> {
        match self {
            State::On => None,
            State::NoConsent => Some(
                "capabilities are off on this machine: nobody consented to the capability engine (forge apply --user asks)",
            ),
            State::NoKey => Some(
                "capabilities are off on this machine: the capability engine has no key (forge apply --user asks for it)",
            ),
        }
    }
}

fn consented(home: &Home) -> bool {
    std::fs::read_to_string(consent_file(home))
        .ok()
        .and_then(|t| t.lines().next().map(str::trim).map(str::to_string))
        .and_then(|v| v.parse::<u32>().ok())
        == Some(DISCLOSURE_VERSION)
}

/// The key, when one is stored.
pub fn key(home: &Home) -> Option<String> {
    std::fs::read_to_string(key_file(home))
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

pub fn state(home: &Home) -> State {
    if !consented(home) {
        State::NoConsent
    } else if key(home).is_none() {
        State::NoKey
    } else {
        State::On
    }
}

/// Whether the person consented to the current text on this machine.
pub fn has_consent(home: &Home) -> bool {
    consented(home)
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(d) = path.parent()
        && !crate::private_dir(d)
    {
        return Err(std::io::Error::other(format!(
            "cannot create {}",
            d.display()
        )));
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = crate::private_file(&tmp, false)?;
        f.write_all(text.as_bytes())?;
    }
    std::fs::rename(&tmp, path)
}

/// Records the person's consent to [`DISCLOSURE`].
pub fn record_consent(home: &Home) -> std::io::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    write_private(
        &consent_file(home),
        &format!("{DISCLOSURE_VERSION}\nconsented at {now}\n{DISCLOSURE}\n"),
    )
}

/// Stores the key (mode 0600).
pub fn store_key(home: &Home, key: &str) -> std::io::Result<()> {
    write_private(&key_file(home), &format!("{}\n", key.trim()))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    // Off until consented and keyed; the key and consent are private files of the user-level
    // state; a consent to another text asks again.
    #[test]
    fn consent_then_key() {
        let d = tempfile::tempdir().unwrap();
        let home = Home::new(d.path().join("c"), d.path().join("s"));
        assert_eq!(state(&home), State::NoConsent);
        store_key(&home, "  sk-1\n").unwrap();
        assert_eq!(
            state(&home),
            State::NoConsent,
            "a key without consent is off"
        );
        record_consent(&home).unwrap();
        assert_eq!(state(&home), State::On);
        assert_eq!(key(&home).as_deref(), Some("sk-1"));
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&key_file(&home)), 0o600);
        assert_eq!(mode(&dir(&home)), 0o700);
        assert!(key_file(&home).starts_with(d.path().join("s/user")));
        std::fs::write(consent_file(&home), "0\n").unwrap();
        assert_eq!(state(&home), State::NoConsent);
        record_consent(&home).unwrap();
        std::fs::remove_file(key_file(&home)).unwrap();
        assert_eq!(state(&home), State::NoKey);
        assert!(State::NoKey.why_off().unwrap().contains("no key"));
    }
}
