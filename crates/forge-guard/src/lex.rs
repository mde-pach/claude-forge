//! The plain tokenizer used when tree-sitter finds a syntax error: the splitting of the shell
//! guard it replaces. Commands end at `;`, `&`, `&&`, `||`, `|`, `|&`, newlines, `(`, `)`,
//! backticks and `$(`; single and double quotes and backslashes are honoured (an unterminated
//! quote runs to the end); `#` at a word start begins a comment; redirections take their
//! target; heredoc bodies (`<<EOF`, `<<-EOF`, quoted delimiters) and here-strings become the
//! command's input. Inside double quotes, `$(…)` stays text.

use crate::parse::{Raw, Redirect, Word};

/// The commands of `text`, and how many pipeline ids they use (from 0).
pub(crate) fn split(text: &str) -> (Vec<Raw>, u32) {
    let mut lx = Lexer::default();
    lx.run(text);
    (lx.out, lx.pipelines)
}

#[derive(Default)]
struct Lexer {
    out: Vec<Raw>,
    pipelines: u32,
    words: Vec<Word>,
    redirects: Vec<Redirect>,
    input: Option<String>,
    word: Word,
    in_word: bool,
    /// The next word is a redirection's target (its operator), or a here-string.
    target_of: Option<String>,
    /// Heredocs declared on the current line: (delimiter, strip tabs, command index).
    heredocs: Vec<(String, bool, usize)>,
    /// The pipeline the current command is a stage of.
    pipe: Option<(u32, u32)>,
}

impl Lexer {
    fn end_word(&mut self) {
        if !self.in_word {
            return;
        }
        let w = std::mem::take(&mut self.word);
        self.in_word = false;
        match self.target_of.take() {
            Some(op) if op == "<<<" => self.input = Some(w.text),
            Some(op) => self.redirects.push(Redirect { op, target: w }),
            None => self.words.push(w),
        }
    }

    /// Ends the current command; `piped`: the next one reads its output.
    fn end_command(&mut self, piped: bool) {
        self.end_word();
        self.target_of = None;
        let pipe = if piped {
            Some(self.pipe.unwrap_or_else(|| {
                self.pipelines += 1;
                (self.pipelines - 1, 0)
            }))
        } else {
            self.pipe
        };
        if !self.words.is_empty() || !self.redirects.is_empty() {
            self.out.push(Raw {
                assigns: vec![],
                words: std::mem::take(&mut self.words),
                redirects: std::mem::take(&mut self.redirects),
                input: self.input.take(),
                pipe,
            });
        } else {
            self.input = None;
        }
        self.pipe = if piped {
            pipe.map(|(p, s)| (p, s + 1))
        } else {
            None
        };
    }

    fn push(&mut self, c: char) {
        self.word.text.push(c);
        self.in_word = true;
    }

