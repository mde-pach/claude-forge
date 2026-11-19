//! A Bash command line, as the simple commands it runs.
//!
//! tree-sitter-bash parses the text; the walk below turns its tree into [`SimpleCommand`]s:
//! assignments, words with their quoting removed, redirections, the heredoc or here-string fed
//! to the command, the directory it runs in, and where its command names start (the command
//! itself, then the command a wrapper or a runner runs: `sudo -u bob npx cowsay` starts at
//! `sudo`, `npx` and `cowsay`).
//!
//! What runs is followed: lists, pipelines, subshells, `$(…)`, backticks, `<(…)`, `if` /
//! `while` / `for` / `case` bodies and function bodies; `sh|bash|zsh|dash|ksh -c '…'`, `su -c`,
//! `eval`, `env -S`, each `find -exec|-execdir|-ok|-okdir … ;|+` and the command `xargs` runs;
//! a shell fed a heredoc, a here-string or `cat`/`echo` through a pipe (`sh <<EOF`,
//! `cat <<EOF | sh`) runs that text. Other heredoc bodies and quoted text are data: they stay
//! one word of their command. `cd` / `pushd` move the directory later commands run in; a
//! target with `$`, `` ` `` or a glob, `cd -` and `popd` cannot be followed.
//!
//! A tree with a syntax error (an unterminated quote, a stray `)`) is not trusted: the text is
//! split again by [`crate::lex`], a plain tokenizer, and the same rules apply to its commands.
//! Nothing here panics on any input; nesting deeper than [`MAX_DEPTH`] is not followed.

use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};

use tree_sitter::{Node, Parser};

use crate::text::Rendered;

/// Scripts inside scripts (`sh -c "eval '…'"`) are followed this deep.
pub const MAX_DEPTH: u32 = 8;

/// Where a command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dir {
    Known(PathBuf),
    /// After `cd "$X"`, `cd -`, `popd`…: it cannot be known before the command runs.
    Unknown,
}

impl Dir {
    pub fn known(&self) -> Option<&Path> {
        match self {
            Dir::Known(p) => Some(p),
            Dir::Unknown => None,
        }
    }
}

/// A word with its quoting removed. Expansions (`$X`, `${X}`, `$(…)`) are kept as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    /// Some part of it was quoted.
    pub quoted: bool,
    /// It holds an expansion or a substitution outside single quotes: its value is known only
    /// when the command runs.
    pub dynamic: bool,
}

impl Word {
    pub fn plain(text: impl Into<String>) -> Self {
        Word {
            text: text.into(),
            ..Word::default()
        }
    }
}

/// A redirection: `>`, `>>`, `<`, `2>`, `&>`, `>&`…, and its target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// The descriptor and the operator, as written (`2>`, `>>`, `&>`).
    pub op: String,
    pub target: Word,
}

impl Redirect {
    /// Whether it writes a file (not a descriptor copy such as `2>&1`).
    pub fn writes_file(&self) -> bool {
        let op = self.op.trim_start_matches(|c: char| c.is_ascii_digit());
        matches!(op, ">" | ">>" | ">|" | "&>" | "&>>" | "<>")
            || (op == ">&"
                && !self
                    .target
                    .text
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '-'))
    }
}

/// One simple command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleCommand {
    /// `NAME=value` before the command (and after `env`), in order.
    pub assigns: Vec<(String, String)>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    /// A heredoc body or here-string fed to it.
    pub input: Option<String>,
    pub dir: Dir,
    /// Indices into `words` where a command name starts: the command, then what each wrapper
    /// or runner runs. Empty for a bare assignment.
    pub starts: Vec<usize>,
    /// The pipeline it is a stage of, and its stage: the next stage reads its output.
    pub pipe: Option<(u32, u32)>,
    /// It comes from a nested script (`sh -c`, `eval`, `find -exec`…).
    pub nested: bool,
    pub rendered: Rendered,
    effective: Option<usize>,
}

impl SimpleCommand {
    /// The command a wrapper runs: the last of the wrappers' starts (before any runner).
    pub fn effective(&self) -> Option<usize> {
        self.effective
    }

    /// The basename of the word at `i`.
    pub fn name_at(&self, i: usize) -> &str {
        basename(&self.words[i].text)
    }

    /// The words from `i` on, as text.
    pub fn texts_from(&self, i: usize) -> impl Iterator<Item = &str> {
        self.words[i.min(self.words.len())..]
            .iter()
            .map(|w| w.text.as_str())
    }
}

