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

/// Asks for consent when this machine has none, then for the key when there is none (or
/// `TYPESAFE_API_KEY` names another). Declined, or without a terminal and without
/// `--capability-engine`: capabilities stay off here, and apply goes on.
pub fn offer(flags: &Flags) -> Result<(), Error> {
    if flags.dry_run {
        return Ok(());
    }
    let home = Home::from_env().map_err(|e| Error::new(format!("forge: {e}")))?;
    let terminal = std::io::stdin().is_terminal();
    if !setup::has_consent(&home) {
        println!("forge: capabilities: {}", setup::DISCLOSURE);
        let yes = if flags.capability_engine {
            true
        } else if terminal {
            print!("forge: send this to the capability engine on this machine? [y/N] ");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
        } else {
            false
        };
        if !yes {
            println!(
                "forge: capabilities stay off on this machine (re-run forge apply --user, or add --capability-engine, to consent)"
            );
            return Ok(());
        }
        setup::record_consent(&home)
            .map_err(|e| Error::new(format!("forge: cannot record the consent: {e}")))?;
        println!("forge: consent to the capability engine recorded on this machine");
    }
    let from_env = std::env::var(KEY_VAR)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    let stored = setup::key(&home);
    let key = match (from_env, &stored) {
        (Some(k), Some(s)) if k == *s => return Ok(()),
        (Some(k), _) => Some(k),
        (None, Some(_)) => return Ok(()),
        (None, None) if terminal => {
            print!(
                "forge: the capability engine's API key (https://console.typesafe.ai/keys; not echoed): "
            );
            let _ = std::io::stdout().flush();
            read_hidden()
        }
        (None, None) => None,
    };
    let Some(key) = key else {
        println!(
            "forge: capabilities stay off on this machine: no key (set {KEY_VAR} and re-run forge apply --user)"
        );
        return Ok(());
    };
    setup::store_key(&home, &key)
        .map_err(|e| Error::new(format!("forge: cannot store the key: {e}")))?;
    println!("forge: the capability engine's key is stored for this machine");
    Ok(())
}
