//! Python's engine: `ty server` (spec §15.1–§15.2), the port of `types_python.py`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::config::PythonConfig;
use crate::diag::Diagnostic;
use crate::rpc::{Framing, Handler, Rpc};
use crate::spec::{Ask, Decl, Found, Match, Verdict, verdicts};
use crate::text::{
    Loc, Pos, as_array, index_of, last_word, next_word, path_of, position, read_text, same_file,
    uri,
};
use crate::{DiagFile, log_stdio};

/// One `ty server` and what it was shown.
struct TyServer {
    rpc: Rpc,
    versions: HashMap<String, i64>,
    texts: HashMap<PathBuf, String>,
}

impl TyServer {
    fn start(
        cfg: &PythonConfig,
        root: &Path,
        untyped: &[String],
        log: &Option<PathBuf>,
    ) -> Result<(TyServer, String), String> {
        let (program, args) = cfg
            .ty
            .split_first()
            .ok_or_else(|| "no ty command configured".to_owned())?;
        let mut cmd = Command::new(program);
        cmd.args(args).arg("server").current_dir(root);
        let mut settings = serde_json::Map::new();
        if let (Some(py), Some(prefix)) = (&cfg.python, &cfg.prefix) {
            settings.insert(
                "pythonExtension".into(),
                json!({"activeEnvironment": {"executable": {"uri": uri(py), "sysPrefix": prefix}}}),
            );
        }
        if !untyped.is_empty() {
            let any: Vec<String> = untyped
                .iter()
                .flat_map(|n| [n.clone(), format!("{n}.**")])
                .collect();
            settings.insert(
                "configuration".into(),
                json!({"analysis": {"replace-imports-with-any": any}}),
            );
        }
        let settings = if settings.is_empty() {
            Value::Null
        } else {
            Value::Object(settings)
        };
        let handler: Handler = Box::new(move |method, params| {
            let items = params.get("items").and_then(Value::as_array);
            match items {
                Some(items) if method == "workspace/configuration" => Value::Array(
                    items
                        .iter()
                        .map(|i| {
                            if i.get("section").and_then(Value::as_str) == Some("ty") {
                                settings.clone()
                            } else {
                                Value::Null
                            }
                        })
                        .collect(),
                ),
                Some(items) => Value::Array(vec![Value::Null; items.len()]),
                None => Value::Null,
            }
        });
        let rpc = Rpc::spawn(cmd, Framing::Lsp, log_stdio(log), Some(handler))?;
        let root_uri = uri(root);
        let info = rpc
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": root_uri,
                    "workspaceFolders": [{"uri": root_uri, "name": "root"}],
                    "capabilities": {
                        "general": {"positionEncodings": ["utf-16"]},
                        "textDocument": {"typeDefinition": {"linkSupport": true}, "definition": {"linkSupport": true}, "typeHierarchy": {}, "diagnostic": {}},
                        "workspace": {"configuration": true, "workspaceFolders": true},
                    },
                }),
            )
            .wait()
            .map_err(|e| format!("ty server did not start: {e}"))?;
        rpc.notify("initialized", json!({}));
        let name = info["serverInfo"]["name"].as_str().unwrap_or("ty");
        let version = info["serverInfo"]["version"]
            .as_str()
            .unwrap_or("")
            .split(' ')
            .next()
            .unwrap_or("");
        Ok((
            TyServer {
                rpc,
                versions: HashMap::new(),
                texts: HashMap::new(),
            },
            format!("{name} {version}").trim().to_owned(),
        ))
    }

    /// Makes the server see `text` as the content of `path` (in memory only).
    fn show(&mut self, path: &Path, text: &str) {
        let u = uri(path);
        let v = self.versions.get(&u).copied().unwrap_or(0) + 1;
        self.versions.insert(u.clone(), v);
        if v == 1 {
            self.rpc.notify(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": u, "languageId": "python", "version": v, "text": text}}),
            );
        } else {
            self.rpc.notify(
                "textDocument/didChange",
                json!({"textDocument": {"uri": u, "version": v}, "contentChanges": [{"text": text}]}),
            );
        }
    }

    /// The file as it is on disk, shown when it differs from what the server has.
    fn sync(&mut self, path: &Path) -> Result<String, String> {
        let text = read_text(path).ok_or_else(|| format!("cannot read {}", path.display()))?;
        if self.texts.get(path) != Some(&text) {
            self.show(path, &text);
            self.texts.insert(path.to_path_buf(), text.clone());
        }
        Ok(text)
    }

    fn changed(&mut self, path: &Path) {
        if self.versions.contains_key(&uri(path)) {
            if path.exists() {
                let _ = self.sync(path);
            }
            return;
        }
        let kind = if path.exists() { 2 } else { 3 };
        self.rpc.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": uri(path), "type": kind}]}),
        );
    }

    /// The diagnostics of what the server has for `path`.
    fn diagnostics(&self, path: &Path) -> Result<Vec<Value>, String> {
        let r = self
            .rpc
            .request(
                "textDocument/diagnostic",
                json!({"textDocument": {"uri": uri(path)}}),
            )
            .wait()?;
        Ok(as_array(&r["items"]))
    }

    fn syntax_errors(&self, path: &Path) -> Result<usize, String> {
        Ok(syntax_count(&json!({"items": self.diagnostics(path)?})))
    }
}

