//! The capability engine's setup on this machine (§20.3), at `forge apply --user`: the person's
//! consent, asked once, naming what is sent; then the key. The key comes from `TYPESAFE_API_KEY`
//! in apply's own environment (the variable TypeSafe's documentation suggests), else it is typed
//! at the terminal without echo. It is stored in the user-level state (mode 0600); hooks never read
//! it from the session's environment.

use std::io::{BufRead, IsTerminal, Write};

use forge_capabilities::setup;
use forge_home::Home;

use super::Flags;
use super::io::Error;

/// The variable apply reads the key from.
pub const KEY_VAR: &str = "TYPESAFE_API_KEY";

/// Reads one line from the terminal with its echo off.
fn read_hidden() -> Option<String> {
    let fd = libc::STDIN_FILENO;
    // SAFETY: termios is plain data; tcgetattr fills it, and the saved copy is restored.
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    let saved = (unsafe { libc::tcgetattr(fd, &mut t) } == 0).then_some(t);
    if let Some(mut quiet) = saved {
        quiet.c_lflag &= !libc::ECHO;
        // SAFETY: a termios read from this terminal, with ECHO cleared.
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &quiet) };
    }
    let mut line = String::new();
    let ok = std::io::stdin().lock().read_line(&mut line).is_ok();
    if let Some(t) = saved {
        // SAFETY: restores the terminal's own settings.
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &t) };
    }
    println!();
    ok.then(|| line.trim().to_string())
        .filter(|k| !k.is_empty())
}

/// The person at the terminal, as the setup talks to them: what they answer, and what they type
/// as the key (unechoed). `None`: no terminal.
pub trait Person {
    fn answer(&mut self, question: &str) -> String;
    fn secret(&mut self, question: &str) -> Option<String>;
}

/// The terminal on stdin, when there is one.
struct Terminal;

impl Person for Terminal {
    fn answer(&mut self, question: &str) -> String {
        print!("{question}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        line
    }

    fn secret(&mut self, question: &str) -> Option<String> {
        print!("{question}");
        let _ = std::io::stdout().flush();
        read_hidden()
    }
}

/// Asks for consent when this machine has none, then for the key when there is none (or
/// `TYPESAFE_API_KEY` names another). Declined, or without a terminal and without
/// `--capability-engine`: capabilities stay off here, and apply goes on.
pub fn offer(flags: &Flags) -> Result<(), Error> {
    if flags.dry_run {
        return Ok(());
    }
    let home = Home::from_env().map_err(|e| Error::new(format!("forge: {e}")))?;
    let env_key = std::env::var(KEY_VAR).ok();
    let mut terminal = Terminal;
    let person: Option<&mut dyn Person> = if std::io::stdin().is_terminal() {
        Some(&mut terminal)
    } else {
        None
    };
    for line in setup_engine(&home, flags.capability_engine, env_key.as_deref(), person)? {
        println!("{line}");
    }
    Ok(())
}

/// The setup itself: the lines it prints for the person, in order.
pub fn setup_engine(
    home: &Home,
    consent_flag: bool,
    env_key: Option<&str>,
    mut person: Option<&mut dyn Person>,
) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    if !setup::has_consent(home) {
        out.push(format!("forge: capabilities: {}", setup::DISCLOSURE));
        let yes = consent_flag
            || person.as_deref_mut().is_some_and(|p| {
                matches!(
                    p.answer("forge: send this to the capability engine on this machine? [y/N] ")
                        .trim()
                        .to_lowercase()
                        .as_str(),
                    "y" | "yes"
                )
            });
        if !yes {
            out.push("forge: capabilities stay off on this machine (re-run forge apply --user, or add --capability-engine, to consent)".into());
            return Ok(out);
        }
        setup::record_consent(home)
            .map_err(|e| Error::new(format!("forge: cannot record the consent: {e}")))?;
        out.push("forge: consent to the capability engine recorded on this machine".into());
    }
    let from_env = env_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    let stored = setup::key(home);
    let key = match (from_env, &stored) {
        (Some(k), Some(s)) if k == *s => return Ok(out),
        (Some(k), _) => Some(k),
        (None, Some(_)) => return Ok(out),
        (None, None) => person.and_then(|p| {
            p.secret("forge: the capability engine's API key (https://console.typesafe.ai/keys; not echoed): ")
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
        }),
    };
    let Some(key) = key else {
        out.push(format!(
            "forge: capabilities stay off on this machine: no key (set {KEY_VAR} and re-run forge apply --user)"
        ));
        return Ok(out);
    };
    setup::store_key(home, &key)
        .map_err(|e| Error::new(format!("forge: cannot store the key: {e}")))?;
    out.push("forge: the capability engine's key is stored for this machine".into());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A person who answers `answer` and types `key`, recording the questions.
    struct Scripted {
        answer: &'static str,
        key: Option<&'static str>,
        asked: Vec<String>,
    }