impl SimpleCommand {
    #[allow(clippy::too_many_arguments)]
    fn build(
        assigns: Vec<(String, String)>,
        words: Vec<Word>,
        redirects: Vec<Redirect>,
        input: Option<String>,
        dir: Dir,
        starts: Vec<usize>,
        effective: Option<usize>,
        pipe: Option<(u32, u32)>,
        nested: bool,
    ) -> Self {
        let rendered = Rendered::of(&assigns, &words, &redirects, input.as_deref(), &starts);
        SimpleCommand {
            assigns,
            words,
            redirects,
            input,
            dir,
            starts,
            pipe,
            nested,
            rendered,
            effective,
        }
    }
}

/// The command line, parsed.
#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub commands: Vec<SimpleCommand>,
    /// tree-sitter found a syntax error: the commands come from the plain tokenizer.
    pub fallback: bool,
    /// `NAME=value` assignments that are statements of their own (`N=npx; $N cowsay`).
    pub variables: Vec<(String, String)>,
}

impl Parsed {
    /// The stages of pipeline `id` after `stage`, in order.
    pub fn next_stage(&self, pipe: (u32, u32)) -> Option<&SimpleCommand> {
        self.commands
            .iter()
            .find(|c| c.pipe == Some((pipe.0, pipe.1 + 1)))
    }

    /// The value of a variable assigned as a statement, the last assignment winning.
    pub fn variable(&self, name: &str) -> Option<&str> {
        self.variables
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Where parsing starts.
#[derive(Debug, Clone)]
pub struct Start {
    pub cwd: Dir,
    pub home: Option<PathBuf>,
}

thread_local! {
    static PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
}

fn with_parser<T>(f: impl FnOnce(&mut Parser) -> T) -> T {
    PARSER.with(|p| {
        let mut p = p.borrow_mut();
        let parser = p.get_or_insert_with(|| {
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_bash::LANGUAGE.into())
                .expect("the bash grammar matches tree-sitter's ABI");
            parser
        });
        f(parser)
    })
}

/// Parses `command`, run from `start`.
pub fn parse(command: &str, start: &Start) -> Parsed {
    let mut w = Walker {
        out: Parsed::default(),
        dir: start.cwd.clone(),
        home: start.home.clone(),
        depth: 0,
        next_pipe: 0,
        pending: None,
        stage: None,
        nested: false,
    };
    w.script(command);
    w.out
}

/// Redirections and input waiting for the command of a redirected statement.
#[derive(Default)]
struct Pending {
    redirects: Vec<Redirect>,
    input: Option<String>,
}

struct Walker {
    out: Parsed,
    dir: Dir,
    home: Option<PathBuf>,
    depth: u32,
    next_pipe: u32,
    pending: Option<Pending>,
    /// The pipeline stage the next command built at this level is.
    stage: Option<(u32, u32)>,
    nested: bool,
}

/// A command before its wrappers and runners are read.
pub(crate) struct Raw {
    pub assigns: Vec<(String, String)>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    pub input: Option<String>,
    pub pipe: Option<(u32, u32)>,
}

impl Walker {
    /// Parses and walks a script: the command line, or the text a nested shell runs.
    fn script(&mut self, text: &str) {
        if self.depth > MAX_DEPTH {
            return;
        }
        let tree = with_parser(|p| p.parse(text, None));
        let Some(tree) = tree else {
            self.fallback(text);
            return;
        };
        let root = tree.root_node();
        if root.has_error() {
            self.fallback(text);
            return;
        }
        self.stmt(root, text.as_bytes());
    }

    fn fallback(&mut self, text: &str) {
        self.out.fallback = true;
        let (raws, pipelines) = crate::lex::split(text);
        let base = self.next_pipe;
        self.next_pipe += pipelines;
        for mut raw in raws {
            raw.pipe = raw.pipe.map(|(p, s)| (p + base, s));
            self.emit(raw);
        }
    }

    /// Runs `f` as a nested script would run: in its own directory (unless `keep_dir`), with
    /// no pending redirection or pipeline stage.
    fn nested<T>(&mut self, keep_dir: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        let dir = self.dir.clone();
        let pending = self.pending.take();
        let stage = self.stage.take();
        let nested = std::mem::replace(&mut self.nested, true);
        self.depth += 1;
        let t = f(self);
        self.depth -= 1;
        self.nested = nested;
        self.stage = stage;
        self.pending = pending;
        if !keep_dir {
            self.dir = dir;
        }
        t
    }

