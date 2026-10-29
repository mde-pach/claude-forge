//! Django REST framework views declare who may call them (`drf_permissions.py`). Arguments: the
//! files to check (none: every tracked Python file).
//!
//! The default is read from the settings module Django runs with: the `DJANGO_SETTINGS_MODULE`
//! that manage.py, wsgi.py or asgi.py sets (star imports followed), else a settings.py or
//! settings/ package outside tests. When it does not set `REST_FRAMEWORK["DEFAULT_PERMISSION_CLASSES"]`
//! to a non-empty list without AllowAny (DRF's default is AllowAny), each class that inherits a
//! DRF APIView, generic view or viewset (directly or through the project's own classes) must get
//! a non-empty `permission_classes` (or `get_permissions`) from itself or a project ancestor, and
//! each `@api_view` function must have `@permission_classes`.

use std::collections::{HashMap, HashSet};

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};

use crate::pyast::{
    self, N, Root, args, assign, bases, body, decorators, elements, ident, is_class, is_def,
    last_name, name_of, string, walk,
};
use crate::util::{Change, at, could_not_run, line, regex_static, strip_script};

pub struct DrfPermissions;

const HINT: &str = "Set permission_classes on the view (IsAuthenticated, or the project's permission), or a restrictive REST_FRAMEWORK DEFAULT_PERMISSION_CLASSES in settings; a public endpoint is the user's call";

fn tests_re() -> &'static regex::Regex {
    regex_static(r"(^|/)(tests?/|test_[^/]*$|[^/]*_test\.py$|conftest\.py$|migrations/)")
}

impl Builtin for DrfPermissions {
    fn name(&self) -> &'static str {
        "drf_permissions"
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        let (_, args) = strip_script(args);
        let ch = Change::new(ctx);
        let mut r = Report::default();
        let tracked = match ch.ls_files(&["*.py"]) {
            Ok(t) => t,
            Err(e) => {
                r.notes.push(could_not_run(&ctx.rule, e));
                vec![]
            }
        };
        let mut paths: Vec<String> = args.iter().filter(|p| p.ends_with(".py")).cloned().collect();
        if paths.is_empty() {
            paths = tracked.clone();
        }
        paths.retain(|p| !tests_re().is_match(p));
        if paths.is_empty() {
            return r;
        }
        let parsed = Parsed::new(&ch, &tracked);
        if default_is_restricted(&parsed, &tracked) {
            return r;
        }
        let classes = Classes::new(&parsed, &tracked);
        for p in &paths {
            let Some(tree) = ch.now(p).and_then(|t| pyast::parse(&t)) else {
                continue;
            };
            for node in walk(&tree.root()) {
                if is_class(&node) {
                    let (drf, declared) = classes.resolve(&node, &mut HashSet::new());
                    if drf && !declared {
                        r.findings.push(at(&ctx.rule, p, pyast::line(&node), format!("{} gets no non-empty permission_classes, and the settings Django runs with set no DEFAULT_PERMISSION_CLASSES other than AllowAny, so DRF lets anyone call it", name_of(&node))));
                    }
                } else if is_def(&node) {
                    let decos: Vec<String> = decorators(&node).iter().map(last_name).collect();
                    if decos.iter().any(|d| d == "api_view")
                        && !decos.iter().any(|d| d == "permission_classes")
                    {
                        r.findings.push(at(&ctx.rule, p, pyast::line(&node), format!("{} (@api_view) has no @permission_classes, and the settings Django runs with set no DEFAULT_PERMISSION_CLASSES other than AllowAny, so DRF lets anyone call it", name_of(&node))));
                    }
                }
            }
        }
        if !r.findings.is_empty() {
            r.findings.push(line(&ctx.rule, HINT));
        }
        r
    }
}

/// The project's Python files, parsed once.
struct Parsed {
    trees: HashMap<String, Root>,
}

impl Parsed {
    fn new(ch: &Change, files: &[String]) -> Self {
        let trees = files
            .iter()
            .filter_map(|f| Some((f.clone(), pyast::parse(&ch.now(f)?)?)))
            .collect();
        Parsed { trees }
    }

