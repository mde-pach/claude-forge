//! The current pack form: a POSIX ERE, matched per simple command.
//!
//! A pack pattern was written for the whole command text, JSON-escaped. It is now matched
//! against one simple command at a time, as [`crate::text::Rendered`] shows it, and where its
//! match may start is read from the pattern:
//!
//! - The pattern is split into its top-level alternatives; each is anchored on its own.
//! - An alternative that opens with a boundary group, `(^|…)` (`(^|[^A-Za-z0-9_-])`,
//!   `(^|[;&|(][[:space:]]*|\\n[[:space:]]*)`…), stood for "a command starts here". The group
//!   is dropped and the rest must match at a command position: the start of the command, of
//!   one of its assignments, or of a command name (after `sudo`, `env`, `nice -n 5`…, and
//!   after a runner: `npx`, `bunx`, `uv run`, `python`…), a directory before the name allowed
//!   (`./manage.py`, `/usr/bin/git`). So `which pnpm` does not match `(^|…)pnpm`, and
//!   `git commit -m "remove chmod 777"` does not match `(^|…)chmod…`.
//! - Any other alternative (a flag or a variable anywhere: `--env-mode[= ]loose`) may match
//!   anywhere except from inside a quoted word holding whitespace: `-m "use --env-mode=loose"`
//!   is prose.
//! - A pattern holding an escaped pipe (`\|`) is about a pipeline (`curl … | sh`): it is
//!   matched against the pipeline's stages joined by ` | `, positions of every stage counted.
//!
//! The ERE itself is read by the `regex` crate unchanged, with `.` matching newlines: the
//! constructs the packs use (`[[:space:]]`, bracket expressions, groups, `?*+{}`) mean the same.
//! A match is a yes/no question, so ERE's leftmost-longest rule makes no difference.

use std::sync::OnceLock;

use regex::Regex;

use crate::text::Rendered;

/// A pack pattern, ready to match.
#[derive(Debug)]
pub struct Pattern {
    pub source: String,
    branches: Vec<Branch>,
    /// Matched against a pipeline's text.
    pub pipeline: bool,
    /// Texts one of which every match contains: tested before the regex is compiled.
    literals: Vec<String>,
}

#[derive(Debug)]
struct Branch {
    anchored: bool,
    regex_source: String,
    regex: OnceLock<Option<Regex>>,
}

impl Branch {
    fn regex(&self) -> Option<&Regex> {
        self.regex
            .get_or_init(|| Regex::new(&self.regex_source).ok())
            .as_ref()
    }
}

impl Pattern {
    pub fn new(source: &str) -> Result<Self, String> {
        let branches: Vec<Branch> = top_level_branches(source)
            .into_iter()
            .map(|b| match strip_boundary(b) {
                Some(body) => Branch {
                    anchored: true,
                    regex_source: format!(r"(?s)^(?:[^[:space:]]*/)?(?:{body})"),
                    regex: OnceLock::new(),
                },
                None => Branch {
                    anchored: false,
                    regex_source: format!("(?s){b}"),
                    regex: OnceLock::new(),
                },
            })
            .collect();
        Ok(Pattern {
            source: source.to_owned(),
            branches,
            pipeline: has_escaped_pipe(source),
            literals: required_literals(source),
        })
    }

    /// Compiles every alternative now.
    pub fn check(&self) -> Result<(), String> {
        for b in &self.branches {
            Regex::new(&b.regex_source).map_err(|e| format!("pattern {:?}: {e}", self.source))?;
            b.regex();
        }
        Ok(())
    }

    /// Whether the pattern matches the command (or pipeline) shown as `r`.
    pub fn matches(&self, r: &Rendered) -> bool {
        if !self.literals.is_empty() && !self.literals.iter().any(|l| r.text.contains(l.as_str())) {
            return false;
        }
        self.branches.iter().any(|b| {
            let Some(re) = b.regex() else { return false };
            if b.anchored {
                r.positions
                    .iter()
                    .any(|&p| r.text.get(p..).is_some_and(|t| re.is_match(t)))
            } else {
                let mut from = 0;
                while let Some(m) = re.find_at(&r.text, from) {
                    if !r.in_spaced(m.start()) {
                        return true;
                    }
                    from = next_boundary(&r.text, m.start());
                    if from > r.text.len() {
                        break;
                    }
                }
                false
            }
        })
    }
}

fn next_boundary(s: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// The index of the `]` closing the bracket expression opening at `i`.
fn bracket_end(re: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    if re.get(j) == Some(&b'^') {
        j += 1;
    }
    if re.get(j) == Some(&b']') {
        j += 1;
    }
    while j < re.len() && re[j] != b']' {
        if re[j] == b'[' && matches!(re.get(j + 1), Some(b':' | b'.' | b'=')) {
            let close = [re[j + 1], b']'];
            match re[j + 2..].windows(2).position(|w| w == close) {
                Some(k) => j = j + 2 + k + 1,
                None => return re.len(),
            }
        }
        j += 1;
    }
    j
}

/// The top-level alternatives of an ERE.
fn top_level_branches(re: &str) -> Vec<&str> {
    let b = re.as_bytes();
    let mut out = Vec::new();
    let (mut depth, mut from, mut i) = (0i32, 0, 0);
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'[' => i = bracket_end(b, i),
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'|' if depth == 0 => {
                out.push(&re[from..i]);
                from = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&re[from..]);
    out
}

/// The alternative without its opening `(^|…)` boundary group, if it has one.
fn strip_boundary(branch: &str) -> Option<&str> {
    if !branch.starts_with("(^|") {
        return None;
    }
    let b = branch.as_bytes();
    let (mut depth, mut i) = (0i32, 0);
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'[' => i = bracket_end(b, i),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&branch[i + 1..]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Whether the ERE holds `\|`: a literal pipe outside a bracket expression.
fn has_escaped_pipe(re: &str) -> bool {
    let b = re.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                if b.get(i + 1) == Some(&b'|') {
                    return true;
                }
                i += 1;
            }
            b'[' => i = bracket_end(b, i),
            _ => {}
        }
        i += 1;
    }
    false
}