    fn stmt(&mut self, n: Node, src: &[u8]) {
        match n.kind() {
            "command" => self.command(n, src),
            "redirected_statement" => self.redirected(n, src),
            "pipeline" => self.pipeline(n, src),
            "subshell" => self.nested(false, |w| w.children(n, src)),
            "variable_assignment" => {
                let (name, value) = self.assignment(n, src);
                self.out.variables.push((name.clone(), value.clone()));
                let pipe = self.stage.take();
                self.emit(Raw {
                    assigns: vec![(name, value)],
                    words: vec![],
                    redirects: vec![],
                    input: None,
                    pipe,
                });
            }
            "declaration_command" | "unset_command" => self.declaration(n, src),
            "comment" | "heredoc_body" | "heredoc_start" | "heredoc_end" => {}
            "command_substitution" | "process_substitution" => {
                self.nested(false, |w| w.children(n, src))
            }
            // words in for lists, case values, test expressions: only their substitutions run
            "word" | "string" | "concatenation" | "raw_string" | "simple_expansion"
            | "expansion" | "number" => {
                self.word(n, src);
            }
            _ => self.children(n, src),
        }
    }

    fn children(&mut self, n: Node, src: &[u8]) {
        let mut c = n.walk();
        let kids: Vec<Node> = n.named_children(&mut c).collect();
        for k in kids {
            self.stmt(k, src);
        }
    }

    fn pipeline(&mut self, n: Node, src: &[u8]) {
        let id = self.next_pipe;
        self.next_pipe += 1;
        let mut c = n.walk();
        let stages: Vec<Node> = n
            .named_children(&mut c)
            .filter(|k| k.kind() != "comment")
            .collect();
        let several = stages.len() > 1;
        for (i, k) in stages.into_iter().enumerate() {
            let dir = self.dir.clone();
            self.stage = Some((id, i as u32));
            self.stmt(k, src);
            self.stage = None;
            // each stage of a pipeline runs in a subshell
            if several {
                self.dir = dir;
            }
        }
    }

    fn redirected(&mut self, n: Node, src: &[u8]) {
        let mut pending = Pending::default();
        let mut continuation: Vec<Node> = Vec::new();
        let mut right: Vec<Node> = Vec::new();
        let mut body = None;
        for i in 0..n.child_count() {
            let Some(k) = n.child(i) else { continue };
            match n.field_name_for_child(i) {
                Some("body") => body = Some(k),
                Some("redirect") => match k.kind() {
                    "heredoc_redirect" => {
                        self.heredoc(k, src, &mut pending, &mut continuation, &mut right)
                    }
                    "herestring_redirect" => pending.input = Some(self.herestring(k, src)),
                    _ => {
                        if let Some(r) = self.file_redirect(k, src) {
                            pending.redirects.push(r);
                        }
                    }
                },
                _ => {
                    if k.kind() == "herestring_redirect" {
                        pending.input = Some(self.herestring(k, src));
                    }
                }
            }
        }
        let Some(body) = body else { return };
        // a heredoc piped on (`cat <<EOF | sh`): the body command is the pipeline's first stage
        let pipe = if continuation.is_empty() {
            None
        } else {
            let id = self.next_pipe;
            self.next_pipe += 1;
            Some(id)
        };
        self.body_with(body, src, pending, pipe);
        if let Some(id) = pipe {
            for (i, k) in continuation.into_iter().enumerate() {
                let dir = self.dir.clone();
                self.stage = Some((id, i as u32 + 1));
                self.stmt(k, src);
                self.stage = None;
                self.dir = dir;
            }
        }
        for k in right {
            self.stmt(k, src);
        }
    }

    /// Walks the body of a redirected statement, its redirections and input going to the
    /// command they apply to: the command itself, a pipeline's last stage, or (tree-sitter
    /// attaches them to a whole `a && b <<EOF` list) the list's last command.
    fn body_with(&mut self, body: Node, src: &[u8], pending: Pending, pipe: Option<u32>) {
        match body.kind() {
            "command" => {
                self.pending = Some(pending);
                if let Some(id) = pipe {
                    self.stage = Some((id, 0));
                }
                self.command(body, src);
                self.pending = None;
            }
            "pipeline" => {
                // the redirections are the last stage's
                let id = self.next_pipe;
                self.next_pipe += 1;
                let mut c = body.walk();
                let stages: Vec<Node> = body.named_children(&mut c).collect();
                let last = stages.len().saturating_sub(1);
                let mut pending = Some(pending);
                for (i, k) in stages.into_iter().enumerate() {
                    let dir = self.dir.clone();
                    self.stage = Some((id, i as u32));
                    if i == last && k.kind() == "command" {
                        self.pending = pending.take();
                    }
                    self.stmt(k, src);
                    self.pending = None;
                    self.stage = None;
                    self.dir = dir;
                }
            }
            "list" => {
                let mut c = body.walk();
                let kids: Vec<Node> = body.named_children(&mut c).collect();
                if let Some((last, init)) = kids.split_last() {
                    for k in init {
                        self.stmt(*k, src);
                    }
                    self.body_with(*last, src, pending, pipe);
                }
            }
            _ => self.stmt(body, src),
        }
    }