/// The syntax errors of a diagnostic report.
fn syntax_count(report: &Value) -> usize {
    as_array(&report["items"])
        .iter()
        .filter(|d| d.get("code").and_then(Value::as_str) == Some("invalid-syntax"))
        .count()
}

fn locations(r: &Value) -> Vec<(Loc, Value)> {
    as_array(r)
        .into_iter()
        .filter_map(|l| {
            let u = l
                .get("targetUri")
                .or_else(|| l.get("uri"))?
                .as_str()?
                .to_owned();
            let rg = l
                .get("targetSelectionRange")
                .or_else(|| l.get("targetRange"))
                .or_else(|| l.get("range"))?;
            let pos = Pos::from_json(&rg["start"])?;
            let whole = l
                .get("targetRange")
                .or_else(|| l.get("range"))
                .cloned()
                .unwrap_or(Value::Null);
            Some((Loc { uri: u, pos }, whole))
        })
        .collect()
}

pub(crate) struct Python {
    root: PathBuf,
    cfg: PythonConfig,
    log: Option<PathBuf>,
    q: TyServer,
    d: Option<TyServer>,
    pub name: String,
    cache: HashMap<Loc, HashSet<Decl>>,
}

impl Python {
    pub fn start(root: &Path, cfg: &PythonConfig, log: &Option<PathBuf>) -> Result<Python, String> {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let (q, name) = TyServer::start(cfg, &root, &cfg.untyped, log)?;
        Ok(Python {
            root,
            cfg: cfg.clone(),
            log: log.clone(),
            q,
            d: None,
            name,
            cache: HashMap::new(),
        })
    }

    pub fn alive(&self) -> bool {
        self.q.rpc.alive() && self.d.as_ref().is_none_or(|d| d.rpc.alive())
    }

    pub fn stop(&mut self) {
        self.q.rpc.stop();
        if let Some(d) = &self.d {
            d.rpc.stop();
        }
    }

    pub fn changed(&mut self, path: &Path) {
        self.q.changed(path);
        if let Some(d) = &mut self.d {
            d.changed(path);
        }
    }

    pub fn answer(&mut self, matches: &[Match]) -> Result<Vec<Verdict>, String> {
        type Asked = Vec<(usize, String, usize, usize)>;
        let mut by_file: Vec<(String, Asked, Asked)> = Vec::new();
        for (n, m) in matches.iter().enumerate() {
            for (var, cond) in &m.conditions {
                let Some(b) = m.bindings.get(var) else {
                    continue;
                };
                let i = match by_file.iter().position(|f| f.0 == m.file) {
                    Some(i) => i,
                    None => {
                        by_file.push((m.file.clone(), Vec::new(), Vec::new()));
                        by_file.len() - 1
                    }
                };
                let item = (n, var.clone(), b.start, b.end);
                match cond.asks() {
                    Ask::Declared => by_file[i].2.push(item),
                    _ => by_file[i].1.push(item),
                }
            }
        }
        let mut found = HashMap::new();
        for (file, wanted, named) in by_file {
            let path = self.root.join(&file);
            found.extend(self.check(&path, &wanted, &named)?);
        }
        Ok(verdicts(matches, &found, '.'))
    }

