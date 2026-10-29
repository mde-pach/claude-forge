//! The types engine as the checker's [`TypeOracle`]: forge-engine's daemon, asked over its socket.
//!
//! `forge check` and Stop start the daemon when none answers ([`start`]): its configuration is
//! the pinned ty (installed first when the run may download), the project's Python environment
//! and the installed packages without typing (forge-helpers' `untyped`), and the project's own
//! TypeScript. After an edit, only a daemon already running is asked ([`running`]): starting one
//! takes seconds.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use ast_grep_language::SupportLang;
use forge_core::{Note, NoteKind};
use forge_engine::{
    Binding, Client, Condition, EngineConfig, Lang, Match, PythonConfig, TypeScriptConfig,
};
use forge_packs::model::{Language, Rule, WhereCondition};

use crate::astgrep::{AstMatch, Compiled, TypeOracle, target_of};
use crate::checker::ActiveRule;
use crate::tools::{ToolResolver, is_exe};

/// The Python the untyped-packages probe assumes when the environment does not say.
const DEFAULT_PY: &str = "3.12";

/// forge-engine's daemon, as the checker asks it.
pub struct EngineOracle {
    client: Client,
    /// After an edit: an engine still loading is silent (Stop asks again).
    edit: bool,
}

fn lang_of(lang: SupportLang) -> Option<Lang> {
    match lang {
        SupportLang::Python => Some(Lang::Python),
        SupportLang::Tsx => Some(Lang::Typescript),
        _ => None,
    }
}

/// The engine's conditions of a rule's `where`.
fn conditions(rule: &Compiled) -> BTreeMap<String, Condition> {
    rule.target
        .where_
        .iter()
        .flatten()
        .filter_map(|(var, c)| {
            let c: Condition = serde_json::to_value(c)
                .ok()
                .and_then(|v| serde_json::from_value(v).ok())?;
            Some((var.trim_start_matches('$').to_string(), c))
        })
        .collect()
}

/// The engine's question for one ast-grep match.
pub fn engine_match(m: &AstMatch, conditions: &BTreeMap<String, Condition>) -> Match {
    Match {
        file: m.path.clone(),
        rule: m.rule.clone(),
        line: m.line,
        bindings: m
            .vars
            .iter()
            .map(|(k, (text, start))| {
                (
                    k.clone(),
                    Binding {
                        start: *start,
                        end: start + text.len(),
                        text: text.clone(),
                    },
                )
            })
            .collect(),
        conditions: conditions.clone(),
    }
}

impl TypeOracle for EngineOracle {
    fn confirm(
        &self,
        _root: &Path,
        rule: &Compiled,
        matches: &[AstMatch],
    ) -> (Vec<bool>, Vec<Note>) {
        let Some(lang) = lang_of(rule.lang) else {
            return (
                vec![false; matches.len()],
                vec![
                    Note::new(
                        NoteKind::NotEnforced,
                        "not checked: no types engine for its language",
                    )
                    .for_rule(rule.id()),
                ],
            );
        };
        let conds = conditions(rule);
        let ms: Vec<Match> = matches.iter().map(|m| engine_match(m, &conds)).collect();
        let answer = self.client.types(lang, &ms);
        forge_engine::judge(lang, &ms, &answer, self.edit)
    }
}

/// The socket of the repository's daemon.
pub fn socket(root: &Path) -> Option<PathBuf> {
    let home = forge_home::Home::from_env().ok()?;
    let id = forge_home::RepoId::for_root(root).ok()?;
    Some(home.engine_socket(&id))
}

/// The daemon already running for `root`, for an edit-time check; `None` when none answers.
pub fn running(root: &Path) -> Option<EngineOracle> {
    let client = Client::new(&socket(root)?).with_timeout(Duration::from_millis(300));
    client
        .clone()
        .with_timeout(Duration::from_millis(100))
        .ping()
        .ok()?;
    Some(EngineOracle { client, edit: true })
}

