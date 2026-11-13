//! Reading TypeScript declarations as text (the port of `types_typescript.mjs`'s scanners): the
//! name declared at a position, the names a type is made of at its top level, heritage clauses.
//! Offsets are bytes; identifiers are ASCII (`[A-Za-z_$][\w$]*`).

use std::collections::HashSet;

use crate::text::{is_word, is_word_start, word_at_index};

const KEYWORDS: &[&str] = &[
    "export",
    "declare",
    "default",
    "abstract",
    "async",
    "class",
    "interface",
    "type",
    "enum",
    "const",
    "let",
    "var",
    "function",
    "namespace",
    "module",
];

// words of a type that name no declaration
const TYPE_WORDS: &[&str] = &[
    "string",
    "number",
    "boolean",
    "bigint",
    "symbol",
    "object",
    "unknown",
    "any",
    "never",
    "void",
    "null",
    "undefined",
    "true",
    "false",
    "this",
    "readonly",
    "unique",
    "asserts",
    "is",
    "infer",
    "new",
    "abstract",
];

fn at(text: &str, i: usize) -> u8 {
    text.as_bytes().get(i).copied().unwrap_or(0)
}

fn is_quote(c: u8) -> bool {
    c == b'"' || c == b'\'' || c == b'`'
}

/// The offset of the first byte at or after `i` that is not whitespace or a comment.
pub fn skip_trivia(text: &str, mut i: usize) -> usize {
    let b = text.as_bytes();
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b[i.min(b.len())..].starts_with(b"//") {
            i = text[i..].find('\n').map_or(b.len(), |n| i + n + 1);
        } else if b[i.min(b.len())..].starts_with(b"/*") {
            i = text[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 2);
        } else {
            return i;
        }
    }
}

