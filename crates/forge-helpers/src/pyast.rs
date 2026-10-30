//! Python syntax trees (tree-sitter, through ast-grep), read the way the original helpers read
//! Python's `ast`: module and class bodies, definitions with their decorators, assignments with
//! their targets, calls with positional and keyword arguments, string constants.

use std::collections::VecDeque;

use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_core::{AstGrep, Node};
use ast_grep_language::Python;

pub type Root = AstGrep<StrDoc<Python>>;
pub type N<'r> = Node<'r, StrDoc<Python>>;

/// The tree of a module; None when it has a syntax error (Python would not import it).
pub fn parse(text: &str) -> Option<Root> {
    let root = Python.ast_grep(text);
    let bad = root.root().dfs().any(|n| n.is_error() || n.is_missing());
    (!bad).then_some(root)
}

/// The 1-based line a node starts on.
pub fn line(n: &N) -> usize {
    n.start_pos().line() + 1
}

/// Every node below `n` (itself included), breadth first as `ast.walk` visits them; named nodes
/// only.
pub fn walk<'r>(n: &N<'r>) -> Vec<N<'r>> {
    let mut out = Vec::new();
    let mut q = VecDeque::from([n.clone()]);
    while let Some(x) = q.pop_front() {
        for c in x.children() {
            if c.is_named() {
                q.push_back(c);
            }
        }
        out.push(x);
    }
    out
}

/// A statement of a body as Python sees it: a decorated definition is its definition, an
/// expression statement its expression.
pub fn stmt<'r>(n: N<'r>) -> N<'r> {
    match &*n.kind() {
        "decorated_definition" => n.field("definition").unwrap_or(n),
        "expression_statement" => {
            let named: Vec<N<'r>> = n.children().filter(|c| c.is_named()).collect();
            if named.len() == 1 {
                named.into_iter().next().unwrap_or(n)
            } else {
                n
            }
        }
        _ => n,
    }
}

/// The statements of a module or of a class or function body.
pub fn body<'r>(n: &N<'r>) -> Vec<N<'r>> {
    let block = if n.kind() == "module" {
        Some(n.clone())
    } else {
        n.field("body")
    };
    block
        .map(|b| {
            b.children()
                .filter(|c| c.is_named() && c.kind() != "comment")
                .map(stmt)
                .collect()
        })
        .unwrap_or_default()
}

pub fn is_def(n: &N) -> bool {
    n.kind() == "function_definition"
}

pub fn is_class(n: &N) -> bool {
    n.kind() == "class_definition"
}

pub fn is_async(n: &N) -> bool {
    n.children().any(|c| c.kind() == "async")
}

pub fn name_of(n: &N) -> String {
    n.field("name")
        .map(|x| x.text().into_owned())
        .unwrap_or_default()
}

/// The decorator expressions of a definition.
pub fn decorators<'r>(n: &N<'r>) -> Vec<N<'r>> {
    match n.parent() {
        Some(p) if p.kind() == "decorated_definition" => p
            .children()
            .filter(|c| c.kind() == "decorator")
            .filter_map(|d| d.children().find(|c| c.is_named() && c.kind() != "comment"))
            .collect(),
        _ => vec![],
    }
}

/// An assignment statement: its target expressions and value (None: an annotation alone);
/// `aug` for `+=` and the like.
pub struct Assign<'r> {
    pub targets: Vec<N<'r>>,
    pub value: Option<N<'r>>,
    pub aug: bool,
    pub annotated: bool,
}

pub fn assign<'r>(n: &N<'r>) -> Option<Assign<'r>> {
    match &*n.kind() {
        "assignment" => {
            let mut targets = vec![];
            let annotated = n.field("type").is_some();
            let mut cur = n.clone();
            loop {
                targets.extend(cur.field("left"));
                match cur.field("right") {
                    Some(r) if r.kind() == "assignment" => cur = r,
                    r => {
                        return Some(Assign {
                            targets,
                            value: r,
                            aug: false,
                            annotated,
                        });
                    }
                }
            }
        }
        "augmented_assignment" => Some(Assign {
            targets: n.field("left").into_iter().collect(),
            value: n.field("right"),
            aug: true,
            annotated: false,
        }),
        _ => None,
    }
}

/// An identifier's name.
pub fn ident(n: &N) -> Option<String> {
    (n.kind() == "identifier").then(|| n.text().into_owned())
}

/// `a.b.c` for a chain of names and attributes.
pub fn dotted(n: &N) -> Option<String> {
    match &*n.kind() {
        "identifier" => Some(n.text().into_owned()),
        "attribute" => {
            let obj = dotted(&n.field("object")?)?;
            Some(format!("{obj}.{}", n.field("attribute")?.text()))
        }
        _ => None,
    }
}