    /// (package, module) of a declaration file (spec §15.1 rule 5).
    fn declared_in(&self, path: &Path) -> (Option<String>, Option<String>) {
        let p = path.to_string_lossy().replace('\\', "/");
        let mut rel: Option<String> = None;
        for marker in ["/site-packages/", "/dist-packages/"] {
            if let Some(i) = p.rfind(marker) {
                rel = Some(p[i + marker.len()..].to_owned());
                break;
            }
        }
        if rel.is_none() {
            // the standard library's own sources: lib/python3.X/
            let mut from = 0;
            while let Some(i) = p[from..].find("/lib/python3.") {
                let at = from + i + "/lib/python3.".len();
                let digits = p[at..].bytes().take_while(u8::is_ascii_digit).count();
                if digits > 0 && p[at + digits..].starts_with('/') {
                    rel = Some(p[at + digits + 1..].to_owned());
                }
                from = at;
            }
        }
        if rel.is_none() {
            if let Some(i) = p.rfind("/typeshed/") {
                let rest: Vec<&str> = p[i + "/typeshed/".len()..].split('/').collect();
                if let Some(s) = rest.iter().position(|s| *s == "stdlib") {
                    rel = Some(rest[s + 1..].join("/"));
                } else if let Some(s) = rest.iter().position(|s| *s == "stubs") {
                    rel = Some(rest.get(s + 2..).unwrap_or_default().join("/"));
                }
            }
        }
        let mut project = false;
        let rel = match rel {
            Some(r) => r,
            None => {
                let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                let Ok(r) = real.strip_prefix(&self.root) else {
                    return (None, None);
                };
                project = true;
                let r = r.to_string_lossy().into_owned();
                r.strip_prefix("src/").map(str::to_owned).unwrap_or(r)
            }
        };
        let rel = rel
            .strip_suffix(".pyi")
            .or_else(|| rel.strip_suffix(".py"))
            .unwrap_or(&rel);
        let mut segs: Vec<String> = rel
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if segs.last().map(String::as_str) == Some("__init__") {
            segs.pop();
        }
        if segs.is_empty() {
            return (None, None);
        }
        if let Some(s) = segs[0].strip_suffix("-stubs") {
            segs[0] = s.to_owned();
        }
        let module = segs.join(".");
        ((!project).then(|| segs[0].clone()), Some(module))
    }

    /// The declarations of the type declared at `loc` and of its supertypes (the server's type
    /// hierarchy), at most 64.
    fn hierarchy(&mut self, loc: &Loc) -> Result<HashSet<Decl>, String> {
        if let Some(c) = self.cache.get(loc) {
            return Ok(c.clone());
        }
        let mut out = HashSet::new();
        let mut seen = HashSet::new();
        let items = self
            .q
            .rpc
            .request(
                "textDocument/prepareTypeHierarchy",
                json!({"textDocument": {"uri": loc.uri}, "position": loc.pos.json()}),
            )
            .wait()?;
        let items = as_array(&items);
        let file = path_of(&loc.uri);
        if items.is_empty() {
            let (package, module) = self.declared_in(&file);
            out.insert(Decl {
                package,
                module,
                name: word_at(&file, loc.pos, None),
            });
        }
        let mut todo = items;
        while let Some(item) = todo.pop() {
            if seen.len() >= 64 {
                break;
            }
            let key = (
                item["uri"].as_str().unwrap_or("").to_owned(),
                item["selectionRange"]["start"].to_string(),
            );
            if !seen.insert(key) {
                continue;
            }
            let (package, module) = self.declared_in(&path_of(item["uri"].as_str().unwrap_or("")));
            out.insert(Decl {
                package,
                module,
                name: item["name"].as_str().map(str::to_owned),
            });
            let sup = self
                .q
                .rpc
                .request("typeHierarchy/supertypes", json!({"item": item}))
                .wait()?;
            todo.extend(as_array(&sup));
        }
        let outside = std::fs::canonicalize(&file)
            .map(|f| !f.starts_with(&self.root))
            .unwrap_or(true);
        if outside || loc.uri.contains("/site-packages/") {
            self.cache.insert(loc.clone(), out.clone());
        }
        Ok(out)
    }