/// The offset just past the string literal starting at `i` (its quote).
pub fn skip_string(text: &str, i: usize) -> usize {
    let b = text.as_bytes();
    let q = b[i];
    let mut j = i + 1;
    while j < b.len() {
        if b[j] == b'\\' {
            j += 1;
        } else if b[j] == q {
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

/// The offset just past the group (`<>`, `()`, `[]`, `{}`) opening at `i`; `=>` is not a `>`.
pub fn skip_group(text: &str, i: usize, to: usize) -> usize {
    let mut depth = 0i32;
    let mut j = i;
    while j < to {
        let k = skip_trivia(text, j);
        if k != j {
            j = k;
            continue;
        }
        let c = at(text, j);
        if is_quote(c) {
            j = skip_string(text, j);
            continue;
        }
        if c == b'=' && at(text, j + 1) == b'>' {
            j += 2;
            continue;
        }
        if b"<([{".contains(&c) {
            depth += 1;
        } else if b">)]}".contains(&c) {
            depth -= 1;
            if depth == 0 {
                return j + 1;
            }
        }
        j += 1;
    }
    to
}

/// Each `type <Name>` head before `index`: (start of the match, its end, the name).
fn type_heads(text: &str, index: usize) -> Vec<(usize, usize, &str)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(n) = text[from..index.min(text.len())].find("type") {
        let s = from + n;
        from = s + 4;
        if s > 0 && is_word(b[s - 1], false) {
            continue;
        }
        let mut i = s + 4;
        let ws = i;
        while i < index && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == ws || i >= index {
            continue;
        }
        if let Some(w) = word_at_index(&text[..index], i, true) {
            out.push((s, i + w.len(), w));
        }
    }
    out
}

/// The name of the type alias whose body holds `index`, the start of an anonymous type.
pub fn enclosing_alias(text: &str, index: usize) -> Option<String> {
    for &(_, head_end, name) in type_heads(text, index).iter().rev() {
        let mut i = skip_trivia(text, head_end);
        if at(text, i) == b'<' {
            let mut depth = 0;
            while i < index {
                let c = at(text, i);
                if c == b'=' && at(text, i + 1) == b'>' {
                    i += 1;
                } else if c == b'<' {
                    depth += 1;
                } else if c == b'>' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            i = skip_trivia(text, i + 1);
        }
        if at(text, i) != b'=' || at(text, i + 1) == b'>' || i >= index {
            continue;
        }
        let mut depth = 0i32;
        let mut ended = false;
        let mut j = i + 1;
        while j < index && !ended {
            let k = skip_trivia(text, j);
            if k != j {
                j = k;
                continue;
            }
            let c = at(text, j);
            if is_quote(c) {
                j = skip_string(text, j);
            } else {
                if b"([{".contains(&c) {
                    depth += 1;
                } else if b")]}".contains(&c) {
                    depth -= 1;
                    ended = depth < 0;
                } else if c == b';' && depth == 0 {
                    ended = true;
                }
                j += 1;
            }
        }
        if !ended {
            return Some(name.to_owned());
        }
    }
    None
}

/// The declared name at an offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameAt {
    pub name: String,
    /// Where a class or interface header's heritage (or an alias's type parameters) starts; none
    /// for an anonymous type named by its alias.
    pub end: Option<usize>,
    pub alias: bool,
}

fn preceded_by_type(text: &str, i: usize) -> bool {
    let s = &text[crate::text::floor_char(text, i.saturating_sub(64))..i];
    let t = s.trim_end_matches(|c: char| c.is_ascii_whitespace());
    t.len() < s.len()
        && t.ends_with("type")
        && t.as_bytes()
            .get(t.len().wrapping_sub(5))
            .is_none_or(|&c| !is_word(c, false))
}

/// The declared name at `index` (keywords and comments skipped).
pub fn name_at(text: &str, index: usize) -> Option<NameAt> {
    let mut alias = false;
    let mut i = skip_trivia(text, index);
    while i < text.len() {
        let Some(m) = word_at_index(text, i, true) else {
            return enclosing_alias(text, i).map(|name| NameAt {
                name,
                end: None,
                alias: false,
            });
        };
        if !KEYWORDS.contains(&m) {
            return Some(NameAt {
                name: m.to_owned(),
                end: Some(i + m.len()),
                alias: alias || preceded_by_type(text, i),
            });
        }
        alias = m == "type";
        i = skip_trivia(text, i + m.len());
    }
    None
}

/// The offsets of the names a type expression (`from`..`to`) is made of at its top level.
pub fn top_level_names(text: &str, from: usize, to: usize, skip: &HashSet<String>) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = from;
    let mut operand = false;
    while i < to {
        i = skip_trivia(text, i);
        if i >= to {
            break;
        }
        let c = at(text, i);
        if is_quote(c) {
            i = skip_string(text, i);
            continue;
        }
        if c == b'?' {
            return Vec::new(); // a conditional type
        }
        if c == b'=' && at(text, i + 1) == b'>' {
            break; // a function type's result
        }
        if c == b'(' {
            let end = skip_group(text, i, to);
            if text[skip_trivia(text, end).min(text.len())..].starts_with("=>") {
                break; // a function type
            }
            i += 1;
            continue;
        }
        if c == b')' {
            i += 1;
            continue;
        }
        if b"<[{".contains(&c) {
            i = skip_group(text, i, to);
            continue;
        }
        let Some(m) = word_at_index(text, i, true) else {
            i += 1;
            continue;
        };
        if m == "extends" {
            return Vec::new();
        }
        let mut last: Option<usize> = Some(i);
        let mut j = i + m.len();
        if m == "import" && at(text, skip_trivia(text, j)) == b'(' {
            j = skip_group(text, skip_trivia(text, j), to);
            last = None;
        }
        loop {
            let k0 = skip_trivia(text, j);
            if at(text, k0) != b'.' {
                break;
            }
            let k = skip_trivia(text, k0 + 1);
            if !is_word_start(at(text, k), true) {
                break;
            }
            let n = word_at_index(text, k, true).unwrap_or("");
            last = Some(k);
            j = k + n.len();
        }
        if m == "typeof" || m == "keyof" {
            operand = true;
            i = j;
            continue;
        }
        let mut after = skip_trivia(text, j);
        if at(text, after) == b'<' {
            after = skip_trivia(text, skip_group(text, after, to));
        }
        let indexed = at(text, after) == b'[';
        let qualified = last != Some(i);
        if let Some(l) = last {
            if !operand
                && !indexed
                && (qualified || (!TYPE_WORDS.contains(&m) && !skip.contains(m)))
            {
                out.push(l);
            }
        }
        operand = false;
        i = after;
    }
    out
}

/// The offsets of the names a type alias's body is made of at its top level, from the end of its
/// name, past its type parameters, to the end of its statement.
pub fn alias_members(text: &str, from: usize) -> Vec<usize> {
    let len = text.len();
    let mut i = skip_trivia(text, from);
    let mut params = HashSet::new();
    if at(text, i) == b'<' {
        let end = skip_group(text, i, len);
        let mut j = skip_trivia(text, i + 1);
        while j + 1 < end {
            if let Some(m) = word_at_index(text, j, true) {
                params.insert(m.to_owned());
            }
            while j + 1 < end && at(text, j) != b',' {
                let c = at(text, j);
                j = if b"<([{".contains(&c) {
                    skip_group(text, j, end)
                } else if c == b'"' || c == b'\'' {
                    skip_string(text, j)
                } else {
                    j + 1
                };
            }
            j = skip_trivia(text, j + 1);
        }
        i = skip_trivia(text, end);
    }
    if at(text, i) != b'=' || at(text, i + 1) == b'>' {
        return Vec::new();
    }
    let mut to = len;
    let mut j = i + 1;
    while j < len {
        let k = skip_trivia(text, j);
        if k != j {
            let gap = &text[j..k];
            let before = text[i + 1..j].trim_end();
            let continues = before
                .as_bytes()
                .last()
                .is_some_and(|c| b"&|=,<(".contains(c));
            if gap.contains('\n')
                && !before.is_empty()
                && !continues
                && !text[k..].starts_with(['&', '|'])
            {
                to = j;
                break;
            }
            j = k;
            continue;
        }
        let c = at(text, j);
        if is_quote(c) {
            j = skip_string(text, j);
        } else if b"<([{".contains(&c) {
            j = skip_group(text, j, len);
        } else if c == b';' || b")]}".contains(&c) {
            to = j;
            break;
        } else {
            j += 1;
        }
    }
    top_level_names(text, i + 1, to, &params)
}

/// The offsets of the names a class or interface header extends or implements.
pub fn heritage(text: &str, from: usize) -> Vec<usize> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut list = false;
    let mut i = from;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && at(text, i + 1) == b'/' {
            match text[i..].find('\n') {
                Some(n) => i += n,
                None => break,
            }
            continue;
        }
        if c == b'/' && at(text, i + 1) == b'*' {
            i = text[i..].find("*/").map_or(b.len(), |n| i + n + 2);
            continue;
        }
        if c == b'=' && at(text, i + 1) == b'>' {
            i += 2;
            continue;
        }
        if b"<([{".contains(&c) {
            if c == b'{' && depth == 0 {
                break;
            }
            depth += 1;
            i += 1;
            continue;
        }
        if b">)]}".contains(&c) {
            depth -= 1;
            i += 1;
            continue;
        }
        if (c == b';' || c == b'=') && depth == 0 {
            break;
        }
        let Some(m) = word_at_index(text, i, true) else {
            i += 1;
            continue;
        };
        if depth == 0 && (m == "extends" || m == "implements") {
            list = true;
        } else if depth == 0 && list {
            let mut last = i;
            let mut j = i + m.len();
            while at(text, j) == b'.' && is_word_start(at(text, j + 1), true) {
                let n = word_at_index(text, j + 1, true).unwrap_or("");
                last = j + 1;
                j = j + 1 + n.len();
            }
            out.push(last);
            i = j;
            continue;
        }
        i += m.len();
    }
    out
}