    fn heredoc<'t>(
        &mut self,
        n: Node<'t>,
        src: &[u8],
        pending: &mut Pending,
        continuation: &mut Vec<Node<'t>>,
        right: &mut Vec<Node<'t>>,
    ) {
        let mut strip_tabs = false;
        for i in 0..n.child_count() {
            let Some(k) = n.child(i) else { continue };
            match (n.field_name_for_child(i), k.kind()) {
                (_, "<<-") => strip_tabs = true,
                (_, "heredoc_body") => {
                    let body = text(k, src);
                    pending.input = Some(if strip_tabs {
                        body.lines()
                            .map(|l| l.trim_start_matches('\t'))
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        body.to_owned()
                    });
                }
                (Some("redirect"), _) => {
                    if let Some(r) = self.file_redirect(k, src) {
                        pending.redirects.push(r);
                    }
                }
                (Some("right"), _) => right.push(k),
                (_, "pipeline") => {
                    let mut c = k.walk();
                    continuation.extend(k.named_children(&mut c));
                }
                (None, kind) if k.is_named() && !kind.starts_with("heredoc") => {
                    // a command after the delimiter word on the same line (`<<EOF; ls`)
                    right.push(k)
                }
                _ => {}
            }
        }
    }

    fn herestring(&mut self, n: Node, src: &[u8]) -> String {
        let mut c = n.walk();
        let kids: Vec<Node> = n.named_children(&mut c).collect();
        let mut out = String::new();
        for k in kids {
            if k.kind() != "file_descriptor" {
                out.push_str(&self.word(k, src).text);
            }
        }
        out
    }

    fn file_redirect(&mut self, n: Node, src: &[u8]) -> Option<Redirect> {
        let mut op = String::new();
        let mut target = None;
        for i in 0..n.child_count() {
            let Some(k) = n.child(i) else { continue };
            match n.field_name_for_child(i) {
                Some("descriptor") => op.insert_str(0, text(k, src)),
                Some("destination") => {
                    let w = self.word(k, src);
                    target = Some(match target.take() {
                        None => w,
                        Some(mut t) => {
                            let t2: &mut Word = &mut t;
                            t2.text.push_str(&w.text);
                            t
                        }
                    });
                }
                _ if !k.is_named() => op.push_str(text(k, src)),
                _ => {}
            }
        }
        Some(Redirect {
            op,
            target: target.unwrap_or_default(),
        })
    }

    fn assignment(&mut self, n: Node, src: &[u8]) -> (String, String) {
        let name = n
            .child_by_field_name("name")
            .map(|k| text(k, src).to_owned())
            .unwrap_or_default();
        let value = n
            .child_by_field_name("value")
            .map(|k| self.word(k, src).text)
            .unwrap_or_default();
        // `a+=x`: the operator is part of the name's text in neither; keep the name only
        (name, value)
    }

    fn declaration(&mut self, n: Node, src: &[u8]) {
        // export / declare / local / readonly / typeset / unset: a command whose words are
        // its arguments, assignments included as `NAME=value`
        let mut words = Vec::new();
        let mut assigns = Vec::new();
        for i in 0..n.child_count() {
            let Some(k) = n.child(i) else { continue };
            if !k.is_named() {
                words.push(Word::plain(text(k, src)));
                continue;
            }
            match k.kind() {
                "variable_assignment" => {
                    let (name, value) = self.assignment(k, src);
                    words.push(Word::plain(format!("{name}={value}")));
                    assigns.push((name, value));
                }
                "comment" => {}
                _ => {
                    let w = self.word(k, src);
                    words.push(w);
                }
            }
        }
        let pipe = self.stage.take();
        let pending = self.pending.take().unwrap_or_default();
        self.out.variables.extend(assigns.iter().cloned());
        self.emit(Raw {
            assigns: vec![],
            words,
            redirects: pending.redirects,
            input: pending.input,
            pipe,
        });
    }

    fn command(&mut self, n: Node, src: &[u8]) {
        let pipe = self.stage.take();
        let pending = self.pending.take().unwrap_or_default();
        let mut assigns = Vec::new();
        let mut words = Vec::new();
        let mut redirects = pending.redirects;
        let mut input = pending.input;
        for i in 0..n.child_count() {
            let Some(k) = n.child(i) else { continue };
            let field = n.field_name_for_child(i);
            match k.kind() {
                "variable_assignment" => assigns.push(self.assignment(k, src)),
                "command_name" => {
                    let mut c = k.walk();
                    let inner: Vec<Node> = k.named_children(&mut c).collect();
                    let mut w = Word::default();
                    for x in inner {
                        let p = self.word(x, src);
                        w.text.push_str(&p.text);
                        w.quoted |= p.quoted;
                        w.dynamic |= p.dynamic;
                    }
                    words.push(w);
                }
                "file_redirect" => {
                    if let Some(r) = self.file_redirect(k, src) {
                        redirects.push(r);
                    }
                }
                "herestring_redirect" => input = Some(self.herestring(k, src)),
                "comment" => {}
                _ if k.is_named() => {
                    let w = self.word(k, src);
                    words.push(w);
                }
                // `==`, `=~` and `$` arguments are anonymous nodes
                _ if field == Some("argument") => words.push(Word::plain(text(k, src))),
                _ => {}
            }
        }
        self.emit(Raw {
            assigns,
            words,
            redirects,
            input,
            pipe,
        });
    }