    fn run(&mut self, s: &str) {
        let cs: Vec<char> = s.chars().collect();
        let n = cs.len();
        let at = |i: usize| cs.get(i).copied().unwrap_or('\0');
        let mut i = 0;
        let mut quote: Option<char> = None;
        while i < n {
            let c = cs[i];
            let nx = at(i + 1);
            match quote {
                Some('\'') => {
                    if c == '\'' {
                        quote = None;
                    } else {
                        self.push(c);
                    }
                    i += 1;
                    continue;
                }
                Some(_) => {
                    if c == '\\' && matches!(nx, '$' | '`' | '"' | '\\') {
                        self.push(nx);
                        i += 2;
                        continue;
                    }
                    if c == '"' {
                        quote = None;
                    } else {
                        if c == '$' || c == '`' {
                            self.word.dynamic = true;
                        }
                        self.push(c);
                    }
                    i += 1;
                    continue;
                }
                None => {}
            }
            match c {
                '\\' => {
                    if nx != '\n' && nx != '\0' {
                        self.push(nx);
                    }
                    i += 2;
                }
                '\'' | '"' => {
                    quote = Some(c);
                    self.word.quoted = true;
                    self.in_word = true;
                    i += 1;
                }
                ' ' | '\t' => {
                    self.end_word();
                    i += 1;
                }
                '#' if !self.in_word => {
                    while i < n && cs[i] != '\n' {
                        i += 1;
                    }
                }
                '\n' => {
                    self.end_command(false);
                    i += 1;
                    // the heredoc bodies declared on that line
                    for (delim, strip, idx) in std::mem::take(&mut self.heredocs) {
                        let mut body = String::new();
                        while i < n {
                            let mut j = i;
                            while j < n && cs[j] != '\n' {
                                j += 1;
                            }
                            let line: String = cs[i..j].iter().collect();
                            i = (j + 1).min(n);
                            let line = if strip {
                                line.trim_start_matches('\t').to_owned()
                            } else {
                                line
                            };
                            if line == delim {
                                break;
                            }
                            body.push_str(&line);
                            body.push('\n');
                        }
                        if let Some(r) = self.out.get_mut(idx) {
                            r.input = Some(body);
                        }
                    }
                }
                '&' if nx == '>' => {
                    self.end_word();
                    let op = if at(i + 2) == '>' { "&>>" } else { "&>" };
                    i += op.len();
                    self.target_of = Some(op.to_owned());
                }
                ';' | '&' | '|' | '(' | ')' | '`' => {
                    let piped = c == '|' && nx != '|';
                    self.end_command(piped);
                    i += if ((c == '&' || c == '|') && nx == c) || (c == '|' && nx == '&') {
                        2
                    } else {
                        1
                    };
                }
                '$' if nx == '(' => {
                    self.end_command(false);
                    i += 2;
                }
                '<' | '>' => {
                    let mut op = String::new();
                    if self.in_word && self.word.text.chars().all(|d| d.is_ascii_digit()) {
                        op = std::mem::take(&mut self.word.text);
                        self.word = Word::default();
                        self.in_word = false;
                    } else {
                        self.end_word();
                    }
                    if c == '<' && nx == '(' {
                        self.end_command(false);
                        i += 2;
                        continue;
                    }
                    if c == '<' && nx == '<' {
                        if at(i + 2) == '<' {
                            i += 3;
                            self.target_of = Some("<<<".into());
                            continue;
                        }
                        i += 2;
                        let strip = at(i) == '-';
                        if strip {
                            i += 1;
                        }
                        while matches!(at(i), ' ' | '\t') {
                            i += 1;
                        }
                        let mut delim = String::new();
                        while i < n
                            && !matches!(
                                cs[i],
                                ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')'
                            )
                        {
                            if !matches!(cs[i], '\'' | '"' | '\\') {
                                delim.push(cs[i]);
                            }
                            i += 1;
                        }
                        if !delim.is_empty() {
                            self.heredocs.push((delim, strip, self.out.len()));
                        }
                        continue;
                    }
                    op.push(c);
                    i += 1;
                    if matches!(at(i), '>' | '|') && c == '>' {
                        op.push(at(i));
                        i += 1;
                    }
                    if at(i) == '&' {
                        op.push('&');
                        i += 1;
                    }
                    self.target_of = Some(op);
                }
                _ => {
                    if c == '$' {
                        self.word.dynamic = true;
                    }
                    self.push(c);
                    i += 1;
                }
            }
        }
        self.end_command(false);
    }
}

#[cfg(test)]
mod tests {
    use super::split;

    fn texts(s: &str) -> Vec<Vec<String>> {
        split(s)
            .0
            .into_iter()
            .map(|r| r.words.into_iter().map(|w| w.text).collect())
            .collect()
    }

    #[test]
    fn splits_like_the_shell() {
        assert_eq!(
            texts("a 'b c' && d \"e $(f)\"; g | h $(i)"),
            [
                vec!["a", "b c"],
                vec!["d", "e $(f)"],
                vec!["g"],
                vec!["h"],
                vec!["i"]
            ]
        );
        let (raws, n) = split("cat <<EOF | sh\nnpx x\nEOF\nls 'open");
        assert_eq!(n, 1);
        assert_eq!(raws[0].input.as_deref(), Some("npx x\n"));
        assert_eq!(raws[1].pipe, Some((0, 1)));
        assert_eq!(raws[2].words[1].text, "open");
    }
}