/// The languages whose active rules have `where` conditions.
pub fn typed_languages(rules: &[ActiveRule]) -> BTreeSet<Lang> {
    rules
        .iter()
        .filter(|a| target_of_rule(&a.rule).is_some_and(|t| t.where_.is_some()))
        .filter_map(|a| match a.rule.scope.language {
            Language::Python => Some(Lang::Python),
            Language::Typescript | Language::Tsx => Some(Lang::Typescript),
            _ => None,
        })
        .collect()
}

fn target_of_rule(rule: &Rule) -> Option<&forge_packs::model::AstGrepTarget> {
    target_of(rule)
}

/// The packages the active rules' `where` conditions name (their types stay known to ty).
fn asked_packages(rules: &[ActiveRule]) -> BTreeSet<String> {
    fn walk(c: &WhereCondition, out: &mut BTreeSet<String>) {
        match c {
            WhereCondition::Type(s) | WhereCondition::Declared(s) => {
                for spec in s.as_slice() {
                    for p in spec.package.iter().flat_map(|p| p.as_slice()) {
                        out.insert(p.replace('-', "_"));
                    }
                }
            }
            WhereCondition::Not(inner) => walk(inner, out),
            WhereCondition::Deprecated(_) => {}
        }
    }
    let mut out = BTreeSet::new();
    for a in rules {
        if let Some(w) = target_of_rule(&a.rule).and_then(|t| t.where_.as_ref()) {
            for c in w.values() {
                walk(c, &mut out);
            }
        }
    }
    out
}

/// The project's virtual environment: its interpreter, prefix and site-packages directories.
fn python_env(root: &Path) -> Option<(PathBuf, PathBuf, Vec<PathBuf>, String)> {
    let prefix = std::env::var_os("VIRTUAL_ENV")
        .map(PathBuf::from)
        .filter(|p| p.join("bin/python").exists())
        .unwrap_or_else(|| root.join(".venv"));
    let python = prefix.join("bin/python");
    if !python.exists() {
        return None;
    }
    let mut sites = Vec::new();
    let mut version = DEFAULT_PY.to_string();
    if let Ok(rd) = std::fs::read_dir(prefix.join("lib")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(v) = name.strip_prefix("python") {
                let site = e.path().join("site-packages");
                if site.is_dir() {
                    sites.push(site);
                    version = v.to_string();
                }
            }
        }
    }
    Some((python, prefix, sites, version))
}

/// The engine's configuration for the active rules of `root`, and what it could not set up
/// (notes). `None` when no language with typed rules can have an engine here.
pub fn config(
    root: &Path,
    rules: &[ActiveRule],
    tools: &dyn ToolResolver,
    download: bool,
    forge_version: &str,
) -> (Option<EngineConfig>, Vec<Note>) {
    let mut notes = Vec::new();
    let langs = typed_languages(rules);
    let Some(socket) = socket(root) else {
        return (None, notes);
    };
    let mut cfg = EngineConfig::new(root, socket);
    if langs.contains(&Lang::Python) {
        let pin = crate::adapters::Adapter::load("ty").and_then(|a| a.pinned);
        let ty = pin.as_deref().map(|v| {
            if download {
                tools.ensure("ty", v)
            } else {
                tools.pinned("ty", v)
            }
        });
        match ty {
            Some(Ok(cmd)) => {
                let mut py = PythonConfig {
                    ty: cmd.argv.clone(),
                    ..PythonConfig::default()
                };
                if let Some((python, prefix, sites, version)) = python_env(root) {
                    py.python = Some(python);
                    py.prefix = Some(prefix);
                    // the probe runs ty itself: only a ty that is one executable
                    if cmd.argv.len() == 1 && is_exe(&cmd.bin) {
                        match forge_helpers::untyped::untyped_modules(
                            root, &version, &sites, &cmd.bin,
                        ) {
                            Ok(u) => {
                                let asked = asked_packages(rules);
                                py.untyped = u
                                    .modules
                                    .iter()
                                    .filter(|m| {
                                        !asked.contains(m.split('.').next().unwrap_or(m))
                                    })
                                    .cloned()
                                    .collect();
                                py.untyped_all = Some(u.modules);
                            }
                            Err(e) => notes.push(Note::new(
                                NoteKind::ForgeFailure,
                                format!(
                                    "the packages without typing could not be listed ({e}); ty checks them as it infers them"
                                ),
                            )),
                        }
                    }
                }
                cfg.python = Some(py);
            }
            Some(Err(why)) => notes.push(Note::new(
                NoteKind::MissingTool,
                format!("the Python types engine needs ty: {why}; the rules that ask for types are not checked"),
            )),
            None => {}
        }
    }
    if langs.contains(&Lang::Typescript) {
        if root.join("node_modules/typescript/package.json").is_file() {
            cfg.typescript = Some(TypeScriptConfig {
                js_runtime: tools.on_path("node").or_else(|| tools.on_path("bun")),
                preload: true,
            });
        } else {
            notes.push(Note::new(
                NoteKind::MissingTool,
                "the TypeScript types engine needs the project's TypeScript installed (node_modules/typescript); the rules that ask for types are not checked",
            ));
        }
    }
    if cfg.python.is_none() && cfg.typescript.is_none() {
        return (None, notes);
    }
    let want = serde_json::json!({
        "forge": forge_version,
        "python": cfg.python,
        "typescript": cfg.typescript,
    });
    cfg.want = forge_engine_want(&want.to_string());
    if let Ok(home) = forge_home::Home::from_env()
        && let Ok(id) = forge_home::RepoId::for_root(root)
    {
        cfg.log = Some(home.repo_state_dir(&id).join("engine.log"));
    }
    (Some(cfg), notes)
}

