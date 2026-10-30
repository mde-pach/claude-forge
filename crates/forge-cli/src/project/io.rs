//! Output, prompts and file writes the setup commands share.

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use forge_render::changes::line_diff;

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub code: u8,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Error {
        Error {
            message: message.into(),
            code: 1,
        }
    }

    pub fn usage(message: impl Into<String>) -> Error {
        Error {
            message: message.into(),
            code: 2,
        }
    }
}

pub fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::new(format!("forge: {}: {e}", path.display()))
}

/// Writes a file, creating its directories.
pub fn write_text(path: &Path, text: &str, executable: bool) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    }
    std::fs::write(path, text).map_err(|e| io_err(path, e))?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| io_err(path, e))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

/// Removes the empty directories from `dir` up to (not including) `stop`.
pub fn prune_dirs(mut dir: &Path, stop: &Path) {
    while dir.starts_with(stop) && dir != stop {
        if std::fs::remove_dir(dir).is_err() {
            return;
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => return,
        }
    }
}

/// Deletes `base/rel` (a file or a directory) and the directories it leaves empty.
pub fn delete_path(base: &Path, rel: &str) -> Result<(), Error> {
    let p = base.join(rel);
    let r = if p.is_dir() && !p.is_symlink() {
        std::fs::remove_dir_all(&p)
    } else {
        std::fs::remove_file(&p)
    };
    match r {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err(&p, e)),
    }
    if let Some(parent) = p.parent() {
        prune_dirs(parent, base);
    }
    Ok(())
}

/// Writes a settings file, or deletes it when nothing is left in it.
pub fn write_settings(base: &Path, file: &str, after: Option<&str>) -> Result<(), Error> {
    match after {
        Some(t) => write_text(&base.join(file), t, false),
        None => delete_path(base, file),
    }
}

/// Prints a file's diff: "(new)" when there was none, "(deleted)" when none is left.
pub fn show_diff(before: Option<&str>, after: Option<&str>, path: &str) {
    let label = match (before, after) {
        (None, _) => format!("{path} (new)"),
        (_, None) => format!("{path} (deleted)"),
        _ => path.to_string(),
    };
    print!(
        "{}",
        line_diff(before.unwrap_or(""), after.unwrap_or(""), &label)
    );
}

/// Asks a yes/no question on the terminal (`yes`: the answer is yes); without a terminal,
/// fails with how to confirm.
/// Asks the person `question` (without the `forge: ` prefix, which this adds).
pub fn confirm(question: &str, yes: bool) -> Result<bool, Error> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err(Error::new(format!(
            "forge: {question} Re-run with --yes to confirm non-interactively."
        )));
    }
    print!("forge: {question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    let a = line.trim().to_lowercase();
    Ok(a == "y" || a == "yes")
}

pub fn stdin_is_terminal() -> bool {
    std::io::stdin().is_terminal()
}
