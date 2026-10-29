//! The part of `forge.lock` the launcher needs: the pinned Forge version.
//!
//! `forge.lock` is TOML. The launcher runs before every hook, so instead of a
//! full TOML parser it scans only the top-level table (everything before the
//! first `[header]`) and reads one key:
//!
//! ```toml
//! forge = "0.7.0"
//! ```
//!
//! Other keys are skipped (their values are lexed just enough to step over
//! strings, arrays and inline tables correctly). The scan stops at the first
//! table header, so the rest of the file — packs, tools, hashes — costs
//! nothing however large it grows. The full parser in the CLI validates the
//! whole file; this one only has to agree with it on `forge`, which the
//! differential tests check against the `toml` crate.

use std::fmt;
use std::io;
use std::path::Path;

/// What the launcher reads from `forge.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lock {
    /// Exact Forge version (`MAJOR.MINOR.PATCH[-pre][+build]`), if pinned.
    pub forge: Option<String>,
}

#[derive(Debug)]
pub enum LockError {
    Io(io::Error),
    /// Not valid TOML where we looked (1-based line).
    Syntax {
        line: usize,
        message: &'static str,
    },
    /// `forge` is present but not an exact semver version.
    InvalidVersion(String),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Io(e) => write!(f, "cannot read forge.lock: {e}"),
            LockError::Syntax { line, message } => {
                write!(f, "forge.lock line {line}: {message}")
            }
            LockError::InvalidVersion(v) => {
                write!(
                    f,
                    "forge.lock: forge = {v:?} is not an exact version like \"1.2.3\""
                )
            }
        }
    }
}

impl std::error::Error for LockError {}

/// Reads `path`. A missing file is `Ok(None)`: the repo has no pin.
pub fn read_lock(path: &Path) -> Result<Option<Lock>, LockError> {
    match std::fs::read(path) {
        Ok(bytes) => parse_lock_bytes(&bytes).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(LockError::Io(e)),
    }
}

/// Parses the top-level `forge` key from lock text.
pub fn parse_lock(text: &str) -> Result<Lock, LockError> {
    parse_lock_bytes(text.as_bytes())
}