fn rfind_from(text: &str, needle: &str, max_start: usize) -> Option<usize> {
    let end = (max_start + needle.len()).min(text.len());
    text.as_bytes()[..end]
        .windows(needle.len())
        .rposition(|w| w == needle.as_bytes())
}

/// Whether `index` is in the clause of an import declaration.
pub fn in_import(text: &str, index: usize) -> bool {
    let mut from = rfind_from(text, "import", index);
    while let Some(i) = from {
        let before_ok = i == 0 || !is_word(text.as_bytes()[i - 1], true);
        let after_ws = at(text, i + 6).is_ascii_whitespace();
        if !before_ok || !after_ws {
            from = if i == 0 {
                None
            } else {
                rfind_from(text, "import", i - 1)
            };
            continue;
        }
        let clause = if index > i + 6 {
            &text.as_bytes()[i + 6..index.min(text.len())]
        } else {
            &[][..]
        };
        if clause.iter().any(|c| b";'\"`()=".contains(c)) {
            return false;
        }
        let s = &text.as_bytes()[i.saturating_sub(200)..i];
        let mut t = s.len();
        while t > 0 && s[t - 1].is_ascii_whitespace() {
            t -= 1;
        }
        return t == 0 || b";}".contains(&s[t - 1]) || s[t..].contains(&b'\n');
    }
    false
}

/// Whether a name (`start`..`end`) is in the position of an object literal's key.
pub fn key_position(text: &str, start: usize, end: usize) -> bool {
    let b = text.as_bytes();
    let mut i = start as isize - 1;
    loop {
        while i >= 0 && b[i as usize].is_ascii_whitespace() {
            i -= 1;
        }
        if i >= 1 && b[i as usize] == b'/' && b[i as usize - 1] == b'*' {
            if i < 2 {
                return false;
            }
            match rfind_from(text, "/*", i as usize - 2) {
                Some(open) => {
                    i = open as isize - 1;
                    continue;
                }
                None => return false,
            }
        }
        break;
    }
    if i < 0 || (b[i as usize] != b'{' && b[i as usize] != b',') {
        return false;
    }
    let after = skip_trivia(text, end);
    match at(text, after) {
        b':' => at(text, after + 1) != b':',
        b',' | b'}' => true,
        _ => false,
    }
}