    /// {(match, variable): what was found} for the types (`wanted`) and the declared names (`named`)
    /// of one file: each expression is bound to a fresh name, in memory only, and the type
    /// definition of that name is asked; then the file is shown as it is.
    #[allow(clippy::type_complexity)]
    fn check(
        &mut self,
        path: &Path,
        wanted: &[(usize, String, usize, usize)],
        named: &[(usize, String, usize, usize)],
    ) -> Result<HashMap<(usize, String), Found>, String> {
        let text = self.q.sync(path)?;
        let mut found = if named.is_empty() {
            HashMap::new()
        } else {
            self.declared(path, &text, named)?
        };
        if wanted.is_empty() {
            return Ok(found);
        }
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let slot: Vec<usize> = wanted
            .iter()
            .map(|&(_, _, s, e)| {
                let r = (s.min(text.len()), e.min(text.len()));
                ranges.iter().position(|x| *x == r).unwrap_or_else(|| {
                    ranges.push(r);
                    ranges.len() - 1
                })
            })
            .collect();
        let mut unchecked: HashSet<usize> = HashSet::new();
        let mut base_errors: Option<usize> = None;
        let results = loop {
            let active: Vec<usize> = (0..ranges.len())
                .filter(|n| !unchecked.contains(n))
                .collect();
            let (overlay, anchors) = bind(&text, &ranges, &active);
            self.q.show(path, &overlay);
            // code that is not an expression (a parameter's or a keyword argument's name) makes the
            // binding unparsable: its type cannot be asked (asked with the types, in one go)
            let syntax = self.q.rpc.request(
                "textDocument/diagnostic",
                json!({"textDocument": {"uri": uri(path)}}),
            );
            let waits: Vec<_> = active
                .iter()
                .map(|n| {
                    (
                        *n,
                        self.q.rpc.request(
                            "textDocument/typeDefinition",
                            json!({"textDocument": {"uri": uri(path)}, "position": position(&overlay, anchors[n]).json()}),
                        ),
                    )
                })
                .collect();
            let errors = syntax_count(&syntax.wait()?);
            let mut results: HashMap<usize, Vec<Loc>> = HashMap::new();
            for (n, w) in waits {
                results.insert(n, locations(&w.wait()?).into_iter().map(|l| l.0).collect());
            }
            if errors == 0 {
                break results;
            }
            let base = match base_errors {
                Some(b) => b,
                None => {
                    self.q.show(path, &text);
                    let b = self.q.syntax_errors(path)?;
                    base_errors = Some(b);
                    b
                }
            };
            let mut more = false;
            if errors > base {
                for &n in &active {
                    let (one, _) = bind(&text, &ranges, &[n]);
                    self.q.show(path, &one);
                    if self.q.syntax_errors(path)? > base {
                        unchecked.insert(n);
                        more = true;
                    }
                }
            }
            if !more {
                // the errors are not one binding's: answer with what was found
                self.q.show(path, &overlay);
                break results;
            }
        };
        self.q.show(path, &text);
        let mut per_range = Vec::new();
        for n in 0..ranges.len() {
            if unchecked.contains(&n) {
                per_range.push(Found::Unchecked);
                continue;
            }
            let mut decls = HashSet::new();
            for l in results.get(&n).cloned().unwrap_or_default() {
                decls.extend(self.hierarchy(&l)?);
            }
            // Any and Unknown (an unannotated parameter, an unresolved import) say nothing
            let unknown = decls.iter().all(|d| {
                (d.package.as_deref() == Some("typing") && d.name.as_deref() == Some("Any"))
                    || d.module
                        .as_deref()
                        .unwrap_or("")
                        .starts_with("ty_extensions")
            });
            let mut decls: Vec<Decl> = decls.into_iter().collect();
            decls.sort();
            per_range.push(Found::Decls { decls, unknown });
        }
        for (i, w) in wanted.iter().enumerate() {
            found.insert((w.0, w.1.clone()), per_range[slot[i]].clone());
        }
        Ok(found)
    }

