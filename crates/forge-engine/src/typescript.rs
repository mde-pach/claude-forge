//! TypeScript's engine: the project's own TypeScript (spec §15.1–§15.2, §15.6, §15.7), the port
//! of `types_typescript.mjs`. TypeScript 7 and later through its language server
//! (`tsc --lsp --stdio`, the native compiler run directly); 5 and 6 through their tsserver.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::config::TypeScriptConfig;
use crate::diag::Diagnostic;
use crate::rpc::{Framing, Handler, Pending, Rpc};
use crate::spec::{Ask, Decl, Found, Match, Verdict, verdicts};
use crate::text::{
    Loc, Pos, as_array, index_of, last_word, path_of, position, read_text, same_file, uri,
    utf16_index,
};
use crate::tsscan::{
    alias_members, bindings, heritage, in_import, key_position, name_at, skip_trivia,
    top_level_names,
};
use crate::{DiagFile, log_stdio};

/// TypeScript's suggestion diagnostics for a deprecated use.
const DEPRECATED_CODES: [i64; 2] = [6385, 6387];

/// The project's TypeScript package.
#[derive(Debug, Clone)]
pub(crate) struct ProjectTs {
    pub dir: PathBuf,
    pub version: String,
    pub major: u32,
}

/// `node_modules/typescript`, resolved from the root as Node resolves it.
pub(crate) fn project_typescript(root: &Path) -> Result<ProjectTs, String> {
    for d in root.ancestors() {
        let pkg = d.join("node_modules/typescript/package.json");
        if let Some(text) = read_text(&pkg) {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let version = v["version"].as_str().unwrap_or("").to_owned();
            let major = version
                .split('.')
                .next()
                .and_then(|m| m.parse().ok())
                .unwrap_or(0);
            let dir = pkg.parent().expect("package dir").to_path_buf();
            return Ok(ProjectTs {
                dir: std::fs::canonicalize(&dir).unwrap_or(dir),
                version,
                major,
            });
        }
    }
    Err(
        "the project has no typescript package (node_modules/typescript): install the project's dependencies"
            .to_owned(),
    )
}

/// TypeScript 7's native compiler: `@typescript/typescript-<platform>-<arch>/lib/tsc`.
fn native_tsc(ts: &ProjectTs) -> Option<PathBuf> {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        o => o,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        a => a,
    };
    let pkg = format!("@typescript/typescript-{platform}-{arch}/lib/tsc");
    ts.dir
        .ancestors()
        .map(|d| d.join("node_modules").join(&pkg))
        .chain(std::iter::once(ts.dir.join("node_modules").join(&pkg)))
        .find(|p| p.is_file())
}

type Later<T> = Box<dyn FnOnce() -> Result<T, String>>;

/// An inlay hint: where it is and its parts (with the declaration a part names).
struct Hint {
    pos: Pos,
    parts: Vec<(String, Option<Loc>)>,
}

/// The two servers, through one interface; positions and results as LSP.
enum Server {
    Lsp {
        rpc: Rpc,
        versions: HashMap<String, i64>,
    },
    TsServer {
        rpc: Rpc,
        open: HashSet<PathBuf>,
        shown: HashMap<PathBuf, String>,
        root: PathBuf,
    },
}

fn language_id(file: &Path) -> &'static str {
    let f = file.to_string_lossy();
    let ts = [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|e| f.ends_with(e));
    match (ts, f.ends_with('x')) {
        (true, true) => "typescriptreact",
        (true, false) => "typescript",
        (false, true) => "javascriptreact",
        (false, false) => "javascript",
    }
}

fn script_kind(file: &Path) -> &'static str {
    match language_id(file) {
        "typescript" => "TS",
        "typescriptreact" => "TSX",
        "javascript" => "JS",
        _ => "JSX",
    }
}

fn ts_pos(p: &Value) -> Pos {
    Pos {
        line: p["line"].as_u64().unwrap_or(1).saturating_sub(1) as u32,
        character: p["offset"].as_u64().unwrap_or(1).saturating_sub(1) as u32,
    }
}

fn file_uri(f: &str) -> String {
    uri(Path::new(f))
}