fn forge_engine_want(text: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Where the configuration of `root`'s engine is written for `forge engine serve --config`.
fn config_file(root: &Path) -> Option<PathBuf> {
    let home = forge_home::Home::from_env().ok()?;
    let id = forge_home::RepoId::for_root(root).ok()?;
    Some(home.repo_state_dir(&id).join("engine.json"))
}

/// The engine for a check that may wait for it (`forge check`, Stop) or for a session (the
/// detached start after SessionStart, which registers `session`): the daemon running with this
/// configuration, or one started now (`<exe> engine serve --config <file>`). `None` and notes
/// when it cannot be had: the typed rules are then not checked.
pub fn start(
    root: &Path,
    rules: &[ActiveRule],
    tools: &dyn ToolResolver,
    download: bool,
    exe: &Path,
    forge_version: &str,
    session: Option<&str>,
) -> (Option<EngineOracle>, Vec<Note>) {
    if typed_languages(rules).is_empty() {
        return (None, Vec::new());
    }
    let (cfg, mut notes) = config(root, rules, tools, download, forge_version);
    let Some(cfg) = cfg else {
        return (None, notes);
    };
    if let Some(log) = &cfg.log
        && let Some(dir) = log.parent()
    {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Some(dir) = forge_engine::socket_addr(&cfg.socket).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Some(file) = config_file(root) else {
        return (None, notes);
    };
    let written = file
        .parent()
        .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
        && serde_json::to_vec(&cfg)
            .ok()
            .is_some_and(|b| write_atomic(&file, &b));
    if !written {
        notes.push(Note::new(
            NoteKind::ForgeFailure,
            format!(
                "the types engine's configuration could not be written ({}); the rules that ask for types are not checked",
                file.display()
            ),
        ));
        return (None, notes);
    }
    let started = forge_engine::ensure(&cfg, session, Duration::from_secs(60), || {
        forge_engine::spawn_detached(
            Command::new(exe)
                .args(["engine", "serve", "--config"])
                .arg(&file)
                .current_dir(root),
        )
    });
    match started {
        Ok(client) => (
            Some(EngineOracle {
                client: client.with_timeout(Duration::from_secs(60)),
                edit: false,
            }),
            notes,
        ),
        Err(e) => {
            notes.push(Note::new(
                NoteKind::ForgeFailure,
                format!("the types engine did not start ({e}); the rules that ask for types are not checked"),
            ));
            (None, notes)
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> bool {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_ok()
}

/// `forge engine serve --config <file>`: runs the daemon with the configuration [`start`] wrote.
pub fn serve(file: &Path) -> Result<(), String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let cfg: EngineConfig = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    forge_engine::serve(cfg)
}
