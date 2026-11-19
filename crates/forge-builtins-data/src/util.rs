//! Small readers the shell modules shared: cached regexes, `forge_toml_get`, durations,
//! versions, word matches.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use regex::Regex;

static CACHE: LazyLock<Mutex<HashMap<String, &'static Regex>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A regex compiled once (patterns are the crate's own literals or built from them).
pub fn re(pattern: &str) -> &'static Regex {
    let mut m = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = m.get(pattern) {
        return r;
    }
    let r: &'static Regex = Box::leak(Box::new(
        Regex::new(pattern).unwrap_or_else(|_| Regex::new("[^\\s\\S]").expect("regex")),
    ));
    m.insert(pattern.to_owned(), r);
    r
}

/// `forge_toml_get <text> <section> <key>`: the value of `key` in `[section]` ("" for the top
/// level), quotes and comments removed; None when absent. One-line values only.
pub fn toml_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut s = String::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            let inner = t.trim_start_matches('[').trim_start();
            s = inner.split(']').next().unwrap_or("").trim_end().to_owned();
            continue;
        }
        if s != section {
            continue;
        }
        let Some(rest) = t.strip_prefix(key) else {
            continue;
        };
        let r = rest.trim_start();
        let Some(r) = r.strip_prefix('=') else {
            continue;
        };
        let r = r.trim_start();
        let v = if let Some(q) = r.strip_prefix('"') {
            q.split('"').next().unwrap_or("").to_owned()
        } else if let Some(q) = r.strip_prefix('\'') {
            q.split('\'').next().unwrap_or("").to_owned()
        } else {
            re(r"[ \t]*(#.*)?$").replace(r, "").into_owned()
        };
        return Some(v);
    }
    None
}

/// `forge_duration_seconds`: a relative duration ("7 days", "1 week", "72 hours", P7D, P1W,
/// PT72H) in seconds.
pub fn duration_seconds(text: &str) -> Option<u64> {
    let v = text.to_lowercase();
    let num = |s: &str| s.parse::<u64>().ok();
    if let Some(c) = re(r"^p([0-9]+)w$").captures(&v) {
        return num(&c[1]).map(|n| n * 604_800);
    }
    if let Some(c) = re(r"^p([0-9]+)d$").captures(&v) {
        return num(&c[1]).map(|n| n * 86_400);
    }
    if let Some(c) = re(r"^pt([0-9]+)h$").captures(&v) {
        return num(&c[1]).map(|n| n * 3_600);
    }
    if let Some(c) = re(r"^([0-9]+)[ \t]*(weeks?|days?|hours?)$").captures(&v) {
        let unit = match &c[2][..1] {
            "w" => 604_800,
            "d" => 86_400,
            _ => 3_600,
        };
        return num(&c[1]).map(|n| n * unit);
    }
    None
}

/// A duration in seconds as people write it ("3 days", "36 hours").
pub fn duration_text(secs: u64) -> String {
    if secs % 86_400 == 0 {
        let d = secs / 86_400;
        format!("{d} day{}", if d == 1 { "" } else { "s" })
    } else {
        let h = secs.div_ceil(3_600);
        format!("{h} hour{}", if h == 1 { "" } else { "s" })
    }
}

/// `forge_version_ge`: a >= b, on numeric dot-separated parts.
pub fn version_ge(a: &str, b: &str) -> bool {
    let parts = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| {
                let d: String = p.chars().take_while(char::is_ascii_digit).collect();
                d.parse().unwrap_or(0)
            })
            .collect()
    };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return p > q;
        }
    }
    true
}

/// Whether `id` occurs in `s` as a whole word, `word` telling which characters make words.
pub fn has_word(s: &str, id: &str, word: fn(char) -> bool) -> bool {
    occurrences(s, id, word).next().is_some()
}

/// The byte offsets of `id` as a whole word in `s`.
pub fn occurrences<'a>(
    s: &'a str,
    id: &'a str,
    word: fn(char) -> bool,
) -> impl Iterator<Item = usize> + 'a {
    s.match_indices(id).map(|(i, _)| i).filter(move |&i| {
        let before = s[..i].chars().next_back();
        let after = s[i + id.len()..].chars().next();
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

pub fn py_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn js_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// The last non-blank line of a tool's output (why it could not run).
pub fn last_line(text: &str) -> String {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// `dirname`: "." for a top-level path.
pub fn dirname(p: &str) -> String {
    match p.rsplit_once('/') {
        Some((d, _)) if !d.is_empty() => d.to_owned(),
        _ => ".".to_owned(),
    }
}

pub fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// `dir/name`, or `name` when `dir` is ".".
pub fn join(dir: &str, name: &str) -> String {
    if dir == "." || dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers() {
        let t = "[project]\nversion = \"1.2\"\n[tool.uv]\nexclude-newer = \"7 days\" # x\nrequired-version = >=0.9 # c\n";
        assert_eq!(toml_get(t, "project", "version").as_deref(), Some("1.2"));
        assert_eq!(
            toml_get(t, "tool.uv", "exclude-newer").as_deref(),
            Some("7 days")
        );
        assert_eq!(
            toml_get(t, "tool.uv", "required-version").as_deref(),
            Some(">=0.9")
        );
        assert_eq!(duration_seconds("7 days"), Some(604_800));
        assert_eq!(duration_seconds("P1W"), Some(604_800));
        assert_eq!(duration_seconds("2026-01-01"), None);
        assert!(version_ge("0.10", "0.9.17"));
        assert!(!version_ge("0.9.2", "0.9.17"));
        assert!(has_word("a foo_b foo", "foo", py_word));
        assert!(!has_word("foo_b", "foo", py_word));
        assert_eq!(duration_text(259_200), "3 days");
    }
}
