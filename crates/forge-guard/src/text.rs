//! A simple command as one line of text, for the regexes of guard rules.
//!
//! The text is the command's assignments, words, redirections and input, in that order,
//! separated by single spaces, quoting removed. A word holding whitespace (a commit message,
//! a `-c` script, a heredoc body) is shown between single quotes, its content unchanged, so a
//! pattern that walks argument by argument (`([[:space:]]+[^[:space:]"']+)*`) cannot step into
//! it. Redirections are shown as `op target` (`> pnpm-lock.yaml`, `2>&1`), the input as
//! `<< 'body'`.

use crate::parse::{Redirect, Word};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    /// Byte offsets where a command starts: the start of the text, each assignment, each
    /// command name (the command, what a wrapper or a runner runs).
    pub positions: Vec<usize>,
    /// Byte ranges of the words shown between quotes (the quotes included).
    pub spaced: Vec<(usize, usize)>,
}

impl Rendered {
    pub fn of(
        assigns: &[(String, String)],
        words: &[Word],
        redirects: &[Redirect],
        input: Option<&str>,
        starts: &[usize],
    ) -> Self {
        let mut r = Rendered::default();
        r.positions.push(0);
        for (name, value) in assigns {
            r.sep();
            r.positions.push(r.text.len());
            r.text.push_str(name);
            r.text.push('=');
            r.word(value);
        }
        for (i, w) in words.iter().enumerate() {
            r.sep();
            if starts.contains(&i) {
                r.positions.push(r.text.len());
            }
            r.word(&w.text);
        }
        for d in redirects {
            r.sep();
            r.text.push_str(&d.op);
            if !d.op.ends_with('&') {
                r.text.push(' ');
            }
            r.word(&d.target.text);
        }
        if let Some(body) = input {
            r.sep();
            r.text.push_str("<< ");
            r.word(body);
        }
        r.positions.dedup();
        r
    }

    fn sep(&mut self) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
    }

    fn word(&mut self, s: &str) {
        if s.is_empty() || s.contains(char::is_whitespace) {
            let start = self.text.len();
            self.text.push('\'');
            self.text.push_str(s);
            self.text.push('\'');
            self.spaced.push((start, self.text.len()));
        } else {
            self.text.push_str(s);
        }
    }

    /// Whether byte `at` is inside a quoted word, after its opening quote.
    pub fn in_spaced(&self, at: usize) -> bool {
        self.spaced.iter().any(|&(a, b)| at > a && at < b)
    }

    /// The texts of `parts` joined by ` | `, with their positions and quoted ranges.
    pub fn join<'a>(parts: impl IntoIterator<Item = &'a Rendered>) -> Rendered {
        let mut out = Rendered::default();
        for p in parts {
            if !out.text.is_empty() {
                out.text.push_str(" | ");
            }
            let base = out.text.len();
            out.text.push_str(&p.text);
            out.positions.extend(p.positions.iter().map(|x| x + base));
            out.spaced
                .extend(p.spaced.iter().map(|(a, b)| (a + base, b + base)));
        }
        out
    }
}