    fn get(&self, p: &str) -> Option<N<'_>> {
        self.trees.get(p).map(|t| t.root())
    }
}

/// Paths of the settings modules Django runs with.
fn settings_modules(parsed: &Parsed, files: &[String]) -> Vec<String> {
    let set: HashSet<&str> = files.iter().map(String::as_str).collect();
    let mut found: Vec<String> = vec![];
    for path in files {
        if !regex_static(r"(^|/)(manage|wsgi|asgi)\.py$").is_match(path) {
            continue;
        }
        let Some(tree) = parsed.get(path) else { continue };
        let root = path.rsplit_once('/').map_or("", |(d, _)| d);
        for node in walk(&tree) {
            if node.kind() != "call" {
                continue;
            }
            let a = args(&node).pos;
            if a.len() < 2 {
                continue;
            }
            let (Some(key), Some(value)) = (string(&a[0]), string(&a[1])) else {
                continue;
            };
            if key != "DJANGO_SETTINGS_MODULE" {
                continue;
            }
            let module = value.replace('.', "/");
            let root_src = if root.is_empty() {
                "src".to_owned()
            } else {
                format!("{root}/src")
            };
            let mut bases_: Vec<String> = vec![];
            for b in [root.to_owned(), String::new(), "src".to_owned(), root_src] {
                if !bases_.contains(&b) {
                    bases_.push(b);
                }
            }
            for base in bases_ {
                let prefix = if base.is_empty() {
                    String::new()
                } else {
                    format!("{base}/")
                };
                for cand in [
                    format!("{prefix}{module}.py"),
                    format!("{prefix}{module}/__init__.py"),
                ] {
                    if set.contains(cand.as_str()) && !found.contains(&cand) {
                        found.push(cand);
                    }
                }
            }
        }
    }
    if found.is_empty() {
        let mut s: Vec<String> = files
            .iter()
            .filter(|p| {
                regex_static(r"(^|/)settings(\.py$|/)").is_match(p) && !tests_re().is_match(p)
            })
            .cloned()
            .collect();
        s.sort();
        found = s;
    }
    found
}

/// Modules a settings module imports with `*`, as paths.
fn star_imports(path: &str, tree: &N, files: &[String]) -> Vec<String> {
    let mut out = vec![];
    let pkg = path.rsplit_once('/').map_or("", |(d, _)| d);
    for st in body(tree) {
        if st.kind() != "import_from_statement"
            || !st.children().any(|c| c.kind() == "wildcard_import")
        {
            continue;
        }
        let Some(m) = st.field("module_name") else { continue };
        let cands: Vec<String> = if m.kind() == "relative_import" {
            let prefix = m
                .children()
                .find(|c| c.kind() == "import_prefix")
                .map_or(0, |p| p.text().chars().filter(|c| *c == '.').count());
            let module = m
                .children()
                .find(|c| c.kind() == "dotted_name")
                .map(|d| d.text().replace('.', "/"))
                .unwrap_or_default();
            let mut base = pkg.to_owned();
            for _ in 0..prefix.saturating_sub(1) {
                base = base.rsplit_once('/').map_or(String::new(), |(d, _)| d.to_owned());
            }
            let full = if base.is_empty() {
                module
            } else {
                format!("{base}/{module}")
            };
            vec![format!("{full}.py"), format!("{full}/__init__.py")]
        } else {
            let module = m.text().replace('.', "/");
            files
                .iter()
                .filter(|c| {
                    c.ends_with(&format!("{module}.py")) || c.ends_with(&format!("{module}/__init__.py"))
                })
                .cloned()
                .collect()
        };
        out.extend(cands.into_iter().filter(|c| files.contains(c)));
    }
    out
}