    /// A word: its text without quotes; the substitutions in it are walked as commands.
    fn word(&mut self, n: Node, src: &[u8]) -> Word {
        let mut w = Word::default();
        self.word_parts(n, src, &mut w);
        w
    }

    fn word_parts(&mut self, n: Node, src: &[u8], w: &mut Word) {
        let t = text(n, src);
        match n.kind() {
            "word" | "extglob_pattern" | "regex" | "variable_name" => {
                w.text.push_str(&unescape_bare(t));
                if t.contains('$') || t.contains('`') {
                    w.dynamic = true;
                }
            }
            "raw_string" => {
                w.quoted = true;
                w.text.push_str(strip(t, 1, 1));
            }
            "ansi_c_string" => {
                w.quoted = true;
                w.text.push_str(&unescape_ansi(strip(t, 2, 1)));
            }
            "string" | "translated_string" => {
                w.quoted = true;
                let inner = if n.kind() == "string" {
                    strip(t, 1, 1)
                } else {
                    strip(t, 2, 1)
                };
                w.text.push_str(&unescape_dquoted(inner));
                let mut c = n.walk();
                let kids: Vec<Node> = n.named_children(&mut c).collect();
                for k in kids {
                    match k.kind() {
                        "string_content" => {}
                        "command_substitution" | "process_substitution" => {
                            w.dynamic = true;
                            self.nested(false, |s| s.children(k, src));
                        }
                        _ => {
                            w.dynamic = true;
                            self.substitutions_in(k, src);
                        }
                    }
                }
            }
            "simple_expansion" | "expansion" | "arithmetic_expansion" => {
                w.dynamic = true;
                w.text.push_str(t);
                self.substitutions_in(n, src);
            }
            "command_substitution" | "process_substitution" => {
                w.dynamic = true;
                w.text.push_str(t);
                self.nested(false, |s| s.children(n, src));
            }
            "concatenation" => {
                let mut last = n.start_byte();
                for i in 0..n.child_count() {
                    let Some(k) = n.child(i) else { continue };
                    if k.start_byte() > last {
                        w.text
                            .push_str(&unescape_bare(slice(src, last, k.start_byte())));
                    }
                    if k.is_named() {
                        self.word_parts(k, src, w);
                    } else {
                        w.text.push_str(&unescape_bare(text(k, src)));
                    }
                    last = k.end_byte();
                }
                if n.end_byte() > last {
                    w.text
                        .push_str(&unescape_bare(slice(src, last, n.end_byte())));
                }
            }
            _ => {
                w.text.push_str(t);
                if t.contains('$') || t.contains('`') {
                    w.dynamic = true;
                }
                self.substitutions_in(n, src);
            }
        }
    }

    /// Walks the command substitutions anywhere below `n`.
    fn substitutions_in(&mut self, n: Node, src: &[u8]) {
        let mut c = n.walk();
        let kids: Vec<Node> = n.named_children(&mut c).collect();
        for k in kids {
            match k.kind() {
                "command_substitution" | "process_substitution" => {
                    self.nested(false, |s| s.children(k, src))
                }
                _ => self.substitutions_in(k, src),
            }
        }
    }