pub fn last_segment(n: &N) -> String {
    dotted(n)
        .map(|d| d.rsplit('.').next().unwrap_or_default().to_owned())
        .unwrap_or_default()
}

/// The last name of a name, an attribute, a subscript or a call (`last_name` of the helpers).
pub fn last_name(n: &N) -> String {
    match &*n.kind() {
        "identifier" => n.text().into_owned(),
        "attribute" => n
            .field("attribute")
            .map(|a| a.text().into_owned())
            .unwrap_or_default(),
        "subscript" => n.field("value").map(|v| last_name(&v)).unwrap_or_default(),
        "call" => n
            .field("function")
            .map(|v| last_name(&v))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// A call's positional arguments and keyword arguments (name, value).
pub struct Args<'r> {
    pub pos: Vec<N<'r>>,
    pub kw: Vec<(String, N<'r>)>,
}

pub fn args<'r>(call: &N<'r>) -> Args<'r> {
    let mut a = Args {
        pos: vec![],
        kw: vec![],
    };
    let Some(list) = call.field("arguments") else {
        return a;
    };
    if list.kind() == "generator_expression" {
        a.pos.push(list);
        return a;
    }
    for c in list.children().filter(|c| c.is_named()) {
        match &*c.kind() {
            "keyword_argument" => {
                if let (Some(k), Some(v)) = (c.field("name"), c.field("value")) {
                    a.kw.push((k.text().into_owned(), v));
                }
            }
            "comment" | "dictionary_splat" => {}
            _ => a.pos.push(c),
        }
    }
    a
}

/// The positional bases of a class.
pub fn bases<'r>(cls: &N<'r>) -> Vec<N<'r>> {
    cls.field("superclasses")
        .map(|s| {
            s.children()
                .filter(|c| {
                    c.is_named()
                        && !matches!(
                            &*c.kind(),
                            "keyword_argument" | "comment" | "dictionary_splat"
                        )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A string constant's value (not an f-string, not bytes); implicit concatenation included.
pub fn string(n: &N) -> Option<String> {
    match &*n.kind() {
        "string" => {
            let start = n.children().find(|c| c.kind() == "string_start")?;
            let prefix = start.text().to_lowercase();
            if prefix.contains('f') || prefix.contains('b') || prefix.contains('t') {
                return None;
            }
            let raw = prefix.contains('r');
            let mut s = String::new();
            for c in n.children() {
                match &*c.kind() {
                    "string_content" => s.push_str(&unescape(&c.text(), raw)),
                    "escape_sequence" => s.push_str(&unescape(&c.text(), raw)),
                    "interpolation" => return None,
                    _ => {}
                }
            }
            Some(s)
        }
        "concatenated_string" => {
            let mut s = String::new();
            for c in n.children().filter(|c| c.is_named()) {
                s.push_str(&string(&c)?);
            }
            Some(s)
        }
        "parenthesized_expression" => {
            let mut named = n.children().filter(|c| c.is_named());
            match (named.next(), named.next()) {
                (Some(only), None) => string(&only),
                _ => None,
            }
        }
        _ => None,
    }
}

fn unescape(s: &str, raw: bool) -> String {
    if raw || !s.contains('\\') {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\n') => {}
            Some(x @ ('\\' | '\'' | '"')) => out.push(x),
            Some(x) => {
                out.push('\\');
                out.push(x);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The items of a list, tuple or set display (`a, b` without brackets is a tuple).
pub fn elements<'r>(n: &N<'r>) -> Option<Vec<N<'r>>> {
    match &*n.kind() {
        "list" | "tuple" | "set" | "expression_list" => Some(
            n.children()
                .filter(|c| c.is_named() && c.kind() != "comment")
                .collect(),
        ),
        "parenthesized_expression" => {
            let mut named = n
                .children()
                .filter(|c| c.is_named() && c.kind() != "comment");
            match (named.next(), named.next()) {
                (Some(only), None) => elements(&only),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The string items of a list, tuple or set display, or None when one is not a string.
pub fn strings(n: &N) -> Option<Vec<String>> {
    elements(n)?.iter().map(string).collect()
}

/// `x is True` for a keyword argument value.
pub fn is_true(n: &N) -> bool {
    n.kind() == "true"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_python() {
        let root =
            parse("import x\n@dec\ndef f(a=1):\n    pass\n__all__ = ['a', \"b\"]\n").unwrap();
        let top = root.root();
        let b = body(&top);
        assert_eq!(b.len(), 3);
        assert!(is_def(&b[1]));
        assert_eq!(decorators(&b[1])[0].text(), "dec");
        let a = assign(&b[2]).unwrap();
        assert_eq!(ident(&a.targets[0]).as_deref(), Some("__all__"));
        assert_eq!(strings(&a.value.unwrap()).unwrap(), ["a", "b"]);
        assert!(parse("def (:\n").is_none());
    }
}