/// The settings Django runs with set DEFAULT_PERMISSION_CLASSES to something other than AllowAny.
fn default_is_restricted(parsed: &Parsed, files: &[String]) -> bool {
    let mut todo: std::collections::VecDeque<String> = settings_modules(parsed, files).into();
    let mut seen = HashSet::new();
    while let Some(path) = todo.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let Some(tree) = parsed.get(&path) else { continue };
        let mut value: Option<N> = None;
        for node in walk(&tree) {
            if node.kind() == "dictionary" {
                for pair in node.children().filter(|c| c.kind() == "pair") {
                    if pair.field("key").and_then(|k| string(&k)).as_deref()
                        == Some("DEFAULT_PERMISSION_CLASSES")
                    {
                        value = pair.field("value");
                    }
                }
            }
            if let Some(a) = assign(&node)
                && !a.aug
                && let Some(t) = a.targets.first()
                && t.kind() == "subscript"
                && t.field("subscript").and_then(|s| string(&s)).as_deref()
                    == Some("DEFAULT_PERMISSION_CLASSES")
                && let Some(v) = a.value
            {
                value = Some(v);
            }
        }
        if let Some(v) = value {
            return restricted(&v);
        }
        todo.extend(star_imports(&path, &tree, files));
    }
    false
}

fn restricted(v: &N) -> bool {
    if matches!(&*v.kind(), "list" | "tuple")
        && let Some(elts) = elements(v)
    {
        let names: Vec<String> = elts.iter().filter_map(string).collect();
        if names.len() != elts.len() {
            return true; // computed entries: assume the project knows
        }
        return !names.is_empty() && !names.iter().any(|n| n.ends_with("AllowAny"));
    }
    true // computed: not ours to judge
}

/// The class sets a non-empty permission_classes or defines get_permissions (None: neither).
fn declares(cls: &N) -> Option<bool> {
    for st in body(cls) {
        if let Some(a) = assign(&st)
            && !a.aug
            && a.targets.iter().any(|t| ident(t).as_deref() == Some("permission_classes"))
        {
            return Some(match &a.value {
                Some(v) if matches!(&*v.kind(), "list" | "tuple") => {
                    !elements(v).is_some_and(|e| e.is_empty())
                }
                _ => true,
            });
        }
        if is_def(&st) && name_of(&st) == "get_permissions" {
            return Some(true);
        }
    }
    None
}

/// The project's classes by name, to follow inheritance through its own bases.
struct Classes<'p> {
    by_name: HashMap<String, Vec<N<'p>>>,
}

impl<'p> Classes<'p> {
    fn new(parsed: &'p Parsed, files: &[String]) -> Self {
        let mut by_name: HashMap<String, Vec<N<'p>>> = HashMap::new();
        for path in files {
            if tests_re().is_match(path) {
                continue;
            }
            let Some(tree) = parsed.get(path) else { continue };
            for node in walk(&tree) {
                if is_class(&node) {
                    by_name.entry(name_of(&node)).or_default().push(node);
                }
            }
        }
        Classes { by_name }
    }

    /// (is a DRF view, declares permissions) for the class, following project bases.
    fn resolve(&self, cls: &N, seen: &mut HashSet<(usize, usize)>) -> (bool, bool) {
        let id = (cls.get_doc() as *const _ as usize, cls.node_id());
        if !seen.insert(id) {
            return (false, false);
        }
        let own = declares(cls);
        let mut drf = false;
        let mut decl = own == Some(true);
        let drf_base = regex_static(
            r"^(APIView|GenericAPIView|(List|Create|Retrieve|Update|Destroy|ListCreate|RetrieveUpdate|RetrieveDestroy|RetrieveUpdateDestroy)APIView|ViewSet|GenericViewSet|ModelViewSet|ReadOnlyModelViewSet|ViewSetMixin)$",
        );
        for b in bases(cls) {
            let name = last_name(&b);
            if drf_base.is_match(&name) {
                drf = true;
                continue;
            }
            for parent in self.by_name.get(&name).into_iter().flatten() {
                let (d, p) = self.resolve(parent, seen);
                drf = drf || d;
                if own.is_none() {
                    decl = decl || p;
                }
            }
        }
        (drf, decl)
    }
}
