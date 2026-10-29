//! Public names of a library are not removed without a deprecation (`public_api.py`,
//! `public_api.mjs removed`).
//!
//! Python: for each changed module whose base copy has a literal `__all__`, a name that left
//! `__all__` (or whose module was deleted) must still be importable with a warning, read from
//! the syntax tree: a def/class of that name decorated with `@deprecated` or whose body calls
//! `warnings.warn(..., DeprecationWarning | FutureWarning | PendingDeprecationWarning or a
//! subclass)`; an alias wrapped in `deprecated(...)(...)`; or a module `__getattr__` that names
//! it and warns that way. A project function that warns that way counts when the shim calls it
//! or decorates with it. A word in a docstring or a comment does not count.
//!
//! npm: names exported by the package's entry points (package.json exports, main, module,
//! types) in the base are still exported; a name that became an alias of another export carries
//! a `/** @deprecated */` JSDoc comment.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};
use serde_json::Value;

use crate::pyast::{
    self, N, Root, args, assign, bases, body, decorators, dotted, ident, is_class, is_def,
    name_of, string, strings,
};
use crate::util::{Change, Lang, at, regex_static, strip_script};

pub struct PublicApi;

impl Builtin for PublicApi {
    fn name(&self) -> &'static str {
        "public_api"
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        let (lang, rest) = strip_script(args);
        let ch = Change::new(ctx);
        let mut r = Report::default();
        if lang != Some(Lang::Js) {
            python(&ch, &ctx.rule, &mut r);
        }
        if lang != Some(Lang::Python) && rest.first().is_none_or(|m| m == "removed") {
            npm(&ch, &ctx.rule, &mut r);
        }
        r
    }
}

const MAJOR: &str = "if the removal is intended for a major version, ask the user to accept this change";

// ---------------------------------------------------------------- Python

const CATEGORIES: &[&str] = &[
    "DeprecationWarning",
    "FutureWarning",
    "PendingDeprecationWarning",
];

fn skip_py() -> &'static regex::Regex {
    regex_static(r"(^|/)(tests?/|test_[^/]*$|[^/]*_test\.py$|conftest\.py$|migrations/)")
}

/// The literal names of `__all__` (assignments, `+=`, `.extend`, `.append`), or None.
fn dunder_all(tree: &N) -> Option<BTreeSet<String>> {
    let mut names: Vec<String> = vec![];
    let mut seen = false;
    for st in body(tree) {
        if let Some(a) = assign(&st) {
            if a.targets.iter().any(|t| ident(t).as_deref() == Some("__all__")) {
                let vals = strings(a.value.as_ref()?)?;
                if !a.aug {
                    names.clear();
                }
                names.extend(vals);
                seen = true;
            }
        } else if st.kind() == "call"
            && let Some(f) = st.field("function")
            && f.kind() == "attribute"
            && f.field("object").and_then(|o| ident(&o)).as_deref() == Some("__all__")
        {
            let a = args(&st).pos;
            let Some(arg) = a.first() else { continue };
            let attr = f.field("attribute").map(|x| x.text().into_owned());
            match attr.as_deref() {
                Some("append") => names.extend(string(arg)),
                Some("extend") => names.extend(strings(arg).unwrap_or_default()),
                _ => {}
            }
        }
    }
    seen.then(|| names.into_iter().collect())
}

fn last_of(n: &N) -> String {
    dotted(n)
        .map(|d| d.rsplit('.').next().unwrap_or_default().to_owned())
        .unwrap_or_default()
}

/// What warns in the project: deprecation warning classes, functions that warn.
struct Index {
    categories: HashSet<String>,
    warners: HashSet<String>,
}