    impl Person for Scripted {
        fn answer(&mut self, q: &str) -> String {
            self.asked.push(q.to_string());
            format!("{}\n", self.answer)
        }
        fn secret(&mut self, q: &str) -> Option<String> {
            self.asked.push(q.to_string());
            self.key.map(str::to_string)
        }
    }

    fn home() -> (tempfile::TempDir, Home) {
        let d = tempfile::tempdir().unwrap();
        let h = Home::new(d.path().join("c"), d.path().join("s"));
        (d, h)
    }

    // At the terminal: the disclosure, then the question; a yes records the consent and asks
    // for the key, typed unechoed; a no, or no answer, keeps capabilities off.
    #[test]
    fn consent_and_key_at_the_terminal() {
        let (_d, h) = home();
        let mut p = Scripted {
            answer: "y",
            key: Some(" sk-typed \n"),
            asked: Vec::new(),
        };
        let out = setup_engine(&h, false, None, Some(&mut p)).unwrap();
        assert!(out[0].starts_with("forge: capabilities: On each prompt"));
        assert_eq!(
            p.asked[0],
            "forge: send this to the capability engine on this machine? [y/N] "
        );
        assert!(p.asked[1].contains("not echoed"));
        assert_eq!(setup::key(&h).as_deref(), Some("sk-typed"));
        assert_eq!(setup::state(&h), setup::State::On);
        // asked once: nothing more on the next apply
        let mut again = Scripted {
            answer: "y",
            key: None,
            asked: Vec::new(),
        };
        assert!(
            setup_engine(&h, false, None, Some(&mut again))
                .unwrap()
                .is_empty()
        );
        assert!(again.asked.is_empty());

        for answer in ["n", "", "maybe"] {
            let (_d, h) = home();
            let mut p = Scripted {
                answer,
                key: Some("k"),
                asked: Vec::new(),
            };
            let out = setup_engine(&h, false, None, Some(&mut p)).unwrap();
            assert!(
                out.last().unwrap().contains("capabilities stay off"),
                "{answer:?}"
            );
            assert_eq!(p.asked.len(), 1, "no key is asked after a no");
            assert_eq!(setup::state(&h), setup::State::NoConsent);
        }
    }

    // Consent given, no key typed and none in the environment: off for want of a key. The
    // environment's key wins over typing it.
    #[test]
    fn the_key_from_the_environment_or_nowhere() {
        let (_d, h) = home();
        let mut p = Scripted {
            answer: "yes",
            key: None,
            asked: Vec::new(),
        };
        let out = setup_engine(&h, false, None, Some(&mut p)).unwrap();
        assert!(out.last().unwrap().contains("no key (set TYPESAFE_API_KEY"));
        assert_eq!(setup::state(&h), setup::State::NoKey);
        let mut p = Scripted {
            answer: "y",
            key: Some("typed"),
            asked: Vec::new(),
        };
        setup_engine(&h, false, Some("sk-env"), Some(&mut p)).unwrap();
        assert!(
            p.asked.is_empty(),
            "consented already, and the key came from the environment"
        );
        assert_eq!(setup::key(&h).as_deref(), Some("sk-env"));
    }
}