fn parse_lock_bytes(b: &[u8]) -> Result<Lock, LockError> {
    let mut s = Scanner { b, i: 0, line: 1 };
    // A UTF-8 BOM is allowed before TOML content.
    if b.starts_with(b"\xEF\xBB\xBF") {
        s.i = 3;
    }
    let mut lock = Lock::default();
    loop {
        s.skip_ws();
        match s.peek() {
            None | Some(b'[') => break,
            Some(b'#') => s.skip_comment(),
            Some(b'\n') => s.newline(),
            Some(b'\r') if s.peek_at(1) == Some(b'\n') => {
                s.i += 1;
                s.newline();
            }
            Some(_) => {
                let is_forge = s.key()?;
                s.skip_ws();
                if s.peek() != Some(b'=') {
                    return Err(s.err("expected '=' after key"));
                }
                s.i += 1;
                s.skip_ws();
                if is_forge {
                    if lock.forge.is_some() {
                        return Err(s.err("duplicate key 'forge'"));
                    }
                    let v = s.simple_string()?;
                    if !is_exact_semver(&v) {
                        return Err(LockError::InvalidVersion(v));
                    }
                    lock.forge = Some(v);
                } else {
                    s.skip_value()?;
                }
                s.end_of_line()?;
            }
        }
    }
    Ok(lock)
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    line: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn peek_at(&self, n: usize) -> Option<u8> {
        self.b.get(self.i + n).copied()
    }

    fn err(&self, message: &'static str) -> LockError {
        LockError::Syntax {
            line: self.line,
            message,
        }
    }

    fn newline(&mut self) {
        self.i += 1;
        self.line += 1;
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }

    fn skip_comment(&mut self) {
        while !matches!(self.peek(), None | Some(b'\n')) {
            self.i += 1;
        }
    }

    /// After a value: optional spaces and comment, then newline or EOF.
    fn end_of_line(&mut self) -> Result<(), LockError> {
        self.skip_ws();
        match self.peek() {
            Some(b'#') => self.skip_comment(),
            Some(b'\r') if self.peek_at(1) == Some(b'\n') => self.i += 1,
            None | Some(b'\n') => {}
            _ => return Err(self.err("unexpected text after value")),
        }
        Ok(())
    }

    /// Reads a (possibly dotted, possibly quoted) key. Returns whether it is
    /// exactly the single bare or quoted key `forge`.
    fn key(&mut self) -> Result<bool, LockError> {
        let mut parts = 0;
        let mut first_is_forge = false;
        loop {
            self.skip_ws();
            let part: Vec<u8> = match self.peek() {
                Some(b'"' | b'\'') if !self.at_triple() => self.simple_string()?.into_bytes(),
                Some(c) if is_bare(c) => {
                    let start = self.i;
                    while self.peek().is_some_and(is_bare) {
                        self.i += 1;
                    }
                    self.b[start..self.i].to_vec()
                }
                _ => return Err(self.err("expected a key")),
            };
            parts += 1;
            if parts == 1 {
                first_is_forge = part == b"forge";
            }
            self.skip_ws();
            if self.peek() == Some(b'.') {
                self.i += 1;
                continue;
            }
            return Ok(first_is_forge && parts == 1);
        }
    }

    fn at_triple(&self) -> bool {
        let q = self.peek();
        q.is_some() && self.peek_at(1) == q && self.peek_at(2) == q
    }

    /// A single-line string. Basic strings may hold escapes other than
    /// `\u`/`\U` (never needed for versions); we decode the simple ones.
    fn simple_string(&mut self) -> Result<String, LockError> {
        let q = match self.peek() {
            Some(q @ (b'"' | b'\'')) if !self.at_triple() => q,
            _ => return Err(self.err("expected a one-line string")),
        };
        self.i += 1;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => return Err(self.err("unterminated string")),
                Some(c) if c == q => {
                    self.i += 1;
                    break;
                }
                Some(b'\\') if q == b'"' => {
                    let c = match self.peek_at(1) {
                        Some(b'"') => b'"',
                        Some(b'\\') => b'\\',
                        Some(b'n') => b'\n',
                        Some(b't') => b'\t',
                        Some(b'r') => b'\r',
                        Some(b'b') => 8,
                        Some(b'f') => 12,
                        _ => return Err(self.err("unsupported escape in string")),
                    };
                    out.push(c);
                    self.i += 2;
                }
                Some(c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
        String::from_utf8(out).map_err(|_| self.err("string is not UTF-8"))
    }

    /// Steps over any value: strings of all four kinds, arrays and inline
    /// tables (which may span lines and hold comments), and scalars.
    fn skip_value(&mut self) -> Result<(), LockError> {
        let mut depth = 0usize;
        loop {
            match self.peek() {
                None => {
                    return if depth == 0 {
                        Ok(())
                    } else {
                        Err(self.err("unterminated array or inline table"))
                    };
                }
                Some(b'"' | b'\'') => {
                    if self.at_triple() {
                        self.skip_multiline_string()?;
                    } else {
                        self.skip_line_string()?;
                    }
                }
                Some(b'[' | b'{') => {
                    depth += 1;
                    self.i += 1;
                }
                Some(b']' | b'}') => {
                    if depth == 0 {
                        return Err(self.err("unbalanced bracket"));
                    }
                    depth -= 1;
                    self.i += 1;
                }
                Some(b'#') if depth > 0 => self.skip_comment(),
                Some(b'\n') if depth > 0 => self.newline(),
                Some(b'#' | b'\n') => return Ok(()),
                Some(b'\r') if depth == 0 && self.peek_at(1) == Some(b'\n') => return Ok(()),
                Some(_) => self.i += 1,
            }
            if depth == 0 && self.value_done() {
                return Ok(());
            }
        }
    }

    /// At depth 0, a value ends at whitespace/comment/newline — except the
    /// single space TOML allows between a date and a time (`1979-05-27 07:32:00`).
    fn value_done(&self) -> bool {
        match self.peek() {
            Some(b' ') => {
                let prev_digit = self.i > 0 && self.b[self.i - 1].is_ascii_digit();
                !(prev_digit && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()))
            }
            c => matches!(c, None | Some(b'\t' | b'#' | b'\n' | b'\r')),
        }
    }

    fn skip_line_string(&mut self) -> Result<(), LockError> {
        let q = self.peek().unwrap_or(b'"');
        self.i += 1;
        loop {
            match self.peek() {
                None | Some(b'\n') => return Err(self.err("unterminated string")),
                Some(b'\\') if q == b'"' => self.i += 2,
                Some(c) if c == q => {
                    self.i += 1;
                    return Ok(());
                }
                Some(_) => self.i += 1,
            }
        }
    }

    fn skip_multiline_string(&mut self) -> Result<(), LockError> {
        let q = self.peek().unwrap_or(b'"');
        self.i += 3;
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated multi-line string")),
                Some(b'\n') => self.newline(),
                Some(b'\\') if q == b'"' => {
                    if self.peek_at(1) == Some(b'\n') {
                        self.line += 1;
                    }
                    self.i += 2;
                }
                Some(c) if c == q && self.at_triple() => {
                    self.i += 3;
                    // Up to two more quotes may close the string ("""a""""").
                    for _ in 0..2 {
                        if self.peek() == Some(q) {
                            self.i += 1;
                        }
                    }
                    return Ok(());
                }
                Some(_) => self.i += 1,
            }
        }
    }
}

