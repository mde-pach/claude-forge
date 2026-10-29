//! Positions and URIs: LSP positions are 0-based lines and UTF-16 columns; the engine works on byte
//! offsets of UTF-8 text.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Pos {
    pub line: u32,
    pub character: u32,
}

impl Pos {
    pub fn json(self) -> Value {
        json!({"line": self.line, "character": self.character})
    }

    pub fn from_json(v: &Value) -> Option<Pos> {
        Some(Pos {
            line: u32::try_from(v.get("line")?.as_u64()?).ok()?,
            character: u32::try_from(v.get("character")?.as_u64()?).ok()?,
        })
    }
}

/// A declaration's location, as a server answers it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Loc {
    pub uri: String,
    pub pos: Pos,
}

/// The LSP position of byte offset `index` of `text`.
pub fn position(text: &str, index: usize) -> Pos {
    let index = floor_char(text, index.min(text.len()));
    let before = &text[..index];
    let line = before.bytes().filter(|&b| b == b'\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let character: usize = text[start..index].chars().map(char::len_utf16).sum();
    Pos {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(character).unwrap_or(u32::MAX),
    }
}

/// The byte offset of an LSP position in `text` (clamped to its line).
pub fn index_of(text: &str, pos: Pos) -> usize {
    let mut i = 0;
    for _ in 0..pos.line {
        match text[i..].find('\n') {
            Some(n) => i += n + 1,
            None => return text.len(),
        }
    }
    let mut col = 0;
    for (j, c) in text[i..].char_indices() {
        if col >= pos.character as usize || c == '\n' {
            return i + j;
        }
        col += c.len_utf16();
    }
    text.len()
}

/// The UTF-16 offset (a JavaScript string index) of byte offset `index`.
pub fn utf16_index(text: &str, index: usize) -> usize {
    text[..floor_char(text, index.min(text.len()))]
        .chars()
        .map(char::len_utf16)
        .sum()
}

pub fn floor_char(text: &str, mut i: usize) -> usize {
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The `file://` URI of a path (its real path when it exists).
pub fn uri(path: &Path) -> String {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = String::from("file://");
    for b in real.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The path of a `file://` URI.
pub fn path_of(uri: &str) -> PathBuf {
    let Some(rest) = uri.strip_prefix("file://") else {
        return PathBuf::from(uri);
    };
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}

/// Whether `a` and `b` are the same file.
pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
}

pub fn read_text(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub fn is_word_start(b: u8, dollar: bool) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || (dollar && b == b'$')
}

pub fn is_word(b: u8, dollar: bool) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || (dollar && b == b'$')
}

/// The identifier starting at `i`, if one does.
pub fn word_at_index(text: &str, i: usize, dollar: bool) -> Option<&str> {
    let b = text.as_bytes();
    if i >= b.len() || !is_word_start(b[i], dollar) {
        return None;
    }
    let mut j = i + 1;
    while j < b.len() && is_word(b[j], dollar) {
        j += 1;
    }
    Some(&text[i..j])
}

/// The first identifier at or after `i` (start, word).
pub fn next_word(text: &str, mut i: usize, dollar: bool) -> Option<(usize, &str)> {
    let b = text.as_bytes();
    while i < b.len() {
        if is_word_start(b[i], dollar) && (i == 0 || !is_word(b[i - 1], dollar)) {
            return word_at_index(text, i, dollar).map(|w| (i, w));
        }
        i += 1;
    }
    None
}

/// The trailing identifier of `expr` (start, word).
pub fn last_word(expr: &str, dollar: bool) -> Option<(usize, &str)> {
    let b = expr.as_bytes();
    let mut s = b.len();
    while s > 0 && is_word(b[s - 1], dollar) {
        s -= 1;
    }
    while s < b.len() && !is_word_start(b[s], dollar) {
        s += 1;
    }
    (s < b.len()).then(|| (s, &expr[s..]))
}

pub fn as_array(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        Value::Null => Vec::new(),
        other => vec![other.clone()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip_utf16() {
        let t = "a = 1\nb = \"é𝄞\" + x\n";
        let i = t.find('x').unwrap();
        let p = position(t, i);
        assert_eq!(
            p,
            Pos {
                line: 1,
                character: 12
            }
        );
        assert_eq!(index_of(t, p), i);
    }

    #[test]
    fn uris_round_trip() {
        let p = Path::new("/tmp/a b/é.py");
        assert_eq!(path_of(&uri(p)), p);
    }

    #[test]
    fn last_words() {
        assert_eq!(last_word("np.load", false), Some((3, "load")));
        assert_eq!(last_word("pick()", false), None);
    }
}
