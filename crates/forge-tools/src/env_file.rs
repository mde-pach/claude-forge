//! Putting Forge's uv on the agent's PATH through `CLAUDE_ENV_FILE`.
//!
//! Claude Code gives SessionStart hooks a file whose `export` lines run before each Bash
//! command of the session. The SessionStart hook appends one line that prepends
//! `<cache>/path` (which holds only `uv` and `uvx`), so the agent's `uv` is Forge's
//! pinned one even when the machine has none.

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use forge_core::{Note, NoteKind};
use forge_home::Home;

/// The environment variable Claude Code sets for SessionStart hooks.
pub const CLAUDE_ENV_FILE: &str = "CLAUDE_ENV_FILE";

/// Quotes text for a POSIX shell: `'…'`, each `'` written as `'\''`.
pub fn sh_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// `export PATH='<dir>'"${PATH:+:$PATH}"`: `dir` first, and no empty entry (which would
/// mean the current directory) when PATH was empty. `None` when the path is not UTF-8 or
/// holds a NUL byte.
pub fn export_line(dir: &Path) -> Option<String> {
    let s = dir.to_str().filter(|s| !s.contains('\0'))?;
    Some(format!("export PATH={}\"${{PATH:+:$PATH}}\"", sh_quote(s)))
}

/// Appends [`export_line`] for `dir` to `env_file` unless the file already has that exact
/// line. Returns whether it appended. The file is created when missing.
pub fn append_path_export(env_file: &Path, dir: &Path) -> io::Result<bool> {
    let line = export_line(dir)
        .ok_or_else(|| io::Error::other("the path is not UTF-8 text without NUL bytes"))?;
    let mut f = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(env_file)?;
    let mut text = Vec::new();
    f.read_to_end(&mut text)?;
    if String::from_utf8_lossy(&text).lines().any(|l| l == line) {
        return Ok(false);
    }
    let mut add = String::new();
    if !text.is_empty() && !text.ends_with(b"\n") {
        add.push('\n');
    }
    add.push_str(&line);
    add.push('\n');
    f.seek(SeekFrom::End(0))?;
    f.write_all(add.as_bytes())?;
    Ok(true)
}

/// What the SessionStart hook calls: puts `<cache>/path` on the agent's PATH when Claude
/// Code gave an env file. Nothing to do without one (an older Claude Code, or `forge` run
/// by hand). A failure is a note for the person; the session goes on without Forge's uv.
pub fn add_forge_path_to_session(home: &Home) -> Option<Note> {
    let file = std::env::var_os(CLAUDE_ENV_FILE).filter(|v| !v.is_empty())?;
    match append_path_export(Path::new(&file), &home.path_dir()) {
        Ok(_) => None,
        Err(e) => Some(Note::new(
            NoteKind::Setup,
            format!(
                "Could not put Forge's uv on the session's PATH ({}: {e}); the agent uses the machine's uv, if any.",
                Path::new(&file).display()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/a b"), "'/a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(
            export_line(Path::new("/c/path")).unwrap(),
            "export PATH='/c/path'\"${PATH:+:$PATH}\""
        );
    }
}