    /// Reads a command's wrappers and runners, follows what it runs, and records it.
    fn emit(&mut self, raw: Raw) {
        let Raw {
            mut assigns,
            words,
            redirects,
            input,
            pipe,
        } = raw;
        let n = words.len();
        let mut dir = self.dir.clone();
        let mut starts = Vec::new();
        let mut k = 0;
        // the tokenizer leaves assignments among the words
        while k < n && is_assignment(&words[k].text) && !words[k].quoted {
            let (a, b) = words[k].text.split_once('=').unwrap_or_default();
            assigns.push((a.to_owned(), b.to_owned()));
            k += 1;
        }
        let mut nested_script: Option<(String, bool)> = None;
        let mut nested_words: Vec<Vec<Word>> = Vec::new();
        'wrappers: while k < n {
            starts.push(k);
            let b = basename(&words[k].text);
            if !WRAPPERS.contains(&b) {
                break;
            }
            // `command -v x`, `command -V x` only look a name up
            if b == "command"
                && words
                    .get(k + 1)
                    .is_some_and(|w| w.text == "-v" || w.text == "-V")
            {
                break;
            }
            k += 1;
            while k < n && words[k].text.starts_with('-') && words[k].text.len() > 1 {
                let o = words[k].text.as_str();
                if o == "--" {
                    k += 1;
                    break;
                }
                if b == "env" && (o == "-S" || o == "--split-string") {
                    nested_script = Some((join_texts(&words[k + 1..]), false));
                    break 'wrappers;
                }
                if b == "env" && (o.starts_with("-S") || o.starts_with("--split-string=")) {
                    let first = o
                        .strip_prefix("--split-string=")
                        .or_else(|| o.strip_prefix("-S"))
                        .unwrap_or_default();
                    let mut s = first.to_owned();
                    for w in &words[k + 1..] {
                        s.push(' ');
                        s.push_str(&w.text);
                    }
                    nested_script = Some((s, false));
                    break 'wrappers;
                }
                let t = wrapper_option(b, o);
                if t == 2 {
                    dir = match words.get(k + 1) {
                        Some(w) => self.chdir(&dir, w),
                        None => Dir::Unknown,
                    };
                } else if (b == "sudo" && (o.starts_with("-D") || o.starts_with("--chdir=")))
                    || (b == "env" && (o.starts_with("-C") || o.starts_with("--chdir=")))
                {
                    let t = o
                        .strip_prefix("--chdir=")
                        .or_else(|| o.strip_prefix("-D"))
                        .or_else(|| o.strip_prefix("-C"))
                        .unwrap_or_default();
                    dir = self.chdir(&dir, &Word::plain(t));
                }
                if t > 0 {
                    k += 1;
                }
                k += 1;
            }
            // env NAME=value cmd
            while k < n && is_assignment(&words[k].text) {
                let (a, v) = words[k].text.split_once('=').unwrap_or_default();
                assigns.push((a.to_owned(), v.to_owned()));
                k += 1;
            }
            if b == "timeout" && k < n {
                k += 1;
            }
        }
        if starts.last().is_some_and(|&s| s >= n) {
            starts.pop();
        }
        let effective = starts.last().copied().filter(|&s| s < n);
        // runners: npx X, uv run X, python X.py…
        if let Some(mut e) = effective {
            while let Some(i) = crate::runners::inner(&words, e) {
                if i >= n || starts.contains(&i) {
                    break;
                }
                starts.push(i);
                e = i;
            }
        }
        let mut cd_to: Option<Dir> = None;
        let mut shell_input = false;
        if nested_script.is_none()
            && let Some(e) = effective
        {
            let b = basename(&words[e].text);
            match b {
                "cd" | "pushd" => {
                    let target = words[e + 1..]
                        .iter()
                        .find(|w| !(w.text.starts_with('-') && w.text.len() > 1));
                    cd_to = Some(match target {
                        Some(w) => self.chdir(&dir, w),
                        None => match &self.home {
                            Some(h) => Dir::Known(h.clone()),
                            None => Dir::Unknown,
                        },
                    });
                }
                "popd" => cd_to = Some(Dir::Unknown),
                "eval" => nested_script = Some((join_texts(&words[e + 1..]), true)),
                "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash" | "busybox" | "su" => {
                    let from = if b == "busybox" {
                        if words
                            .get(e + 1)
                            .is_some_and(|w| w.text == "sh" || w.text == "ash")
                        {
                            e + 2
                        } else {
                            n
                        }
                    } else {
                        e + 1
                    };
                    let mut script_file = false;
                    let mut j = from;
                    while j < n {
                        let o = &words[j].text;
                        if o.starts_with('-') && o.len() > 1 && !o.starts_with("--") {
                            if o[1..].chars().all(|c| c.is_ascii_alphabetic()) && o.contains('c') {
                                if let Some(s) = words.get(j + 1) {
                                    nested_script = Some((s.text.clone(), false));
                                }
                                break;
                            }
                        } else if o == "--" {
                        } else if !o.starts_with("--") {
                            if b != "su" {
                                script_file = true;
                            }
                            break;
                        }
                        j += 1;
                    }
                    if nested_script.is_none() && !script_file && from < n + 1 {
                        shell_input = true;
                    }
                }
                "find" => {
                    let mut j = e + 1;
                    while j < n {
                        if matches!(
                            words[j].text.as_str(),
                            "-exec" | "-execdir" | "-ok" | "-okdir"
                        ) {
                            let mut o = j + 1;
                            while o < n && words[o].text != ";" && words[o].text != "+" {
                                o += 1;
                            }
                            if o > j + 1 {
                                nested_words.push(words[j + 1..o].to_vec());
                            }
                            j = o;
                        }
                        j += 1;
                    }
                }
                _ => {}
            }
        }
        // what a shell reads on its input: a heredoc, a here-string, or `cat`/`echo` piped in
        if shell_input {
            if let Some(text) = &input {
                nested_script = Some((text.clone(), false));
            } else if let Some((p, s)) = pipe
                && s > 0
                && let Some(prev) = self
                    .out
                    .commands
                    .iter()
                    .rev()
                    .find(|c| c.pipe == Some((p, s - 1)))
                && let Some(pe) = prev.effective()
            {
                match prev.name_at(pe) {
                    "cat" if prev.words.len() == pe + 1 => {
                        nested_script = prev.input.clone().map(|t| (t, false))
                    }
                    "echo" => {
                        let args: Vec<&Word> = prev.words[pe + 1..]
                            .iter()
                            .skip_while(|w| w.text.starts_with('-'))
                            .collect();
                        nested_script = Some((
                            args.iter()
                                .map(|w| w.text.as_str())
                                .collect::<Vec<_>>()
                                .join(" "),
                            false,
                        ));
                    }
                    _ => {}
                }
            }
        }
        let cmd = SimpleCommand::build(
            assigns,
            words,
            redirects,
            input,
            dir.clone(),
            starts,
            effective,
            pipe,
            self.nested,
        );
        self.out.commands.push(cmd);
        if let Some((script, keep_dir)) = nested_script {
            let saved = self.dir.clone();
            self.dir = dir.clone();
            self.nested(keep_dir, |w| w.script(&script));
            if !keep_dir {
                self.dir = saved;
            }
        }
        for ws in nested_words {
            let saved = self.dir.clone();
            self.dir = dir.clone();
            self.nested(false, |w| {
                w.emit(Raw {
                    assigns: vec![],
                    words: ws,
                    redirects: vec![],
                    input: None,
                    pipe: None,
                })
            });
            self.dir = saved;
        }
        if let Some(d) = cd_to {
            self.dir = d;
        }
    }

    fn chdir(&self, base: &Dir, t: &Word) -> Dir {
        chdir(base, t, self.home.as_deref())
    }
}