/// The text with the expressions of `ranges` for which `active` holds each bound to a fresh
/// parameter, `((__forge_t<n> = (<expr>)) => __forge_t<n>)()`.
pub struct Bound {
    pub overlay: String,
    /// The inserts: (offset in the text, piece).
    pub events: Vec<(usize, String)>,
    /// n → (start, end) of the parameter's name in the overlay.
    pub anchors: Vec<Option<(usize, usize)>>,
    /// n → where the arrow uses it.
    pub refs: Vec<Option<usize>>,
}

pub fn bindings(text: &str, ranges: &[(usize, usize)], active: impl Fn(usize) -> bool) -> Bound {
    let mut ev: Vec<(usize, u8, i64, usize, String)> = Vec::new();
    for (n, &(s, e)) in ranges.iter().enumerate() {
        if !active(n) {
            continue;
        }
        let size = i64::try_from(e - s).unwrap_or(i64::MAX);
        ev.push((s, 1, -size, n, format!("((__forge_t{n} = (")));
        ev.push((e, 0, size, n, format!(")) => __forge_t{n})()")));
    }
    ev.sort();
    let mut overlay = String::with_capacity(text.len() + ev.len() * 24);
    let mut last = 0;
    let mut anchors = vec![None; ranges.len()];
    let mut refs = vec![None; ranges.len()];
    for (off, kind, _, n, piece) in &ev {
        overlay.push_str(&text[last..*off]);
        last = *off;
        if *kind == 1 {
            let s = overlay.len() + 2;
            anchors[*n] = Some((s, s + format!("__forge_t{n}").len()));
        } else {
            refs[*n] = Some(overlay.len() + 6);
        }
        overlay.push_str(piece);
    }
    overlay.push_str(&text[last..]);
    Bound {
        overlay,
        events: ev.into_iter().map(|(o, _, _, _, p)| (o, p)).collect(),
        anchors,
        refs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str, idx: Vec<usize>) -> Vec<&str> {
        idx.into_iter()
            .map(|i| word_at_index(text, i, true).unwrap())
            .collect()
    }

    #[test]
    fn alias_members_of_intersections() {
        let t =
            "export type RedisClientType<M = {}> = (RedisClient<M> & RedisClientExtensions<M>);\n";
        let n = name_at(t, t.find("RedisClientType").unwrap()).unwrap();
        assert!(n.alias);
        assert_eq!(
            names(t, alias_members(t, n.end.unwrap())),
            ["RedisClient", "RedisClientExtensions"]
        );
        let u = "type UseBoundStore<S> = { (): S } & S;\n";
        let n = name_at(u, u.find("UseBoundStore").unwrap()).unwrap();
        assert!(alias_members(u, n.end.unwrap()).is_empty());
    }

    #[test]
    fn top_level_names_of_printed_types() {
        let t = ": NodePgDatabase<Record<string, never>> & { $client: Pool; }";
        assert_eq!(
            names(t, top_level_names(t, 1, t.len(), &HashSet::new())),
            ["NodePgDatabase"]
        );
        let t = ": Prisma.PrismaPromise<T> | string[] | typeof x";
        assert_eq!(
            names(t, top_level_names(t, 1, t.len(), &HashSet::new())),
            ["PrismaPromise"]
        );
    }

    #[test]
    fn heritage_and_anonymous_types() {
        let t =
            "export interface Prisma__UserClient<T> extends Prisma.PrismaPromise<T>, Other {\n}";
        let n = name_at(t, t.find("Prisma__").unwrap()).unwrap();
        assert_eq!(
            names(t, heritage(t, n.end.unwrap())),
            ["PrismaPromise", "Other"]
        );
        let t = "export type UseFormWatch<T> = {\n  (): T;\n};";
        let n = name_at(t, t.find('{').unwrap()).unwrap();
        assert_eq!(n.name, "UseFormWatch");
        assert_eq!(n.end, None);
    }

    #[test]
    fn imports_and_keys() {
        let t = "import { a, b as c } from \"m\";\nconst x = { a: 1, b };\n";
        assert!(in_import(t, t.find("c }").unwrap()));
        assert!(!in_import(t, t.find("x =").unwrap()));
        let a = t.rfind("a: 1").unwrap();
        assert!(key_position(t, a, a + 1));
        let b = t.rfind("b }").unwrap();
        assert!(key_position(t, b, b + 1));
        assert!(!key_position(
            t,
            t.find("x =").unwrap(),
            t.find("x =").unwrap() + 1
        ));
    }

    #[test]
    fn binds_as_arrow_parameters() {
        let t = "f(a.b);";
        let b = bindings(t, &[(2, 5)], |_| true);
        assert_eq!(b.overlay, "f(((__forge_t0 = (a.b)) => __forge_t0)());");
        let (s, e) = b.anchors[0].unwrap();
        assert_eq!(&b.overlay[s..e], "__forge_t0");
        assert_eq!(
            &b.overlay[b.refs[0].unwrap()..b.refs[0].unwrap() + 10],
            "__forge_t0"
        );
    }
}