impl Server {
    fn start(
        root: &Path,
        ts: &ProjectTs,
        cfg: &TypeScriptConfig,
        log: &Option<PathBuf>,
    ) -> Result<(Server, String), String> {
        if ts.major >= 7 {
            let mut cmd = match native_tsc(ts) {
                Some(exe) => Command::new(exe),
                None => {
                    let rt = cfg.js_runtime.clone().ok_or_else(|| {
                        format!(
                            "TypeScript {}'s native compiler is not installed and no JavaScript runtime is configured",
                            ts.version
                        )
                    })?;
                    let mut c = Command::new(rt);
                    c.arg(ts.dir.join("bin/tsc"));
                    c
                }
            };
            cmd.args(["--lsp", "--stdio"]).current_dir(root);
            let handler: Handler = Box::new(|method, params| {
                let items = params.get("items").and_then(Value::as_array);
                match items {
                    Some(items) if method == "workspace/configuration" => Value::Array(
                        items
                            .iter()
                            .map(|i| match i.get("section").and_then(Value::as_str) {
                                Some("js/ts" | "typescript") => {
                                    json!({"inlayHints": {"parameterTypes": {"enabled": true}}})
                                }
                                _ => Value::Null,
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
                            "textDocument": {"typeDefinition": {"linkSupport": true}, "definition": {"linkSupport": true}, "inlayHint": {}, "diagnostic": {}, "hover": {"contentFormat": ["markdown"]}},
                            "workspace": {"configuration": true, "workspaceFolders": true},
                        },
                    }),
                )
                .wait()
                .map_err(|e| format!("tsc --lsp did not start: {e}"))?;
            rpc.notify("initialized", json!({}));
            let name = format!(
                "{} {}",
                info["serverInfo"]["name"].as_str().unwrap_or("tsc"),
                info["serverInfo"]["version"].as_str().unwrap_or("")
            );
            Ok((
                Server::Lsp {
                    rpc,
                    versions: HashMap::new(),
                },
                name.trim().to_owned(),
            ))
        } else {
            let rt = cfg.js_runtime.clone().ok_or_else(|| {
                format!(
                    "TypeScript {} needs a JavaScript runtime for tsserver, and none is configured",
                    ts.version
                )
            })?;
            let mut cmd = Command::new(rt);
            cmd.arg(ts.dir.join("lib/tsserver.js"))
                .args([
                    "--disableAutomaticTypingAcquisition",
                    "--suppressDiagnosticEvents",
                ])
                .current_dir(root);
            let rpc = Rpc::spawn(cmd, Framing::TsServer, log_stdio(log), None)?;
            rpc.request(
                "configure",
                json!({"preferences": {"includeInlayFunctionParameterTypeHints": true, "interactiveInlayHints": true}, "hostInfo": "forge"}),
            )
            .wait()
            .map_err(|e| format!("tsserver did not start: {e}"))?;
            Ok((
                Server::TsServer {
                    rpc,
                    open: HashSet::new(),
                    shown: HashMap::new(),
                    root: root.to_path_buf(),
                },
                format!("tsserver {}", ts.version),
            ))
        }
    }

    fn rpc(&self) -> &Rpc {
        match self {
            Server::Lsp { rpc, .. } | Server::TsServer { rpc, .. } => rpc,
        }
    }

    fn show(&mut self, file: &Path, text: &str) {
        match self {
            Server::Lsp { rpc, versions } => {
                let u = uri(file);
                let v = versions.get(&u).copied().unwrap_or(0) + 1;
                versions.insert(u.clone(), v);
                if v == 1 {
                    rpc.notify(
                        "textDocument/didOpen",
                        json!({"textDocument": {"uri": u, "languageId": language_id(file), "version": v, "text": text}}),
                    );
                } else {
                    rpc.notify(
                        "textDocument/didChange",
                        json!({"textDocument": {"uri": u, "version": v}, "contentChanges": [{"text": text}]}),
                    );
                }
            }
            Server::TsServer {
                rpc,
                open,
                shown,
                root,
            } => {
                let f = file.to_string_lossy().into_owned();
                if open.insert(file.to_path_buf()) {
                    rpc.notify(
                        "open",
                        json!({"file": f, "fileContent": text, "scriptKindName": script_kind(file), "projectRootPath": root}),
                    );
                } else {
                    // only the span that differs: tsserver reparses less
                    let old = shown.get(file).cloned().unwrap_or_default();
                    let (o, t) = (old.as_bytes(), text.as_bytes());
                    let mut a = 0;
                    while a < o.len() && a < t.len() && o[a] == t[a] {
                        a += 1;
                    }
                    while a > 0 && !(old.is_char_boundary(a) && text.is_char_boundary(a)) {
                        a -= 1;
                    }
                    let mut b = 0;
                    while b < o.len() - a
                        && b < t.len() - a
                        && o[o.len() - 1 - b] == t[t.len() - 1 - b]
                    {
                        b += 1;
                    }
                    while b > 0
                        && !(old.is_char_boundary(o.len() - b)
                            && text.is_char_boundary(t.len() - b))
                    {
                        b -= 1;
                    }
                    let at = |i: usize| {
                        let p = position(&old, i);
                        json!({"line": p.line + 1, "offset": p.character + 1})
                    };
                    let change = json!({"start": at(a), "end": at(o.len() - b), "newText": &text[a..t.len() - b]});
                    rpc.notify(
                        "updateOpen",
                        json!({"changedFiles": [{"fileName": f, "textChanges": [change]}]}),
                    );
                }
                shown.insert(file.to_path_buf(), text.to_owned());
            }
        }
    }

    fn is_open(&self, file: &Path) -> bool {
        match self {
            Server::Lsp { versions, .. } => versions.contains_key(&uri(file)),
            Server::TsServer { open, .. } => open.contains(file),
        }
    }

    fn changed_unopened(&self, file: &Path) {
        match self {
            Server::Lsp { rpc, .. } => {
                let kind = if file.exists() { 2 } else { 3 };
                rpc.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": uri(file), "type": kind}]}),
                );
            }
            Server::TsServer { rpc, root, .. } => {
                // a file the server reads from disk: opening and closing it makes it read it again
                if file.exists() {
                    let f = file.to_string_lossy();
                    rpc.notify("open", json!({"file": f, "projectRootPath": root}));
                    rpc.notify("close", json!({"file": f}));
                }
            }
        }
    }

    fn locations(&self, method: &str, file: &Path, pos: Pos) -> Later<Vec<Loc>> {
        match self {
            Server::Lsp { rpc, .. } => {
                let p = rpc.request(
                    &format!("textDocument/{method}"),
                    json!({"textDocument": {"uri": uri(file)}, "position": pos.json()}),
                );
                Box::new(move || {
                    Ok(as_array(&p.wait()?)
                        .iter()
                        .filter_map(|l| {
                            let u = l.get("targetUri").or_else(|| l.get("uri"))?.as_str()?;
                            let rg = l
                                .get("targetSelectionRange")
                                .or_else(|| l.get("targetRange"))
                                .or_else(|| l.get("range"))?;
                            Some(Loc {
                                uri: u.to_owned(),
                                pos: Pos::from_json(&rg["start"])?,
                            })
                        })
                        .collect())
                })
            }
            Server::TsServer { rpc, .. } => {
                let p = rpc.request(
                    method,
                    json!({"file": file, "line": pos.line + 1, "offset": pos.character + 1}),
                );
                Box::new(move || {
                    Ok(as_array(&p.wait()?)
                        .iter()
                        .filter_map(|s| {
                            Some(Loc {
                                uri: file_uri(s["file"].as_str()?),
                                pos: ts_pos(&s["start"]),
                            })
                        })
                        .collect())
                })
            }
        }
    }

    fn type_definition(&self, file: &Path, pos: Pos) -> Later<Vec<Loc>> {
        self.locations("typeDefinition", file, pos)
    }

    fn definition(&self, file: &Path, pos: Pos) -> Later<Vec<Loc>> {
        self.locations("definition", file, pos)
    }

    fn lsp_diagnostics(rpc: &Rpc, file: &Path) -> Pending {
        rpc.request(
            "textDocument/diagnostic",
            json!({"textDocument": {"uri": uri(file)}}),
        )
    }

    /// The positions of the file's syntax errors (TypeScript's diagnostics 1000–1999).
    fn syntax_errors(&self, file: &Path) -> Result<Vec<Pos>, String> {
        match self {
            Server::Lsp { rpc, .. } => {
                let r = Self::lsp_diagnostics(rpc, file).wait()?;
                Ok(as_array(&r["items"])
                    .iter()
                    .filter(|d| {
                        d["code"]
                            .as_i64()
                            .is_some_and(|c| (1000..2000).contains(&c))
                    })
                    .filter_map(|d| Pos::from_json(&d["range"]["start"]))
                    .collect())
            }
            Server::TsServer { rpc, .. } => {
                let r = rpc
                    .request("syntacticDiagnosticsSync", json!({"file": file}))
                    .wait()?;
                Ok(as_array(&r).iter().map(|d| ts_pos(&d["start"])).collect())
            }
        }
    }

    fn inlay_hints(&self, file: &Path, start: Pos, end: Pos) -> Result<Vec<Hint>, String> {
        match self {
            Server::Lsp { rpc, .. } => {
                let r = rpc
                    .request(
                        "textDocument/inlayHint",
                        json!({"textDocument": {"uri": uri(file)}, "range": {"start": start.json(), "end": end.json()}}),
                    )
                    .wait()?;
                Ok(as_array(&r)
                    .iter()
                    .filter_map(|h| {
                        let pos = Pos::from_json(&h["position"])?;
                        let parts = match &h["label"] {
                            Value::Array(parts) => parts
                                .iter()
                                .map(|p| {
                                    let loc = p.get("location").and_then(|l| {
                                        Some(Loc {
                                            uri: l["uri"].as_str()?.to_owned(),
                                            pos: Pos::from_json(&l["range"]["start"])?,
                                        })
                                    });
                                    (p["value"].as_str().unwrap_or("").to_owned(), loc)
                                })
                                .collect(),
                            l => vec![(l.as_str().unwrap_or("").to_owned(), None)],
                        };
                        Some(Hint { pos, parts })
                    })
                    .collect())
            }
            Server::TsServer { rpc, shown, .. } => {
                let text = shown.get(file).cloned().unwrap_or_default();
                let a = utf16_index(&text, index_of(&text, start));
                let b = utf16_index(&text, index_of(&text, end));
                let r = rpc
                    .request(
                        "provideInlayHints",
                        json!({"file": file, "start": a, "length": b.saturating_sub(a)}),
                    )
                    .wait()?;
                Ok(as_array(&r)
                    .iter()
                    .map(|h| {
                        let parts = match h.get("displayParts").and_then(Value::as_array) {
                            Some(parts) => parts
                                .iter()
                                .map(|p| {
                                    let loc = p.get("span").and_then(|s| {
                                        Some(Loc {
                                            uri: file_uri(s["file"].as_str()?),
                                            pos: ts_pos(&s["start"]),
                                        })
                                    });
                                    (p["text"].as_str().unwrap_or("").to_owned(), loc)
                                })
                                .collect(),
                            None => vec![(h["text"].as_str().unwrap_or("").to_owned(), None)],
                        };
                        Hint {
                            pos: ts_pos(&h["position"]),
                            parts,
                        }
                    })
                    .collect())
            }
        }
    }

    fn deprecated_ranges(&self, file: &Path) -> Result<Vec<(Pos, Pos)>, String> {
        match self {
            Server::Lsp { rpc, .. } => {
                let r = Self::lsp_diagnostics(rpc, file).wait()?;
                Ok(as_array(&r["items"])
                    .iter()
                    .filter(|d| {
                        let c = d["code"]
                            .as_i64()
                            .or_else(|| d["code"].as_str().and_then(|s| s.parse().ok()));
                        c.is_some_and(|c| DEPRECATED_CODES.contains(&c))
                    })
                    .filter_map(|d| {
                        Some((
                            Pos::from_json(&d["range"]["start"])?,
                            Pos::from_json(&d["range"]["end"])?,
                        ))
                    })
                    .collect())
            }
            Server::TsServer { rpc, .. } => {
                let r = rpc
                    .request("suggestionDiagnosticsSync", json!({"file": file}))
                    .wait()?;
                Ok(as_array(&r)
                    .iter()
                    .filter(|d| {
                        d["code"]
                            .as_i64()
                            .is_some_and(|c| DEPRECATED_CODES.contains(&c))
                    })
                    .map(|d| (ts_pos(&d["start"]), ts_pos(&d["end"])))
                    .collect())
            }
        }
    }

    fn deprecated_at(&self, file: &Path, pos: Pos) -> Result<bool, String> {
        match self {
            Server::Lsp { rpc, .. } => {
                let r = rpc
                    .request(
                        "textDocument/hover",
                        json!({"textDocument": {"uri": uri(file)}, "position": pos.json()}),
                    )
                    .wait()?;
                let c = &r["contents"];
                let text = match c {
                    Value::String(s) => s.clone(),
                    Value::Array(a) => a
                        .iter()
                        .map(|x| x.as_str().or_else(|| x["value"].as_str()).unwrap_or(""))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => c["value"].as_str().unwrap_or("").to_owned(),
                };
                Ok(text.starts_with("*@deprecated*") || text.contains("\n*@deprecated*"))
            }
            Server::TsServer { rpc, .. } => {
                let r = rpc
                    .request(
                        "quickinfo",
                        json!({"file": file, "line": pos.line + 1, "offset": pos.character + 1}),
                    )
                    .wait()?;
                Ok(r["kindModifiers"]
                    .as_str()
                    .unwrap_or("")
                    .split(',')
                    .any(|m| m == "deprecated"))
            }
        }
    }

    /// The errors and warnings of what the server has for `file`.
    fn diagnostics(&self, file: &Path, rel: &str) -> Result<Vec<Diagnostic>, String> {
        match self {
            Server::Lsp { rpc, .. } => {
                let r = Self::lsp_diagnostics(rpc, file).wait()?;
                Ok(as_array(&r["items"])
                    .iter()
                    .filter(|d| d["severity"].as_u64().is_none_or(|s| s <= 2))
                    .map(|d| Diagnostic::from_lsp(rel, d))
                    .collect())
            }
            Server::TsServer { rpc, .. } => {
                let syn = rpc.request("syntacticDiagnosticsSync", json!({"file": file}));
                let sem = rpc.request("semanticDiagnosticsSync", json!({"file": file}));
                let mut out = Vec::new();
                for d in as_array(&syn.wait()?)
                    .iter()
                    .chain(as_array(&sem.wait()?).iter())
                {
                    let p = ts_pos(&d["start"]);
                    let severity = match d["category"].as_str() {
                        Some("warning") => "warning",
                        Some("suggestion" | "message") => continue,
                        _ => "error",
                    };
                    out.push(Diagnostic {
                        path: rel.to_owned(),
                        line: p.line + 1,
                        column: p.character + 1,
                        severity: severity.to_owned(),
                        code: d["code"].as_i64().map(|c| c.to_string()),
                        message: d["text"]
                            .as_str()
                            .unwrap_or("")
                            .split('\n')
                            .collect::<Vec<_>>()
                            .join(" "),
                    });
                }
                Ok(out)
            }
        }
    }
}

pub(crate) struct TypeScript {
    root: PathBuf,
    server: Server,
    texts: HashMap<PathBuf, String>,
    cache: HashMap<Loc, Vec<Decl>>,
    pub name: String,
}

type Item = (usize, String, usize, usize);

impl TypeScript {
    pub fn start(
        root: &Path,
        cfg: &TypeScriptConfig,
        log: &Option<PathBuf>,
    ) -> Result<TypeScript, String> {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let ts = project_typescript(&root)?;
        let (server, name) = Server::start(&root, &ts, cfg, log)?;
        Ok(TypeScript {
            root,
            server,
            texts: HashMap::new(),
            cache: HashMap::new(),
            name,
        })
    }