/// The directory after `cd <t>` from `base`.
pub fn chdir(base: &Dir, t: &Word, home: Option<&Path>) -> Dir {
    let Dir::Known(base) = base else {
        return Dir::Unknown;
    };
    let s = t.text.as_str();
    if t.dynamic && !(s.starts_with("$HOME") || s.starts_with("${HOME}")) {
        return Dir::Unknown;
    }
    if s == "-" || s.contains(['*', '?', '`']) {
        return Dir::Unknown;
    }
    let home_rest = if s.is_empty() || s == "~" || s == "$HOME" || s == "${HOME}" {
        Some("")
    } else {
        s.strip_prefix("~/")
            .or_else(|| s.strip_prefix("$HOME/"))
            .or_else(|| s.strip_prefix("${HOME}/"))
    };
    if s.contains('$') && home_rest.is_none() {
        return Dir::Unknown;
    }
    let p = match home_rest {
        Some(rest) => match home {
            Some(h) => h.join(rest),
            None => return Dir::Unknown,
        },
        None if s.starts_with('/') => PathBuf::from(s),
        None => base.join(s),
    };
    Dir::Known(normalize(&p))
}

/// `p` without `.` and `..` components (lexically, as the shell's `cd` does).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(x) => out.push(x),
            _ => {}
        }
    }
    out
}

/// Commands that run the command after their own options.
pub const WRAPPERS: &[&str] = &[
    "sudo",
    "doas",
    "env",
    "time",
    "nohup",
    "nice",
    "ionice",
    "exec",
    "command",
    "builtin",
    "xargs",
    "stdbuf",
    "timeout",
    "chronic",
    "caffeinate",
];

/// 1 when the wrapper's option takes the next word as its argument, 2 when that argument is a
/// directory the command then runs in.
fn wrapper_option(b: &str, o: &str) -> u8 {
    let one = |set: &[&str]| u8::from(set.contains(&o));
    match b {
        "sudo" => {
            if o == "-D" || o == "--chdir" {
                2
            } else {
                one(&[
                    "-u",
                    "-g",
                    "-p",
                    "-C",
                    "-r",
                    "-t",
                    "-U",
                    "-h",
                    "-R",
                    "-T",
                    "--user",
                    "--group",
                    "--prompt",
                    "--close-from",
                    "--role",
                    "--type",
                    "--other-user",
                    "--host",
                    "--command-timeout",
                ])
            }
        }
        "doas" => one(&["-u", "-C"]),
        "env" => {
            if o == "-C" || o == "--chdir" {
                2
            } else {
                one(&["-u", "--unset", "-P"])
            }
        }
        "nice" => one(&["-n", "--adjustment"]),
        "ionice" => one(&["-c", "-n", "-p"]),
        "timeout" => one(&["-s", "--signal", "-k", "--kill-after"]),
        "time" => one(&["-f", "--format", "-o", "--output"]),
        "exec" => one(&["-a"]),
        "xargs" => one(&[
            "-I",
            "-i",
            "-L",
            "-n",
            "-P",
            "-s",
            "-d",
            "-E",
            "-e",
            "-a",
            "-J",
            "-R",
            "--arg-file",
            "--max-args",
            "--max-procs",
            "--max-chars",
            "--delimiter",
            "--eof",
            "--replace",
            "--max-lines",
        ]),
        "stdbuf" => one(&["-i", "-o", "-e"]),
        _ => 0,
    }
}

