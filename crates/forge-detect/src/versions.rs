//! Versions as detection compares them (`src/core/versions.ts`): every declared or locked
//! version becomes semver, and a pack's `version` is an npm range checked against it.

use std::sync::LazyLock;

use nodejs_semver::{Range, Version};
use regex::Regex;

static COERCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{1,16})(?:\.(\d{1,16}))?(?:\.(\d{1,16}))?").expect("regex"));
static PEP440: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:===|==|>=|~=)\s*([0-9][0-9A-Za-z.]*)").expect("regex"));

/// A version string as semver (`v1.2` → `1.2.0`); `""` when it has no number (node-semver's `coerce`).
pub fn coerce(v: &str) -> String {
    let Some(c) = COERCE.captures(v) else {
        return String::new();
    };
    let n = |i: usize| {
        c.get(i)
            .map(|m| m.as_str().trim_start_matches('0'))
            .map(|s| if s.is_empty() { "0" } else { s })
            .unwrap_or("0")
            .to_string()
    };
    format!("{}.{}.{}", n(1), n(2), n(3))
}

/// The lowest version an npm range admits (`""` when it is not a semver range: `workspace:*`, a URL).
pub fn from_range(range: &str) -> String {
    Range::parse(range)
        .ok()
        .and_then(|r| r.min_version())
        .map(|v| v.to_string())
        .unwrap_or_default()
}

/// The version a PEP 440 specifier pins or starts from (`==`, `>=`, `~=`, `===`), as semver; `""` without one.
pub fn from_pep440(spec: &str) -> String {
    PEP440
        .captures(spec)
        .map(|c| coerce(&c[1]))
        .unwrap_or_default()
}

/// A Poetry or Pipenv constraint (`^2.0`, `~1.4`, `>=1,<2`, `1.2.3`, `*`): its lowest version.
pub fn from_poetry(spec: &str) -> String {
    let s = spec.trim();
    if s.is_empty() || s == "*" {
        return String::new();
    }
    if (s.starts_with('^') || s.starts_with('~')) && !s.starts_with("~=") {
        return from_range(s);
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        return coerce(s);
    }
    from_pep440(s)
}

/// Whether a declared version is in `range`; an unknown version (`""`, `workspace:*`, a git URL) is accepted.
pub fn satisfies(version: &str, range: Option<&str>) -> bool {
    let Some(range) = range.filter(|r| !r.is_empty()) else {
        return true;
    };
    let v = coerce(version);
    if v.is_empty() {
        return true;
    }
    match (Version::parse(&v), Range::parse(range)) {
        (Ok(v), Ok(r)) => r.satisfies_with_prerelease(&v, true),
        _ => false,
    }
}

/// Compares dotted versions on their first three numbers (1.10.0 > 1.9.3).
pub fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| -> [u64; 3] {
        let mut out = [0; 3];
        for (i, p) in s.split('.').take(3).enumerate() {
            let digits: String = p.chars().take_while(char::is_ascii_digit).collect();
            out[i] = digits.parse().unwrap_or(0);
        }
        out
    };
    parts(a).cmp(&parts(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(coerce("v1.2"), "1.2.0");
        assert_eq!(coerce("x"), "");
        assert_eq!(from_range("^14.2.1"), "14.2.1");
        assert_eq!(from_range(">=5 <7"), "5.0.0");
        assert_eq!(from_range("*"), "0.0.0");
        assert_eq!(from_range("workspace:*"), "");
        assert_eq!(from_pep440(">=0.110,<1"), "0.110.0");
        assert_eq!(from_pep440(""), "");
        assert_eq!(from_poetry("^2.0"), "2.0.0");
        assert_eq!(from_poetry("1.4.2"), "1.4.2");
        assert_eq!(from_poetry(">=3.1,<4"), "3.1.0");
        assert_eq!(from_poetry("*"), "");
    }

    #[test]
    fn ranges() {
        assert!(satisfies("15.1.0", Some(">=15")));
        assert!(!satisfies("14.2.0", Some(">=15")));
        assert!(satisfies("", Some(">=15")));
        assert!(satisfies("4.0.0-beta.1", Some(">=4.0.0-0")));
        assert!(satisfies("1.0.0", None));
        assert_eq!(compare("1.10.0", "1.9.3"), std::cmp::Ordering::Greater);
        assert_eq!(compare("2.1.295", "2.1.295"), std::cmp::Ordering::Equal);
    }
}