/// Literal texts one of which every match of the ERE contains (empty: none known), as
/// `requiredLiterals` in `src/render/guards.ts` reads them.
pub fn required_literals(re: &str) -> Vec<String> {
    let branches = top_level_branches(re);
    if branches.len() > 1 {
        let sets: Vec<Vec<String>> = branches.iter().map(|b| required_literals(b)).collect();
        if sets.iter().any(|s| s.is_empty()) {
            return vec![];
        }
        let mut out: Vec<String> = Vec::new();
        for s in sets.into_iter().flatten() {
            if !out.contains(&s) {
                out.push(s);
            }
        }
        return out;
    }
    let b = re.as_bytes();
    let n = b.len();
    let optional = |i: usize| {
        matches!(b.get(i), Some(b'?' | b'*'))
            || (b.get(i) == Some(&b'{')
                && b.get(i + 1) == Some(&b'0')
                && matches!(b.get(i + 2), Some(b',' | b'}')))
    };
    let skip_quantifier = |i: usize| -> usize {
        match b.get(i) {
            Some(b'{') => re[i..].find('}').map(|k| i + k).unwrap_or(n),
            Some(b'?' | b'*' | b'+') => i,
            _ => i.wrapping_sub(1),
        }
    };
    let mut candidates: Vec<Vec<String>> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, c: &mut Vec<Vec<String>>| {
        if !cur.is_empty() {
            c.push(vec![std::mem::take(cur)]);
        }
    };
    let mut i = 0usize;
    while i < n {
        let c = b[i];
        if c == b'[' {
            flush(&mut cur, &mut candidates);
            i = skip_quantifier(bracket_end(b, i) + 1).wrapping_add(1);
            continue;
        }
        if c == b'(' {
            flush(&mut cur, &mut candidates);
            let mut d = 1;
            let mut j = i + 1;
            while j < n && d > 0 {
                match b[j] {
                    b'\\' => j += 1,
                    b'[' => j = bracket_end(b, j),
                    b'(' => d += 1,
                    b')' => d -= 1,
                    _ => {}
                }
                j += 1;
            }
            if !optional(j) && j >= 1 && j - 1 > i {
                let inner = required_literals(&re[i + 1..j - 1]);
                if !inner.is_empty() {
                    candidates.push(inner);
                }
            }
            i = skip_quantifier(j).wrapping_add(1);
            continue;
        }
        let lit = if c == b'\\' {
            i += 1;
            b.get(i).copied()
        } else if !b".^$*+?{}()|".contains(&c) {
            Some(c)
        } else {
            None
        };
        let Some(lit) = lit else {
            flush(&mut cur, &mut candidates);
            if c == b'{' {
                i = re[i..].find('}').map(|k| i + k).unwrap_or(n);
            }
            i += 1;
            continue;
        };
        if optional(i + 1) {
            flush(&mut cur, &mut candidates);
            i = skip_quantifier(i + 1).wrapping_add(1);
        } else if matches!(b.get(i + 1), Some(b'+' | b'{')) {
            cur.push(lit as char);
            flush(&mut cur, &mut candidates);
            i = skip_quantifier(i + 1).wrapping_add(1);
        } else {
            cur.push(lit as char);
            i += 1;
        }
    }
    flush(&mut cur, &mut candidates);
    // fewest alternatives, then the longest shortest text
    candidates.sort_by_key(|x| {
        (
            x.len(),
            std::cmp::Reverse(x.iter().map(|t| t.len()).min().unwrap_or(0)),
        )
    });
    match candidates.into_iter().next() {
        Some(best) if best.iter().map(|t| t.len()).min().unwrap_or(0) >= 2 => best,
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_as_the_typescript_reads_them() {
        assert_eq!(
            required_literals("(^|[^[:alnum:]_])(terraform|tofu)[[:space:]]+destroy"),
            ["destroy"]
        );
        assert_eq!(required_literals("abcd|(x|yz)qq"), ["abcd", "qq"]);
        assert!(required_literals("ab?c*d").is_empty());
    }

    #[test]
    fn branches_and_boundaries() {
        assert_eq!(top_level_branches("a|(b|c)|[|]d"), ["a", "(b|c)", "[|]d"]);
        assert_eq!(strip_boundary("(^|[^A-Za-z0-9_-])git x"), Some("git x"));
        assert_eq!(
            strip_boundary(r"(^|[;&|(][[:space:]]*|\\n[[:space:]]*)sudo"),
            Some("sudo")
        );
        assert!(has_escaped_pipe(r"curl[^|]*\|sh"));
        assert!(!has_escaped_pipe(r"[;&|]x\\|y"));
    }
}