pub fn is_assignment(s: &str) -> bool {
    let Some((name, _)) = s.split_once('=') else {
        return false;
    };
    let name = name.strip_suffix('+').unwrap_or(name);
    let mut cs = name.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The last path component of a command name (`/usr/bin/git` → `git`).
pub fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

fn join_texts(ws: &[Word]) -> String {
    ws.iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    slice(src, n.start_byte(), n.end_byte())
}

fn slice(src: &[u8], a: usize, b: usize) -> &str {
    src.get(a..b)
        .and_then(|s| std::str::from_utf8(s).ok())
        .unwrap_or("")
}

fn strip(s: &str, front: usize, back: usize) -> &str {
    if s.len() >= front + back {
        s.get(front..s.len() - back).unwrap_or(s)
    } else {
        s
    }
}

/// An unquoted word: `\x` is `x`, a backslash-newline is nothing.
pub fn unescape_bare(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        if c == '\\' {
            match cs.next() {
                Some('\n') | None => {}
                Some(d) => out.push(d),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Inside double quotes, a backslash escapes only `$`, `` ` ``, `"`, `\` and newline.
pub fn unescape_dquoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut cs = s.chars().peekable();
    while let Some(c) = cs.next() {
        if c == '\\' {
            match cs.peek() {
                Some('$' | '`' | '"' | '\\') => out.push(cs.next().unwrap_or('\\')),
                Some('\n') => {
                    cs.next();
                }
                _ => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `$'…'`: the C escapes that matter here.
fn unescape_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match cs.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('e' | 'E') => out.push('\u{1b}'),
            Some(d) => out.push(d),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(s: &str) -> Parsed {
        parse(
            s,
            &Start {
                cwd: Dir::Known("/r".into()),
                home: Some("/h".into()),
            },
        )
    }

    fn words(p: &Parsed) -> Vec<Vec<&str>> {
        p.commands
            .iter()
            .map(|c| c.words.iter().map(|w| w.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn quoting_heredocs_and_substitutions() {
        let p = cmds("git commit -m \"fix $(date) x\" a\\ b && cat <<'EOF' > f\nnpx x\nEOF");
        assert!(!p.fallback);
        assert_eq!(
            words(&p),
            [
                vec!["date"],
                vec!["git", "commit", "-m", "fix $(date) x", "a b"],
                vec!["cat"]
            ]
        );
        assert_eq!(p.commands[2].input.as_deref(), Some("npx x\n"));
        assert_eq!(p.commands[2].redirects[0].target.text, "f");
    }

    #[test]
    fn wrappers_runners_and_directories() {
        let p = cmds("cd packages/app && sudo -u bob nice -n 5 npx -y prisma migrate; cd ~; ls");
        let c = &p.commands[1];
        assert_eq!(c.starts, [0, 3, 6, 8]);
        assert_eq!(c.dir, Dir::Known("/r/packages/app".into()));
        assert_eq!(p.commands[3].dir, Dir::Known("/h".into()));
        let p = cmds("cd \"$X\" && ls");
        assert_eq!(p.commands[1].dir, Dir::Unknown);
    }

    #[test]
    fn nested_scripts() {
        let p = cmds(
            "bash -c 'git push -f' && eval \"rm -rf x\" && find . -exec rm {} \\; && env -S 'ls -l'",
        );
        let names: Vec<&str> = p
            .commands
            .iter()
            .map(|c| c.words[0].text.as_str())
            .collect();
        assert_eq!(
            names,
            ["bash", "git", "eval", "rm", "find", "rm", "env", "ls"]
        );
        assert!(p.commands[1].nested);
    }

    #[test]
    fn pipelines() {
        let p = cmds("curl -s x | sudo bash");
        assert_eq!(p.commands[0].pipe, Some((0, 0)));
        assert_eq!(p.next_stage((0, 0)).map(|c| c.starts.len()), Some(2));
    }
}