    pub fn alive(&self) -> bool {
        self.server.rpc().alive()
    }

    pub fn stop(&mut self) {
        self.server.rpc().stop();
    }

    fn sync(&mut self, file: &Path) -> Result<String, String> {
        let text = read_text(file).ok_or_else(|| format!("cannot read {}", file.display()))?;
        if self.texts.get(file) != Some(&text) {
            self.server.show(file, &text);
            self.texts.insert(file.to_path_buf(), text.clone());
        }
        Ok(text)
    }

    pub fn changed(&mut self, file: &Path) {
        if self.server.is_open(file) {
            if file.exists() {
                let _ = self.sync(file);
            }
            return;
        }
        self.server.changed_unopened(file);
    }

    /// One tracked source file of each tsconfig project, so the server loads them early.
    pub fn preload_files(&self) -> Vec<PathBuf> {
        let Ok(out) = Command::new("git")
            .args([
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .current_dir(&self.root)
            .output()
        else {
            return Vec::new();
        };
        let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|f| !f.is_empty())
            .map(str::to_owned)
            .collect();
        let is_source = |f: &str| {
            [".ts", ".tsx", ".mts", ".cts"]
                .iter()
                .any(|e| f.ends_with(e))
                && ![".d.ts", ".d.mts", ".d.cts"].iter().any(|e| f.ends_with(e))
                && !f.contains("node_modules/")
        };
        let sources: Vec<&String> = files.iter().filter(|f| is_source(f)).collect();
        let mut picked: Vec<PathBuf> = Vec::new();
        for t in files.iter().filter(|f| {
            (f.as_str() == "tsconfig.json" || f.ends_with("/tsconfig.json"))
                && !f.contains("node_modules/")
        }) {
            let dir = t.strip_suffix("tsconfig.json").unwrap_or("");
            if let Some(s) = sources.iter().find(|s| s.starts_with(dir)) {
                let p = self.root.join(s.as_str());
                if !picked.contains(&p) {
                    picked.push(p);
                }
            }
        }
        picked
    }

    pub fn warm(&mut self, file: &Path) {
        if self.sync(file).is_ok() {
            let _ = self.server.type_definition(file, Pos::default())();
        }
    }

    pub fn answer(&mut self, matches: &[Match]) -> Result<Vec<Verdict>, String> {
        let mut by_file: Vec<(String, [Vec<Item>; 3])> = Vec::new();
        for (n, m) in matches.iter().enumerate() {
            for (var, cond) in &m.conditions {
                let Some(b) = m.bindings.get(var) else {
                    continue;
                };
                let i = match by_file.iter().position(|f| f.0 == m.file) {
                    Some(i) => i,
                    None => {
                        by_file.push((m.file.clone(), Default::default()));
                        by_file.len() - 1
                    }
                };
                let k = match cond.asks() {
                    Ask::Type => 0,
                    Ask::Declared => 1,
                    Ask::Deprecated => 2,
                };
                by_file[i].1[k].push((n, var.clone(), b.start, b.end));
            }
        }
        let mut found = HashMap::new();
        for (f, [types, declared, deprecated]) in by_file {
            let file = self.root.join(&f);
            if !types.is_empty() || !declared.is_empty() {
                found.extend(self.check(&file, &types, &declared)?);
            }
            if !deprecated.is_empty() {
                found.extend(self.deprecated(&file, &deprecated)?);
            }
        }
        Ok(verdicts(matches, &found, '/'))
    }

    /// [package, module] of a declaration file (spec §15.1 rule 5).
    fn declared_in(&self, file: &Path) -> (Option<String>, Option<String>) {
        let p = file.to_string_lossy().replace('\\', "/");
        let strip = |s: &str| -> String {
            for ext in [
                ".d.ts", ".d.mts", ".d.cts", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs",
                ".cjs",
            ] {
                if let Some(x) = s.strip_suffix(ext) {
                    return x.to_owned();
                }
            }
            s.to_owned()
        };
        let Some(i) = p.rfind("/node_modules/") else {
            let real = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
            return match real.strip_prefix(&self.root) {
                Ok(r) => (None, Some(strip(&r.to_string_lossy()))),
                Err(_) => (None, None),
            };
        };
        let rest = &p[i + "/node_modules/".len()..];
        let segs: Vec<&str> = rest.split('/').collect();
        // TypeScript's own lib files (shipped in typescript or its native @typescript/* package)
        let lib_pkg = segs[0] == "typescript" || (segs[0] == "@typescript" && segs.len() > 1);
        if lib_pkg {
            let off = if segs[0] == "typescript" { 1 } else { 2 };
            if segs.len() == off + 2
                && segs[off] == "lib"
                && segs[off + 1].starts_with("lib.")
                && segs[off + 1].ends_with(".d.ts")
            {
                let name = segs[off + 1].strip_suffix(".d.ts").unwrap_or(segs[off + 1]);
                return (
                    Some("typescript".into()),
                    Some(format!("typescript/lib/{name}")),
                );
            }
        }
        let mut pkg = if segs[0].starts_with('@') && segs.len() > 1 {
            format!("{}/{}", segs[0], segs[1])
        } else {
            segs[0].to_owned()
        };
        let rest = segs[pkg.split('/').count().min(segs.len())..].join("/");
        if let Some(t) = pkg.strip_prefix("@types/") {
            pkg = match t.split_once("__") {
                Some((a, b)) => format!("@{a}/{b}"),
                None => t.to_owned(),
            };
        }
        let module = format!("{pkg}/{}", strip(&rest));
        (Some(pkg), Some(module))
    }

    /// The declarations of the type declared at `loc` and of its supertypes, transitively.
    fn hierarchy(&mut self, loc: &Loc) -> Result<Vec<Decl>, String> {
        if let Some(c) = self.cache.get(loc) {
            return Ok(c.clone());
        }
        let mut out: Vec<Decl> = Vec::new();
        let mut seen = HashSet::new();
        let mut todo = vec![loc.clone()];
        while let Some(l) = todo.pop() {
            if seen.len() >= 64 {
                break;
            }
            if !seen.insert(l.clone()) {
                continue;
            }
            let file = path_of(&l.uri);
            let (package, module) = self.declared_in(&file);
            let Some(text) = read_text(&file) else {
                continue;
            };
            let Some(at) = name_at(&text, index_of(&text, l.pos)) else {
                continue;
            };
            let d = Decl {
                package,
                module,
                name: Some(at.name.clone()),
            };
            if !out.contains(&d) {
                out.push(d);
            }
            let bases = match at.end {
                None => Vec::new(),
                Some(e) if at.alias => alias_members(&text, e),
                Some(e) => heritage(&text, e),
            };
            let defs: Vec<_> = bases
                .iter()
                .map(|b| self.server.definition(&file, position(&text, *b)))
                .collect();
            for d in defs {
                todo.extend(d()?);
            }
        }
        let inside = std::fs::canonicalize(path_of(&loc.uri))
            .map(|f| f.starts_with(&self.root))
            .unwrap_or(false);
        if !inside || loc.uri.contains("/node_modules/") {
            self.cache.insert(loc.clone(), out.clone());
        }
        Ok(out)
    }

    fn check(
        &mut self,
        file: &Path,
        wanted: &[Item],
        named: &[Item],
    ) -> Result<HashMap<(usize, String), Found>, String> {
        let text = read_text(file).ok_or_else(|| format!("cannot read {}", file.display()))?;
        if wanted.is_empty() {
            self.sync(file)?;
            return self.declared(file, &text, named, &text, &[]);
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
        let (shown, results) = loop {
            let shown = bindings(&text, &ranges, |n| !unchecked.contains(&n));
            // the server keeps this version until the file changes: the wrapping binds no name, so
            // what other files see of this one is unchanged
            self.server.show(file, &shown.overlay);
            self.texts.insert(file.to_path_buf(), shown.overlay.clone());
            let overlay = &shown.overlay;
            let waits: Vec<Option<Later<Vec<Loc>>>> = (0..ranges.len())
                .map(|n| {
                    shown.anchors[n]
                        .map(|(s, _)| self.server.type_definition(file, position(overlay, s + 1)))
                })
                .collect();
            let mut results: Vec<Vec<Loc>> = Vec::new();
            for w in waits {
                results.push(match w {
                    Some(w) => w()?,
                    None => Vec::new(),
                });
            }
            let mut unknown = Vec::new();
            for (n, r) in results.iter_mut().enumerate() {
                if !r.is_empty() || unchecked.contains(&n) {
                    continue;
                }
                if let Some(a) = shown.anchors[n] {
                    *r = self.type_of_intersection(file, overlay, a)?;
                }
                if r.is_empty() {
                    unknown.push(n);
                }
            }
            // a binding the parser rejects: a syntax error in the code it inserts
            let errors: Vec<usize> = if unknown.is_empty() {
                Vec::new()
            } else {
                self.server
                    .syntax_errors(file)?
                    .into_iter()
                    .map(|p| index_of(overlay, p))
                    .collect()
            };
            let inserted = |n: usize| -> [(usize, usize); 2] {
                let (s, e) = shown.anchors[n].expect("active");
                let r = shown.refs[n].expect("active");
                [
                    (s - 2, e + 4),
                    (r - 6, r + format!("__forge_t{n})()").len()),
                ]
            };
            let unbound: Vec<usize> = unknown
                .into_iter()
                .filter(|&n| {
                    errors
                        .iter()
                        .any(|&i| inserted(n).iter().any(|&(a, b)| a <= i && i < b))
                })
                .collect();
            if unbound.is_empty() {
                break (shown, results);
            }
            let first = unbound
                .into_iter()
                .min_by_key(|&n| ranges[n].0)
                .expect("one");
            unchecked.insert(first);
        };
        let mut per_range = Vec::new();
        for (n, r) in results.iter().enumerate() {
            if unchecked.contains(&n) {
                per_range.push(Found::Unchecked);
                continue;
            }
            let mut decls: Vec<Decl> = Vec::new();
            for l in r {
                for d in self.hierarchy(l)? {
                    if !decls.contains(&d) {
                        decls.push(d);
                    }
                }
            }
            let unknown = decls.is_empty();
            per_range.push(Found::Decls { decls, unknown });
        }
        let mut found = self.declared(file, &text, named, &shown.overlay, &shown.events)?;
        for (i, w) in wanted.iter().enumerate() {
            found.insert((w.0, w.1.clone()), per_range[slot[i]].clone());
        }
        Ok(found)
    }

    /// The declarations of the top-level members of the type of the parameter at `start..end` of
    /// the overlay, from its inlay hint (the type as the checker prints it, linked).
    fn type_of_intersection(
        &self,
        file: &Path,
        overlay: &str,
        (start, end): (usize, usize),
    ) -> Result<Vec<Loc>, String> {
        let at = position(overlay, end);
        let hints = self
            .server
            .inlay_hints(file, position(overlay, start), at)?;
        let Some(hint) = hints.into_iter().find(|h| h.pos == at) else {
            return Ok(Vec::new());
        };
        let mut text = String::new();
        let mut named: HashMap<usize, Loc> = HashMap::new();
        for (t, loc) in hint.parts {
            if let Some(l) = loc {
                named.insert(text.len(), l);
            }
            text.push_str(&t);
        }
        let from = text.find(':').map_or(0, |i| i + 1);
        Ok(top_level_names(&text, from, text.len(), &HashSet::new())
            .into_iter()
            .filter_map(|i| named.get(&i).cloned())
            .collect())
    }

    /// The declarations names refer to (spec §15.6), asked on the version the server has (`shown`:
    /// the text with `events` inserted).
    fn declared(
        &mut self,
        file: &Path,
        text: &str,
        named: &[Item],
        shown: &str,
        events: &[(usize, String)],
    ) -> Result<HashMap<(usize, String), Found>, String> {
        let mut found = HashMap::new();
        let mut asked: Vec<(usize, bool, Later<Vec<Loc>>)> = Vec::new();
        let mut at_of = Vec::new();
        for (n, v, s, e) in named {
            let expr = text.get(*s..*e).unwrap_or("");
            let name = last_word(expr, true)
                .filter(|(i, _)| *i == 0 || expr[..*i].trim_end().ends_with('.'));
            let Some((i, w)) = name else {
                found.insert(
                    (*n, v.clone()),
                    Found::Decls {
                        decls: vec![],
                        unknown: true,
                    },
                );
                continue;
            };
            let at = s + i;
            // where the name is in the version shown: past every insert at or before it
            let shift: usize = events
                .iter()
                .filter(|(off, _)| *off <= at)
                .map(|(_, p)| p.len())
                .sum();
            if !asked.iter().any(|a| a.0 == at) {
                // a called name: the servers add the declaration of the signature it is called
                // with after the name's own declarations: what it is called with, left out
                let after = skip_trivia(shown, at + shift + w.len());
                let called = shown
                    .as_bytes()
                    .get(after)
                    .is_some_and(|c| b"(<`".contains(c));
                asked.push((
                    at,
                    called,
                    self.server.definition(file, position(shown, at + shift)),
                ));
            }
            at_of.push(((*n, v.clone()), at));
        }
        let mut answers: HashMap<usize, Found> = HashMap::new();
        for (at, called, later) in asked {
            let mut locs = later()?;
            if called && locs.len() > 1 {
                locs.pop();
            }
            let mut decls: Vec<Decl> = Vec::new();
            for l in locs {
                let f = path_of(&l.uri);
                let body = if same_file(&f, file) {
                    Some(shown.to_owned())
                } else {
                    read_text(&f)
                };
                let Some(body) = body else { continue };
                let i = index_of(&body, l.pos);
                if in_import(&body, i) {
                    continue; // the server stopped at an import it could not resolve
                }
                let (package, module) = self.declared_in(&f);
                let d = Decl {
                    package,
                    module,
                    name: name_at(&body, i).map(|a| a.name),
                };
                if !decls.contains(&d) {
                    decls.push(d);
                }
            }
            let unknown = decls.is_empty();
            answers.insert(at, Found::Decls { decls, unknown });
        }
        for (k, at) in at_of {
            found.insert(k, answers[&at].clone());
        }
        Ok(found)
    }

    /// Whether each name is a use of a deprecated declaration (spec §15.7).
    fn deprecated(
        &mut self,
        file: &Path,
        items: &[Item],
    ) -> Result<HashMap<(usize, String), Found>, String> {
        let text = self.sync(file)?;
        let ranges: Vec<(usize, usize)> = self
            .server
            .deprecated_ranges(file)?
            .into_iter()
            .map(|(a, b)| (index_of(&text, a), index_of(&text, b)))
            .collect();
        let mut found = HashMap::new();
        for (n, v, s, e) in items {
            let (a, b) = (*s, *e);
            let mut deprecated = ranges.iter().any(|&(rs, re)| rs < b && a < re);
            if !deprecated
                && key_position(&text, a, b)
                && self.server.deprecated_at(file, position(&text, a))?
            {
                // a key that declares the property (an interface or type member) is not a use of it
                let defs = self.server.definition(file, position(&text, a))()?;
                deprecated = !defs
                    .iter()
                    .any(|l| same_file(&path_of(&l.uri), file) && index_of(&text, l.pos) == a);
            }
            found.insert((*n, v.clone()), Found::Deprecated(deprecated));
        }
        Ok(found)
    }

    pub fn diagnose(
        &mut self,
        files: &[DiagFile],
    ) -> Result<Vec<crate::diag::FileDiagnostics>, String> {
        let mut out = Vec::new();
        for f in files {
            let path = self.root.join(&f.path);
            let Some(now) = read_text(&path) else {
                out.push((vec![], vec![]));
                continue;
            };
            let base = match f.base.as_ref().and_then(|b| read_text(b)) {
                Some(b) => {
                    self.server.show(&path, &b);
                    self.server.diagnostics(&path, &f.path)?
                }
                None => vec![],
            };
            self.server.show(&path, &now);
            self.texts.insert(path.clone(), now);
            out.push((self.server.diagnostics(&path, &f.path)?, base));
        }
        Ok(out)
    }
}