fn is_bare(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// `MAJOR.MINOR.PATCH`, optional `-pre` and `+build` (semver 2.0 grammar,
/// no leading zeros in numeric parts).
pub fn is_exact_semver(v: &str) -> bool {
    let (rest, build) = match v.split_once('+') {
        Some((r, b)) => (r, Some(b)),
        None => (v, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (rest, None),
    };
    let num = |s: &str| {
        !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
    };
    let ident =
        |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-');
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| num(p))
        && pre.is_none_or(|p| {
            p.split('.')
                .all(|id| ident(id) && (!id.bytes().all(|c| c.is_ascii_digit()) || num(id)))
        })
        && build.is_none_or(|b| b.split('.').all(ident))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forge(t: &str) -> Option<String> {
        parse_lock(t).unwrap().forge
    }

    #[test]
    fn reads_forge() {
        assert_eq!(forge("forge = \"0.7.0\"\n").as_deref(), Some("0.7.0"));
        assert_eq!(
            forge("forge='1.2.3-rc.1+b5'").as_deref(),
            Some("1.2.3-rc.1+b5")
        );
        assert_eq!(
            forge("\u{feff}\"forge\" = \"1.0.0\" # pinned\r\n").as_deref(),
            Some("1.0.0")
        );
        assert_eq!(forge("").as_deref(), None);
    }

    #[test]
    fn only_top_level() {
        assert_eq!(forge("[tool]\nforge = \"1.0.0\"\n"), None);
        assert_eq!(forge("x.forge = \"1.0.0\"\n"), None);
        assert_eq!(forge("forge.x = \"1.0.0\"\n"), None);
        assert_eq!(forge("a = 1\n[[packs]]\nforge = \"1.0.0\"\n"), None);
    }

    #[test]
    fn skips_tricky_values() {
        let t = r#"
schema = 1
note = """
[not-a-table]
forge = "9.9.9"
"""
lit = '''x'''
arr = [
  "[x]", # comment ]
  [1, 2],
  { a = "}" },
]
inline = { forge = "8.8.8" }
date = 2026-10-29T10:00:00Z
forge = "0.7.0"
[packs]
"#;
        assert_eq!(forge(t).as_deref(), Some("0.7.0"));
    }

    #[test]
    fn rejects_bad() {
        assert!(matches!(
            parse_lock("forge = 7"),
            Err(LockError::Syntax { .. })
        ));
        assert!(matches!(
            parse_lock("forge = \"1.2\""),
            Err(LockError::InvalidVersion(_))
        ));
        assert!(matches!(
            parse_lock("forge = \"01.2.3\""),
            Err(LockError::InvalidVersion(_))
        ));
        assert!(matches!(
            parse_lock("forge = \"1.2.3\"\nforge = \"1.2.3\""),
            Err(LockError::Syntax { line: 2, .. })
        ));
        assert!(parse_lock("forge = \"1.2.3\" x").is_err());
        assert!(parse_lock("{\"version\": 1}").is_err());
        assert!(parse_lock("a = [1,\n").is_err());
    }

    #[test]
    fn semver() {
        for ok in [
            "0.0.0",
            "1.2.3",
            "10.20.30",
            "1.0.0-alpha.1",
            "1.0.0-0.3.7",
            "1.0.0+20130313",
            "0.6.0-dev",
        ] {
            assert!(is_exact_semver(ok), "{ok}");
        }
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.02.3",
            "1.2.3-",
            "1.2.3-01",
            "1.2.3+",
            "1.2.3-a..b",
            "1.2.x",
        ] {
            assert!(!is_exact_semver(bad), "{bad}");
        }
    }
}