    /// The declarations the definition of each expression's last name gives (spec §15.6).
    fn declared(
        &mut self,
        path: &Path,
        text: &str,
        named: &[(usize, String, usize, usize)],
    ) -> Result<HashMap<(usize, String), Found>, String> {
        let mut out = HashMap::new();
        let mut asked: Vec<(usize, crate::rpc::Pending)> = Vec::new();
        let mut at_of: Vec<((usize, String), usize)> = Vec::new();
        for (n, var, s, e) in named {
            let expr = text.get(*s..*e).unwrap_or("");
            let name = last_word(expr, false)
                .filter(|(i, _)| *i == 0 || expr[..*i].trim_end().ends_with('.'));
            let Some((i, _)) = name else {
                out.insert(
                    (*n, var.clone()),
                    Found::Decls {
                        decls: vec![],
                        unknown: true,
                    },
                );
                continue;
            };
            let at = s + i;
            if !asked.iter().any(|a| a.0 == at) {
                asked.push((
                    at,
                    self.q.rpc.request(
                        "textDocument/definition",
                        json!({"textDocument": {"uri": uri(path)}, "position": position(text, at).json()}),
                    ),
                ));
            }
            at_of.push(((*n, var.clone()), at));
        }
        let mut answers: HashMap<usize, Found> = HashMap::new();
        for (at, wait) in asked {
            let mut decls = HashSet::new();
            for (l, whole) in locations(&wait.wait()?) {
                let f = path_of(&l.uri);
                let body = if same_file(&f, path) {
                    Some(text.to_owned())
                } else {
                    read_text(&f)
                };
                let Some(body) = body else { continue };
                if in_import(&body, index_of(&body, l.pos)) {
                    continue; // the server stopped at an import it could not resolve
                }
                let (package, module) = self.declared_in(&f);
                let zero = json!({"line": 0, "character": 0});
                if whole["start"] == zero && whole["end"] == zero {
                    // a module: declared by its file, with its name
                    let name = module
                        .as_deref()
                        .and_then(|m| m.rsplit('.').next())
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);
                    decls.insert(Decl {
                        package,
                        module,
                        name,
                    });
                } else {
                    decls.insert(Decl {
                        package,
                        module,
                        name: word_at(&f, l.pos, Some(&body)),
                    });
                }
            }
            let mut decls: Vec<Decl> = decls.into_iter().collect();
            decls.sort();
            let unknown = decls.is_empty();
            answers.insert(at, Found::Decls { decls, unknown });
        }
        for (k, at) in at_of {
            out.insert(k, answers[&at].clone());
        }
        Ok(out)
    }

    /// The diagnostics of each file now and with its base text, from a server that sees every
    /// untyped package as Any (as `ty check` does).
    pub fn diagnose(
        &mut self,
        files: &[DiagFile],
    ) -> Result<Vec<crate::diag::FileDiagnostics>, String> {
        let all = self
            .cfg
            .untyped_all
            .clone()
            .unwrap_or_else(|| self.cfg.untyped.clone());
        let same = all.iter().collect::<HashSet<_>>() == self.cfg.untyped.iter().collect();
        if !same && self.d.is_none() {
            let (d, _) = TyServer::start(&self.cfg, &self.root, &all, &self.log)?;
            self.d = Some(d);
        }
        let root = self.root.clone();
        let server = if same {
            &mut self.q
        } else {
            self.d.as_mut().expect("diagnostics server")
        };
        let mut out = Vec::new();
        for f in files {
            let path = root.join(&f.path);
            let Some(now) = read_text(&path) else {
                out.push((vec![], vec![]));
                continue;
            };
            let rel = f.path.clone();
            let of = |server: &mut TyServer, text: &str| -> Result<Vec<Diagnostic>, String> {
                server.show(&path, text);
                Ok(server
                    .diagnostics(&path)?
                    .iter()
                    .map(|d| Diagnostic::from_lsp(&rel, d))
                    .collect())
            };
            let base = match f.base.as_ref().and_then(|b| read_text(b)) {
                Some(b) => of(server, &b)?,
                None => vec![],
            };
            let now_d = of(server, &now)?;
            server.texts.insert(path.clone(), now);
            out.push((now_d, base));
        }
        Ok(out)
    }
}