impl Index {
    fn new(trees: &[Root]) -> Self {
        let mut ix = Index {
            categories: CATEGORIES.iter().map(|s| (*s).to_owned()).collect(),
            warners: HashSet::new(),
        };
        let all: Vec<N> = trees.iter().flat_map(|t| t.root().dfs().collect::<Vec<_>>()).collect();
        let classes: Vec<&N> = all.iter().filter(|n| is_class(n)).collect();
        let defs: Vec<&N> = all.iter().filter(|n| is_def(n)).collect();
        loop {
            let mut changed = false;
            for c in &classes {
                let name = name_of(c);
                if !ix.categories.contains(&name)
                    && bases(c).iter().any(|b| ix.categories.contains(&last_of(b)))
                {
                    ix.categories.insert(name);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        loop {
            let mut changed = false;
            for d in &defs {
                let name = name_of(d);
                if !ix.warners.contains(&name) && ix.warns(d) {
                    ix.warners.insert(name);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        ix
    }

    fn category(&self, node: &N, func: Option<&N>) -> bool {
        if self.categories.contains(&last_of(node)) {
            return true;
        }
        // a parameter of the enclosing function whose default is a deprecation category
        if let (Some(func), Some(id)) = (func, ident(node))
            && let Some(params) = func.field("parameters")
        {
            for p in params.children() {
                if matches!(&*p.kind(), "default_parameter" | "typed_default_parameter")
                    && p.field("name").map(|n| n.text().into_owned()).as_deref() == Some(&id)
                    && p.field("value").is_some_and(|d| self.categories.contains(&last_of(&d)))
                {
                    return true;
                }
            }
        }
        false
    }

    fn is_deprecated_decorator(&self, node: &N) -> bool {
        let target = if node.kind() == "call" {
            node.field("function")
        } else {
            Some(node.clone())
        };
        let name = target.and_then(|t| dotted(&t)).unwrap_or_default();
        if matches!(
            name.as_str(),
            "deprecated" | "typing_extensions.deprecated" | "warnings.deprecated" | "deprecation.deprecated"
        ) {
            return true;
        }
        self.warners.contains(name.rsplit('.').next().unwrap_or_default())
    }

    fn warn_call(&self, node: &N, func: Option<&N>) -> bool {
        if node.kind() != "call" {
            return false;
        }
        let Some(f) = node.field("function") else {
            return false;
        };
        let name = dotted(&f).unwrap_or_default();
        if matches!(
            name.as_str(),
            "warnings.warn" | "warn" | "warnings.warn_explicit" | "warn_explicit"
        ) {
            let a = args(node);
            let mut cat = a.pos.get(1).cloned();
            for (k, v) in &a.kw {
                if k == "category" {
                    cat = Some(v.clone());
                }
            }
            // warn(DeprecationWarning("...")): the category is the message's class
            if cat.is_none()
                && let Some(msg) = a.pos.first()
                && msg.kind() == "call"
                && msg.field("function").is_some_and(|mf| self.category(&mf, func))
            {
                return true;
            }
            return cat.is_some_and(|c| self.category(&c, func));
        }
        if self.warners.contains(name.rsplit('.').next().unwrap_or_default()) {
            return true;
        }
        // deprecated("...")(target): an alias wrapped by the decorator
        f.kind() == "call" && self.is_deprecated_decorator(&f)
    }

    /// The node (a def, a class, an expression) warns with a deprecation category.
    fn warns(&self, node: &N) -> bool {
        let func = is_def(node).then_some(node);
        if (is_def(node) || is_class(node))
            && decorators(node).iter().any(|d| self.is_deprecated_decorator(d))
        {
            return true;
        }
        if is_class(node) {
            // a class warns when it is created or subclassed
            const HOOKS: &[&str] = &["__init__", "__new__", "__init_subclass__", "__post_init__"];
            return body(node)
                .iter()
                .any(|m| is_def(m) && HOOKS.contains(&name_of(m).as_str()) && self.warns(m));
        }
        let start = node.node_id();
        for sub in node.dfs() {
            if !sub.is_named() {
                continue;
            }
            let inner = if is_def(&sub) { Some(&sub) } else { func };
            if (is_def(&sub) || is_class(&sub))
                && sub.node_id() != start
                && decorators(&sub).iter().any(|d| self.is_deprecated_decorator(d))
            {
                return true;
            }
            if self.warn_call(&sub, inner) {
                return true;
            }
        }
        false
    }
}

fn names_string(node: &N, name: &str) -> bool {
    node.dfs()
        .any(|n| n.kind() == "string" && string(&n).as_deref() == Some(name))
}

fn shimmed(ix: &Index, tree: &N, name: &str) -> bool {
    let top = body(tree);
    for st in &top {
        if (is_def(st) || is_class(st)) && name_of(st) == name && ix.warns(st) {
            return true;
        }
        if let Some(a) = assign(st)
            && !a.aug
            && a.targets.iter().any(|t| ident(t).as_deref() == Some(name))
            && let Some(v) = &a.value
            && ix.warns(v)
        {
            return true;
        }
    }
    for st in &top {
        if is_def(st) && !pyast::is_async(st) && name_of(st) == "__getattr__" && ix.warns(st) {
            // the name is in __getattr__ itself, or in a module-level table it reads
            if names_string(st, name) {
                return true;
            }
            let read: HashSet<String> = st.dfs().filter_map(|n| ident(&n)).collect();
            for other in &top {
                if let Some(a) = assign(other)
                    && !a.aug
                    && a.targets
                        .iter()
                        .any(|t| ident(t).is_some_and(|i| read.contains(&i)))
                    && a.value.as_ref().is_some_and(|v| names_string(v, name))
                {
                    return true;
                }
            }
        }
    }
    false
}

fn python(ch: &Change, rule: &str, r: &mut Report) {
    let mut index: Option<Index> = None;
    let mut project: Option<Vec<Root>> = None;
    for path in ch.changed {
        if !path.ends_with(".py") || skip_py().is_match(path) || !ch.in_base(path) {
            continue;
        }
        let Some(base_tree) = ch.base_copy(path).and_then(|t| pyast::parse(&t)) else {
            continue;
        };
        let Some(base_all) = dunder_all(&base_tree.root()).filter(|a| !a.is_empty()) else {
            continue;
        };
        let Some(text) = ch.now(path) else {
            let gone: Vec<&str> = base_all
                .iter()
                .filter(|n| !n.starts_with('_'))
                .map(String::as_str)
                .collect();
            if !gone.is_empty() {
                r.findings.push(at(rule, path, 0, format!(
                    "the module was deleted, with public names {} in its __all__: keep it, re-exporting them with a DeprecationWarning; {MAJOR}",
                    gone.join(", ")
                )));
            }
            continue;
        };
        let Some(tree) = pyast::parse(&text) else { continue };
        let Some(now_all) = dunder_all(&tree.root()) else {
            continue;
        };
        for name in base_all.difference(&now_all) {
            let ix = index.get_or_insert_with(|| {
                let trees = project.get_or_insert_with(|| {
                    ch.ls_files(&["*.py"])
                        .unwrap_or_default()
                        .iter()
                        .take(5000)
                        .filter_map(|p| pyast::parse(&ch.now(p)?))
                        .collect()
                });
                Index::new(trees)
            });
            if !shimmed(ix, &tree.root(), name) {
                r.findings.push(at(rule, path, 0, format!(
                    "{name} left __all__ without a deprecation: keep it importable with warnings.warn(..., DeprecationWarning, stacklevel=2) (a module __getattr__, or @typing_extensions.deprecated on a wrapper); {MAJOR}"
                )));
            }
        }
    }
}

// ---------------------------------------------------------------- npm

const SOURCE: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".jsx"];

fn skip_js() -> &'static regex::Regex {
    regex_static(r"(^|/)(__tests__|e2e)/|\.(test|spec)\.[cm]?[jt]sx?$")
}

/// `path.normalize` for a relative path: `.` and `..` resolved, `./` dropped.
fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = vec![];
    let abs = p.starts_with('/');
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !abs {
                    out.push("..");
                }
            }
            x => out.push(x),
        }
    }
    let s = out.join("/");
    if abs { format!("/{s}") } else if s.is_empty() { ".".into() } else { s }
}

