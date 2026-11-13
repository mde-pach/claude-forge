//! grep targets: a POSIX extended regular expression matched line by line (`grep -E`, `-iE`
//! with `ignore_case`), for text ast-grep does not parse (config files, comments).
//!
//! Patterns are POSIX EREs; they are translated to the `regex` crate's syntax, which differs
//! inside bracket expressions (`[`, `\`, `&&`, `--`, `~~` are literal in POSIX).

use forge_packs::model::{GrepTarget, Rule, Target};
use regex::{Regex, RegexBuilder};

use crate::scope::{Roles, RuleScope};

/// A POSIX ERE in the `regex` crate's syntax.
pub fn ere_to_regex(ere: &str) -> String {
    let c: Vec<char> = ere.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '\\' if i + 1 < c.len() => {
                out.push('\\');
                out.push(c[i + 1]);
                i += 2;
                continue;
            }
            '[' => {
                // a bracket expression: up to the first `]` that is not its first member
                let mut j = i + 1;
                let mut body = String::from("[");
                if j < c.len() && c[j] == '^' {
                    body.push('^');
                    j += 1;
                }
                let mut first = true;
                let mut closed = false;
                while j < c.len() {
                    let ch = c[j];
                    if ch == ']' && !first {
                        closed = true;
                        break;
                    }
                    if ch == '[' && j + 1 < c.len() && c[j + 1] == ':' {
                        // a character class: [:space:]
                        if let Some(end) = (j + 2..c.len().saturating_sub(1))
                            .find(|&k| c[k] == ':' && c[k + 1] == ']')
                        {
                            body.extend(&c[j..end + 2]);
                            j = end + 2;
                            first = false;
                            continue;
                        }
                    }
                    match ch {
                        '[' | ']' | '\\' | '&' | '~' => {
                            body.push('\\');
                            body.push(ch);
                        }
                        '-' => {
                            // a range when between two members, else literal
                            let at_end = j + 1 < c.len() && c[j + 1] == ']';
                            if first || at_end {
                                body.push_str("\\-");
                            } else {
                                body.push('-');
                            }
                        }
                        _ => body.push(ch),
                    }
                    first = false;
                    j += 1;
                }
                if closed {
                    body.push(']');
                    out.push_str(&body);
                    i = j + 1;
                } else {
                    out.push_str("\\[");
                    i += 1;
                }
                continue;
            }
            '{' => {
                // an interval (`{2}`, `{1,3}`) stays; any other brace is literal
                let rest: String = c[i..].iter().collect();
                static INTERVAL: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
                    Regex::new(r"^\{[0-9]+(,[0-9]*)?\}").expect("regex")
                });
                match INTERVAL.find(&rest) {
                    Some(m) if !out.is_empty() => {
                        out.push_str(m.as_str());
                        i += m.as_str().chars().count();
                        continue;
                    }
                    _ => out.push_str("\\{"),
                }
            }
            '}' => out.push_str("\\}"),
            ch => out.push(ch),
        }
        i += 1;
    }
    out
}

/// A compiled grep target.
pub struct GrepRule {
    pub rule: Rule,
    pub target: GrepTarget,
    pub regex: Regex,
    pub scope: RuleScope,
}

pub fn compile(
    rule: &Rule,
    target: &GrepTarget,
    roles: &Roles,
    dirs: Option<&[String]>,
) -> Result<GrepRule, String> {
    let regex = RegexBuilder::new(&ere_to_regex(&target.pattern))
        .case_insensitive(target.ignore_case == Some(true))
        .build()
        .map_err(|e| format!("{}: grep pattern: {e}", rule.id))?;
    Ok(GrepRule {
        rule: rule.clone(),
        target: target.clone(),
        regex,
        scope: RuleScope::new(rule, roles, dirs, None),
    })
}

pub fn target_of(rule: &Rule) -> Option<&GrepTarget> {
    rule.enforce.iter().find_map(|t| match t {
        Target::Grep(g) => Some(g),
        _ => None,
    })
}

impl GrepRule {
    /// The 1-based numbers of the lines that match.
    pub fn matching_lines(&self, text: &str) -> Vec<u32> {
        text.lines()
            .enumerate()
            .filter(|(_, l)| self.regex.is_match(l))
            .map(|(i, _)| i as u32 + 1)
            .collect()
    }

    /// `grep -c`: how many lines match.
    pub fn count(&self, text: &str) -> usize {
        self.matching_lines(text).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn re(ere: &str) -> Regex {
        Regex::new(&ere_to_regex(ere)).unwrap_or_else(|e| panic!("{ere}: {e}"))
    }

    #[test]
    fn brackets_are_posix() {
        let r = re(r#""(black|isort)([[:space:]]*[]<>=!~;[]|")"#);
        assert!(r.is_match(r#""black>=1""#));
        assert!(r.is_match(r#""black[x]""#));
        assert!(r.is_match(r#""isort""#));
        assert!(!r.is_match(r#""blackx""#));
        let r = re(r"[^$[:space:]]x");
        assert!(r.is_match("ax"));
        assert!(!r.is_match("$x"));
        let r = re(r"a\.b{2}");
        assert!(r.is_match("a.bb"));
        let r = re(r"\{@html[[:space:]]");
        assert!(r.is_match("{@html x}"));
        let r = re(r"[a-z-]+");
        assert!(r.is_match("a-b"));
    }
}