/// The text with the expressions of `ranges[n]` (n in `which`) each bound to a fresh name
/// (`(__forge_t<n> := (<expr>))`), and {n: offset inside that name}.
fn bind(text: &str, ranges: &[(usize, usize)], which: &[usize]) -> (String, HashMap<usize, usize>) {
    // at each start the opening, at each end the closing; at the same offset closings first
    // (inner first), then openings (outer first)
    let mut events: Vec<(usize, u8, i64, usize, String)> = Vec::new();
    for &n in which {
        let (s, e) = ranges[n];
        let size = i64::try_from(e - s).unwrap_or(i64::MAX);
        events.push((s, 1, -size, n, format!("(__forge_t{n} := (")));
        events.push((e, 0, size, n, "))".to_owned()));
    }
    events.sort();
    let mut out = String::with_capacity(text.len() + events.len() * 16);
    let mut last = 0;
    let mut anchors = HashMap::new();
    for (off, kind, _, n, piece) in events {
        out.push_str(&text[last..off]);
        last = off;
        if kind == 1 {
            anchors.insert(n, out.len() + 3);
        }
        out.push_str(&piece);
    }
    out.push_str(&text[last..]);
    (out, anchors)
}

/// The declared name at `pos` (`class`, `def`, `type`, `async` skipped).
fn word_at(path: &Path, pos: Pos, text: Option<&str>) -> Option<String> {
    let owned;
    let text = match text {
        Some(t) => t,
        None => {
            owned = read_text(path)?;
            &owned
        }
    };
    let mut i = index_of(text, pos);
    loop {
        let (at, w) = next_word(text, i, false)?;
        if !["class", "def", "type", "async"].contains(&w) {
            return Some(w.to_owned());
        }
        i = at + w.len();
    }
}

/// Whether byte offset `index` is inside an import statement of the Python source `text`.
fn in_import(text: &str, index: usize) -> bool {
    import_spans(text)
        .iter()
        .any(|&(a, b)| a <= index && index < b)
}

/// The byte spans of the import statements of a Python source (`import …`, `from … import …`),
/// continuation lines included.
fn import_spans(text: &str) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut line_start = 0;
    while line_start < b.len() {
        let mut i = line_start;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        let rest = &text[i..];
        let is_import = ["import", "from"]
            .iter()
            .any(|k| rest.starts_with(k) && rest[k.len()..].starts_with([' ', '\t', '\\', '(']));
        // the end of the logical line
        let mut depth = 0i32;
        let mut j = i;
        while j < b.len() {
            match b[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'#' => {
                    while j < b.len() && b[j] != b'\n' {
                        j += 1;
                    }
                    continue;
                }
                b'\\' if b.get(j + 1) == Some(&b'\n') => j += 1,
                b'\'' | b'"' => {
                    let q = b[j];
                    j += 1;
                    while j < b.len() && b[j] != q && b[j] != b'\n' {
                        if b[j] == b'\\' {
                            j += 1;
                        }
                        j += 1;
                    }
                }
                b';' if depth <= 0 && is_import => break,
                b'\n' if depth <= 0 => break,
                _ => {}
            }
            j += 1;
        }
        if is_import {
            out.push((i, j.min(b.len())));
        }
        line_start = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_nested_expressions() {
        let t = "x = f(a.b)\n";
        let a = t.find("a.b").unwrap();
        let (o, anchors) = bind(t, &[(a, a + 3), (a - 2, a + 4)], &[0, 1]);
        assert_eq!(o, "x = (__forge_t1 := (f((__forge_t0 := (a.b)))))\n");
        assert_eq!(&o[anchors[&0]..anchors[&0] + 3], "for");
    }

    #[test]
    fn finds_import_statements() {
        let t = "import os\nfrom a import (\n  b,\n  c)\nx = 1\n";
        let spans = import_spans(t);
        assert_eq!(spans.len(), 2);
        assert!(in_import(t, t.find("c)").unwrap()));
        assert!(!in_import(t, t.find("x =").unwrap()));
    }
}