struct Js<'a> {
    ch: &'a Change<'a>,
    has_base: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum When {
    Now,
    Base,
}

#[derive(Debug, Clone)]
struct Export {
    file: String,
    line: usize,
    at: Option<usize>,
    spec: Option<usize>,
    of: Option<String>,
}

impl Js<'_> {
    fn exists(&self, p: &str) -> bool {
        self.ch.exists(p)
    }

    fn read(&self, p: &str, when: When) -> Option<String> {
        match when {
            When::Now => self.ch.now(p),
            When::Base => {
                if !self.ch.is_changed(p) {
                    return self.ch.now(p);
                }
                if !self.ch.in_base(p) || !self.has_base {
                    return None;
                }
                self.ch.base_copy(p)
            }
        }
    }

    /// Source files of the package's entry points.
    fn entries(&self, pkg: &Value) -> Vec<String> {
        let mut targets: Vec<String> = vec![];
        fn walk(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::String(s) => out.push(s.clone()),
                Value::Object(m) => {
                    for (k, x) in m {
                        if k != "./package.json" {
                            walk(x, out);
                        }
                    }
                }
                Value::Array(a) => {
                    for x in a {
                        walk(x, out);
                    }
                }
                _ => {}
            }
        }
        if let Some(e) = pkg.get("exports") {
            walk(e, &mut targets);
        }
        for k in ["source", "main", "module", "types", "typings"] {
            if let Some(e) = pkg.get(k) {
                walk(e, &mut targets);
            }
        }
        let mut out: Vec<String> = vec![];
        let ext_re = regex_static(r"\.d\.[cm]?ts$|\.[cm]?[jt]sx?$");
        let build_re = regex_static(r"^(dist|lib|build|out|esm|cjs|es|types)/");
        for t in targets {
            if t.contains('*') {
                continue;
            }
            let p = normalize(&t);
            let p = p.strip_prefix("./").unwrap_or(&p).to_owned();
            let stem = ext_re.replace(&p, "").into_owned();
            let mut stems: Vec<String> = vec![];
            for s in [
                stem.clone(),
                build_re.replace(&stem, "src/").into_owned(),
                build_re.replace(&stem, "").into_owned(),
            ] {
                if !stems.contains(&s) {
                    stems.push(s);
                }
            }
            for s in stems {
                for ext in SOURCE {
                    let c = format!("{s}{ext}");
                    if self.exists(&c)
                        && !regex_static(r"\.d\.[cm]?ts$").is_match(&c)
                        && !out.contains(&c)
                    {
                        out.push(c);
                    }
                }
            }
        }
        out
    }

    fn resolve(&self, from: &str, spec: &str) -> Option<String> {
        if !spec.starts_with('.') {
            return None;
        }
        let dir = Path::new(from).parent().and_then(|d| d.to_str()).unwrap_or("");
        let joined = if dir.is_empty() {
            spec.to_owned()
        } else {
            format!("{dir}/{spec}")
        };
        let base = regex_static(r"\.[cm]?jsx?$")
            .replace(&normalize(&joined), "")
            .into_owned();
        let cands = SOURCE
            .iter()
            .map(|e| format!("{base}{e}"))
            .chain(SOURCE.iter().map(|e| normalize(&format!("{base}/index{e}"))));
        cands.into_iter().find(|c| self.exists(c) || self.ch.is_changed(c))
    }

    /// The exports of a module (`export *` followed through relative modules).
    fn exports_of(
        &self,
        file: &str,
        when: When,
        seen: &mut HashSet<String>,
    ) -> Vec<(String, Export)> {
        let mut out: Vec<(String, Export)> = vec![];
        if !seen.insert(file.to_owned()) {
            return out;
        }
        let Some(raw) = self.read(file, when) else {
            return out;
        };
        let text = strip_comments(&raw);
        let has = |out: &Vec<(String, Export)>, n: &str| out.iter().any(|(k, _)| k == n);
        let decl = regex_static(
            r"\bexport\s+(?:default\s+)?(?:declare\s+)?(?:abstract\s+)?(?:async\s+)?(?:function\*?|class|const|let|var|enum|interface|type|namespace)\s+([A-Za-z_$][\w$]*)",
        );
        for m in decl.captures_iter(&text) {
            let whole = m.get(0).expect("match");
            let name = m[1].to_owned();
            if has(&out, &name) {
                continue;
            }
            let rest = text[whole.end()..].split('\n').next().unwrap_or("");
            let of = if regex_static(r"\b(const|let|var)\s+[\w$]+$").is_match(whole.as_str()) {
                regex_static(r"^\s*(?::[^=]+)?=\s*([A-Za-z_$][\w$]*)\s*;?\s*$")
                    .captures(rest)
                    .map(|c| c[1].to_owned())
            } else {
                None
            };
            out.push((
                name,
                Export {
                    file: file.to_owned(),
                    line: line_at(&text, whole.start()),
                    at: Some(whole.start()),
                    spec: None,
                    of,
                },
            ));
        }
        for m in regex_static(r"\bexport\s+default\b").find_iter(&text) {
            if !has(&out, "default") {
                out.push((
                    "default".into(),
                    Export {
                        file: file.to_owned(),
                        line: line_at(&text, m.start()),
                        at: None,
                        spec: None,
                        of: None,
                    },
                ));
            }
        }
        let braces =
            regex_static(r#"\bexport\s+(?:type\s+)?\{([^}]*)\}(?:\s*from\s*["']([^"']+)["'])?"#);
        for m in braces.captures_iter(&text) {
            let whole = m.get(0).expect("match");
            let mut off = whole.start() + whole.as_str().find('{').unwrap_or(0) + 1;
            let _via = m.get(2).and_then(|f| self.resolve(file, f.as_str()));
            for part in m[1].split(',') {
                let spec = off + part.len() - part.trim_start().len();
                off += part.len() + 1;
                let p = part.trim();
                let p = regex_static(r"^type\s+").replace(p, "");
                let p = p.as_ref();
                if p.is_empty() {
                    continue;
                }
                let mut it = regex_static(r"\s+as\s+").splitn(p, 2);
                let local = it.next().unwrap_or("").trim().to_owned();
                let name = it.next().map_or(local.clone(), |x| x.trim().to_owned());
                if !has(&out, &name) {
                    let of = (local != name && name != "default").then(|| local.clone());
                    out.push((
                        name,
                        Export {
                            file: file.to_owned(),
                            line: line_at(&text, whole.start()),
                            at: Some(whole.start()),
                            spec: Some(spec),
                            of,
                        },
                    ));
                }
            }
        }
        for m in regex_static(r"\bexport\s+\*\s+as\s+([A-Za-z_$][\w$]*)\s+from").captures_iter(&text)
        {
            let name = m[1].to_owned();
            if !has(&out, &name) {
                out.push((
                    name,
                    Export {
                        file: file.to_owned(),
                        line: line_at(&text, m.get(0).expect("match").start()),
                        at: None,
                        spec: None,
                        of: None,
                    },
                ));
            }
        }
        for m in regex_static(r#"\bexport\s+\*\s+from\s*["']([^"']+)["']"#).captures_iter(&text) {
            let Some(target) = self.resolve(file, &m[1]) else {
                continue;
            };
            for (name, info) in self.exports_of(&target, when, seen) {
                if name != "default" && !has(&out, &name) {
                    out.push((name, info));
                }
            }
        }
        out
    }

    /// A JSDoc block right before offset `at` of `file` (now) that has @deprecated.
    fn deprecated_doc(&self, file: &str, at: usize) -> bool {
        let raw = self.ch.now(file).unwrap_or_default();
        let at = floor_char(&raw, at.min(raw.len()));
        let before = raw[..at].trim_end();
        if !before.ends_with("*/") {
            return false;
        }
        before
            .rfind("/**")
            .is_some_and(|open| regex_static(r"@deprecated\b").is_match(&before[open..]))
    }
}

fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn line_at(text: &str, index: usize) -> usize {
    text[..index].matches('\n').count() + 1
}

/// Comments blanked, positions kept (the regexes of public_api.mjs).
fn strip_comments(t: &str) -> String {
    let blank = |m: &str| -> String {
        m.chars()
            .map(|c| if c == '\n' { "\n".to_owned() } else { " ".repeat(c.len_utf8()) })
            .collect()
    };
    let s = regex_static(r"(?s)/\*.*?\*/").replace_all(t, |c: &regex::Captures| blank(&c[0]));
    regex_static(r#"(^|[^:"'`\\])(//[^\n]*)"#)
        .replace_all(&s, |c: &regex::Captures| format!("{}{}", &c[1], blank(&c[2])))
        .into_owned()
}

fn npm(ch: &Change, rule: &str, r: &mut Report) {
    let js = Js {
        ch,
        has_base: ch.base.is_some_and(Path::is_dir),
    };
    let Some(pkg) = ch
        .now("package.json")
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return;
    };
    let base_pkg = js
        .read("package.json", When::Base)
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(pkg);
    for entry in js.entries(&base_pkg) {
        let was = js.exports_of(&entry, When::Base, &mut HashSet::new());
        let now = js.exports_of(&entry, When::Now, &mut HashSet::new());
        let now_map: HashMap<&str, &Export> = now.iter().map(|(k, v)| (k.as_str(), v)).collect();
        let was_map: HashMap<&str, &Export> = was.iter().map(|(k, v)| (k.as_str(), v)).collect();
        for (name, info) in &was {
            if skip_js().is_match(&info.file) {
                continue;
            }
            match now_map.get(name.as_str()) {
                None => r.findings.push(at(rule, &info.file, 0, format!(
                    "{name} was exported by the package entry {entry} and is gone: keep it exported as a /** @deprecated Use <new name> */ alias; {MAJOR}"
                ))),
                Some(cur) => {
                    let renamed = cur.of.as_deref().is_some_and(|o| now_map.contains_key(o));
                    let was_alias = info.of.as_deref().is_some_and(|o| was_map.contains_key(o));
                    let documented = cur.at.is_some_and(|a| js.deprecated_doc(&cur.file, a))
                        || cur.spec.is_some_and(|s| js.deprecated_doc(&cur.file, s));
                    if renamed && !was_alias && !documented {
                        r.findings.push(at(rule, &cur.file, cur.line, format!(
                            "{name} is now an alias of another export: mark it /** @deprecated Use <new name> */ so its users see the rename"
                        )));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_of_a_module() {
        let t = pyast::parse("__all__ = ['a', 'b']\n__all__ += ['c']\n__all__.append('d')\n").unwrap();
        let a = dunder_all(&t.root()).unwrap();
        assert_eq!(a.into_iter().collect::<Vec<_>>(), ["a", "b", "c", "d"]);
        let t = pyast::parse("__all__ = [x for x in y]\n").unwrap();
        assert!(dunder_all(&t.root()).is_none());
    }

    #[test]
    fn shims() {
        let src = "import warnings\n__all__ = ['new']\ndef new(): pass\ndef old():\n    warnings.warn('x', DeprecationWarning, stacklevel=2)\n    return new()\n";
        let t = pyast::parse(src).unwrap();
        let ix = Index::new(std::slice::from_ref(&t));
        assert!(shimmed(&ix, &t.root(), "old"));
        let src = "__all__ = ['new']\ndef old():\n    \"\"\"deprecated, DeprecationWarning\"\"\"\n";
        let t = pyast::parse(src).unwrap();
        let ix = Index::new(std::slice::from_ref(&t));
        assert!(!shimmed(&ix, &t.root(), "old"));
    }

    #[test]
    fn comments_blanked() {
        assert_eq!(strip_comments("a /* x\ny */ b // c\n"), "a     \n     b     \n");
        assert_eq!(normalize("./dist/../src/index.js"), "src/index.js");
    }
}
