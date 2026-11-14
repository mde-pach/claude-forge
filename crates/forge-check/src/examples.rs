//! A rule's examples through its ast-grep, grep and check targets (what `forge test` runs, and
//! the built-in packs' example tests):
//!
//! - ast-grep and grep: a `match` code example must match, a `no_match` one must not. A rule
//!   with a `where` (the types engine) proves only its `match` examples here; its `no_match`
//!   examples are skipped, with a note; [`typed`] runs them all through the types engine.
//! - check: each code example (a file at a path in the rule's scope) or fixture (`head`
//!   committed, then `files`) is a turn of a fresh git repository, checked with the rule alone
//!   (a format stage checks the whole tree): a `match` example must give the rule a finding, a
//!   `no_match` one none. A run whose notes say the rule did not run (a missing tool, not
//!   enforced here…) is a skip carrying that note.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use forge_core::examples::{ExampleKind, ExampleResult, ExampleVerdict};
use forge_core::{Note, NoteKind};
use forge_packs::PackSet;
use forge_packs::model::{Example, Language, Rule, Stage, Target};

use crate::scope::Roles;
use crate::tools::ToolResolver;
use crate::{ActiveRule, Checker, Mode, Options, astgrep, grep};

fn code_examples(rule: &Rule) -> Vec<(ExampleKind, usize, &str)> {
    rule.examples
        .as_ref()
        .map(|e| e.listed())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, n, e)| match e {
            Example::Code(c) => Some((k, n, c.as_str())),
            _ => None,
        })
        .collect()
}

/// The rule's ast-grep target on its code examples; none without one.
pub fn ast_grep(rule: &Rule, roles: &Roles) -> Vec<ExampleResult> {
    let Some(t) = astgrep::target_of(rule) else {
        return Vec::new();
    };
    let compiled = astgrep::compile(rule, t, roles, None);
    code_examples(rule)
        .into_iter()
        .map(|(kind, n, code)| {
            let verdict = match &compiled {
                Err(e) => ExampleVerdict::Fail {
                    why: format!("the ast-grep rule does not compile: {e}"),
                },
                Ok(_) if kind == ExampleKind::NoMatch && t.where_.is_some() => {
                    ExampleVerdict::Skip {
                        note: Note::new(
                            NoteKind::NotEnforced,
                            "its no_match examples need the types engine, which this run does not ask",
                        )
                        .for_rule(&rule.id),
                    }
                }
                Ok(c) => match (kind, astgrep::example_matches(c, code)) {
                    (ExampleKind::Match, false) => ExampleVerdict::Fail {
                        why: format!("does not match: {code:?}"),
                    },
                    (ExampleKind::NoMatch, true) => ExampleVerdict::Fail {
                        why: format!("matches: {code:?}"),
                    },
                    _ => ExampleVerdict::Pass,
                },
            };
            ExampleResult::new(&rule.id, "ast-grep", kind, n, verdict)
        })
        .collect()
}

/// The rule's grep target on its code examples; none without one.
pub fn grep(rule: &Rule, roles: &Roles) -> Vec<ExampleResult> {
    let Some(t) = grep::target_of(rule) else {
        return Vec::new();
    };
    let compiled = grep::compile(rule, t, roles, None);
    code_examples(rule)
        .into_iter()
        .map(|(kind, n, code)| {
            let verdict = match &compiled {
                Err(e) => ExampleVerdict::Fail {
                    why: format!("the grep pattern does not compile: {e}"),
                },
                Ok(g) => match (kind, g.count(code) > 0) {
                    (ExampleKind::Match, false) => ExampleVerdict::Fail {
                        why: format!("does not match: {code:?}"),
                    },
                    (ExampleKind::NoMatch, true) => ExampleVerdict::Fail {
                        why: format!("matches: {code:?}"),
                    },
                    _ => ExampleVerdict::Pass,
                },
            };
            ExampleResult::new(&rule.id, "grep", kind, n, verdict)
        })
        .collect()
}

/// A concrete path inside the rule's scope, for a code example.
pub fn example_path(rule: &Rule) -> String {
    let ext = match rule.scope.language {
        Language::Python => ".py",
        Language::Typescript => ".ts",
        Language::Tsx => ".tsx",
        Language::Yaml => ".yaml",
        Language::Any => ".txt",
    };
    let stem = "example";
    let globs = Roles::defaults().expand(rule.scope.files.as_deref().unwrap_or(&[]));
    let Some(g) = globs.iter().find(|g| !g.starts_with('!')) else {
        return format!("{stem}{ext}");
    };
    let re = |p: &str| regex::Regex::new(p).expect("example path regex");
    let mut p = re(r"\{([^,}]*)[^}]*\}").replace_all(g, "$1").into_owned();
    p = re(r"\.\*$").replace(&p, ext).into_owned();
    p = re(r"(^|/)\*\*/").replace_all(&p, "$1").into_owned();
    p = p.replace("**", stem).replace('*', stem).replace('?', "x");
    p = re(r"\[(!?)(.)[^\]]*\]")
        .replace_all(&p, |c: &regex::Captures| {
            if &c[1] == "!" {
                "x".to_string()
            } else {
                c[2].to_string()
            }
        })
        .into_owned();
    if p.ends_with('/') {
        p.push_str(stem);
    }
    let base = p.rsplit('/').next().unwrap_or(&p).to_string();
    if !re(r"\.[A-Za-z0-9]+$").is_match(&base) {
        p.push_str(ext);
    }
    p
}

fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=forge", "-c", "user.email=forge@localhost"])
        .args(["-c", "maintenance.auto=false", "-c", "commit.gpgsign=false"])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn write(dir: &Path, files: &BTreeMap<String, String>) -> Result<(), String> {
    for (p, t) in files {
        let f = dir.join(p);
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{p}: {e}"))?;
        }
        std::fs::write(&f, t).map_err(|e| format!("{p}: {e}"))?;
    }
    Ok(())
}

/// A fresh repository holding `head` (committed) and a turn snapshot of that head, then the
/// changed files.
struct Turn {
    repo: tempfile::TempDir,
    snap: tempfile::TempDir,
}

impl Turn {
    fn new(
        head: &BTreeMap<String, String>,
        changed: &BTreeMap<String, String>,
    ) -> Result<Turn, String> {
        let tmp = |what| tempfile::tempdir().map_err(|e| format!("{what}: {e}"));
        let (repo, snap) = (tmp("repository")?, tmp("snapshot")?);
        let root = repo.path();
        git(root, &["init", "-q", "-b", "main"])?;
        let mut head = head.clone();
        head.entry(".gitignore".into())
            .or_insert_with(|| "node_modules/\n.venv/\n".into());
        write(root, &head)?;
        git(root, &["add", "-A"])?;
        git(root, &["commit", "-qm", "head", "--allow-empty"])?;
        let rev = git(root, &["rev-parse", "HEAD"])?;
        std::fs::write(snap.path().join("head"), rev).map_err(|e| e.to_string())?;
        std::fs::write(snap.path().join("dirty"), "").map_err(|e| e.to_string())?;
        write(root, changed)?;
        git(root, &["add", "-A"])?;
        Ok(Turn { repo, snap })
    }
}

/// What the checker does with one example: the rule's findings' agent texts, and the notes
/// about the rule.
fn check_one(
    rule: &Rule,
    set: &PackSet,
    stage: Stage,
    ex: &Example,
    exe: &Path,
    tools: &Arc<dyn ToolResolver>,
    project: Option<&Project>,
) -> Result<(Vec<String>, Vec<Note>), String> {
    let (mut head, changed): (BTreeMap<String, String>, BTreeMap<String, String>) = match ex {
        Example::Code(code) => (
            BTreeMap::new(),
            [(example_path(rule), code.clone())].into_iter().collect(),
        ),
        Example::Fixture(f) => (
            f.head
                .clone()
                .map(|h| h.into_iter().collect())
                .unwrap_or_default(),
            f.files.clone().into_iter().collect(),
        ),
        Example::Command(_) => return Ok((Vec::new(), Vec::new())),
    };
    // the examples' project (its manifest, tsconfig), under the example's own files
    if let Some(p) = project {
        for (f, t) in &p.files {
            head.entry(f.clone()).or_insert_with(|| t.clone());
        }
    }
    let turn = Turn::new(&head, &changed)?;
    // its installed dependencies (node_modules or .venv, which the repository ignores)
    if let Some((name, target)) = project.and_then(|p| p.link.as_ref()) {
        std::os::unix::fs::symlink(target, turn.repo.path().join(name))
            .map_err(|e| format!("{name}: {e}"))?;
    }
    // the repository root as git and the checker see it (the temp dir may be behind a link)
    let root: PathBuf = turn.repo.path().canonicalize().map_err(|e| e.to_string())?;
    // the example's forge.yaml (its file roles, its accepted changes), as `forge check` reads it
    let config = forge_packs::load_project_config(&root).map_err(|e| e.to_string())?;
    let selection = crate::select::project(&root, set, config.as_ref())?;
    let mut checker = Checker::new(root.clone(), vec![ActiveRule::new(rule.clone())]);
    checker.roles = selection.roles;
    checker.accept = config.and_then(|c| c.accept).unwrap_or_default();
    checker.tools = tools.clone();
    // a type-checked example: the types engine, as `forge check` starts it (its daemon exits
    // once the throwaway repository is gone)
    if project.is_some() {
        let (oracle, notes) = crate::engine::start(
            &root,
            &checker.rules,
            checker.tools.as_ref(),
            true,
            exe,
            env!("CARGO_PKG_VERSION"),
            None,
        );
        let Some(oracle) = oracle else {
            let why: Vec<&str> = notes.iter().map(|n| n.text.as_str()).collect();
            return Err(format!(
                "the types engine did not start: {}",
                why.join("; ")
            ));
        };
        checker.types = Some(Arc::new(oracle));
    }
    let mode = if stage == Stage::Format {
        Mode::All
    } else {
        Mode::Turn(turn.snap.path().to_path_buf())
    };
    let o = checker
        .check(mode, &Options::stop(exe.to_path_buf()))
        .map_err(|e| e.to_string())?;
    // the state the run left for this throwaway repository
    if let (Ok(home), Ok(id)) = (
        forge_home::Home::from_env(),
        forge_home::RepoId::for_root(&root),
    ) {
        let _ = std::fs::remove_dir_all(home.repo_state_dir(&id));
    }
    let texts = o
        .report
        .findings
        .iter()
        .filter(|f| f.rule == rule.id)
        .map(|f| f.agent_text())
        .collect();
    let notes = o
        .report
        .notes
        .into_iter()
        .filter(|n| n.rule.as_deref() == Some(rule.id.as_str()))
        .collect();
    Ok((texts, notes))
}

/// The rule's check target on its code and fixture examples, each a turn of its own repository
/// checked by `exe`'s builtins with `tools`; none without a check target.
pub fn check(
    rule: &Rule,
    set: &PackSet,
    exe: &Path,
    tools: &Arc<dyn ToolResolver>,
) -> Vec<ExampleResult> {
    let Some(stage) = rule.enforce.iter().find_map(|t| match t {
        Target::Check(c) => Some(c.stage),
        _ => None,
    }) else {
        return Vec::new();
    };
    let Some(examples) = &rule.examples else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (kind, n, ex) in examples.listed() {
        if matches!(ex, Example::Command(_)) {
            continue;
        }
        let mut res = match check_one(rule, set, stage, ex, exe, tools, None) {
            Err(e) => ExampleResult::new(
                &rule.id,
                "check",
                kind,
                n,
                ExampleVerdict::Skip {
                    note: Note::new(
                        NoteKind::ForgeFailure,
                        format!("the example could not run: {e}"),
                    )
                    .for_rule(&rule.id),
                },
            ),
            Ok((texts, notes)) => {
                let found = !texts.is_empty();
                let notes: Vec<&Note> = notes.iter().collect();
                let verdict = match (kind, found) {
                    (ExampleKind::Match, true) | (ExampleKind::NoMatch, false) => {
                        ExampleVerdict::Pass
                    }
                    (ExampleKind::Match, false) => {
                        ExampleVerdict::missed(&notes, "did not fail the check")
                    }
                    (ExampleKind::NoMatch, true) => ExampleVerdict::missed(
                        &notes,
                        format!("failed the check: {}", texts.join(" | ")),
                    ),
                };
                let mut r = ExampleResult::new(&rule.id, "check", kind, n, verdict);
                r.agent_texts = texts;
                r
            }
        };
        if res.agent_texts.len() > 20 {
            res.agent_texts.truncate(20);
        }
        out.push(res);
    }
    out
}

/// The TypeScript of a type-checked examples' project whose `examples.deps` name none.
pub const EXAMPLES_TYPESCRIPT: &str = "5.9.3";

/// The project a rule's type-checked examples run in: the manifest declaring `examples.deps`
/// (and, for TypeScript, a tsconfig with their `@types` packages), and the installed
/// dependencies' directory to link in (`node_modules` or `.venv`, with its path).
#[derive(Debug, Clone, Default)]
pub struct Project {
    pub files: BTreeMap<String, String>,
    pub link: Option<(String, PathBuf)>,
}

/// The manifest files of the examples' project of `rule`: what an installer writes before
/// installing (`bun install`, `uv sync`), and what each example's repository holds. `None` for
/// a rule without a type-checked ast-grep target.
pub fn project_files(rule: &Rule) -> Option<BTreeMap<String, String>> {
    astgrep::target_of(rule).filter(|t| t.where_.is_some())?;
    let deps = rule
        .examples
        .as_ref()
        .and_then(|e| e.deps.clone())
        .unwrap_or_default();
    let version = |d: &forge_packs::model::DepSpec| d.version.clone().unwrap_or_default();
    let mut files = BTreeMap::new();
    match rule.scope.language {
        Language::Python => {
            let reqs: Vec<String> = deps
                .iter()
                .filter(|d| d.ecosystem == forge_packs::model::Ecosystem::Pypi)
                .map(|d| format!("{:?}", format!("{}=={}", d.name, version(d))))
                .collect();
            files.insert(
                "pyproject.toml".into(),
                format!(
                    "[project]\nname = \"examples\"\nversion = \"0\"\nrequires-python = \">=3.12\"\ndependencies = [{}]\n",
                    reqs.join(", ")
                ),
            );
        }
        _ => {
            let npm: Vec<_> = deps
                .iter()
                .filter(|d| d.ecosystem == forge_packs::model::Ecosystem::Npm)
                .collect();
            let mut dev: serde_json::Map<String, serde_json::Value> = npm
                .iter()
                .map(|d| (d.name.clone(), serde_json::Value::String(version(d))))
                .collect();
            // the engine answers with the project's TypeScript: the examples' own, else this one
            dev.entry("typescript")
                .or_insert_with(|| serde_json::Value::String(EXAMPLES_TYPESCRIPT.into()));
            let package =
                serde_json::json!({"name": "examples", "private": true, "devDependencies": dev});
            files.insert("package.json".into(), format!("{package:#}\n"));
            let types: Vec<String> = npm
                .iter()
                .filter_map(|d| d.name.strip_prefix("@types/").map(str::to_string))
                .collect();
            let tsconfig = serde_json::json!({"compilerOptions": {
                "strict": true, "target": "es2022", "module": "esnext", "moduleResolution": "bundler",
                "jsx": "preserve", "lib": ["es2022", "dom"], "skipLibCheck": true, "noEmit": true,
                "types": types,
            }});
            files.insert("tsconfig.json".into(), format!("{tsconfig:#}\n"));
        }
    }
    Some(files)
}

/// The rule's ast-grep target with a `where`, on all its examples (code and fixtures), through
/// the checker and the types engine: each example is a turn of its own repository holding
/// `project` (see [`project_files`]), checked with the rule alone. A `match` example must give
/// the rule a finding, a `no_match` one none; a note about the rule (the engine could not
/// decide, a dependency is missing) fails the example, since the engine was asked. None for a
/// rule without such a target.
pub fn typed(
    rule: &Rule,
    set: &PackSet,
    exe: &Path,
    tools: &Arc<dyn ToolResolver>,
    project: &Result<Project, String>,
) -> Vec<ExampleResult> {
    if astgrep::target_of(rule).is_none_or(|t| t.where_.is_none()) {
        return Vec::new();
    }
    let Some(examples) = &rule.examples else {
        return Vec::new();
    };
    // what the examples test is what the rule matches: an advice pattern reports only after an
    // edit, so it runs here as a forbid one would, at the end of the turn
    let mut judged = rule.clone();
    for t in &mut judged.enforce {
        if let Target::AstGrep(a) = t
            && a.mode == forge_packs::model::Mode::Advise
        {
            a.mode = forge_packs::model::Mode::Forbid;
        }
    }
    let rule = &judged;
    examples
        .listed()
        .into_iter()
        .filter(|(_, _, ex)| !matches!(ex, Example::Command(_)))
        .map(|(kind, n, ex)| {
            let verdict = match project {
                Err(e) => ExampleVerdict::Skip {
                    note: Note::new(
                        NoteKind::MissingTool,
                        format!("the examples' dependencies could not be installed: {e}"),
                    )
                    .for_rule(&rule.id),
                },
                Ok(p) => match check_one(rule, set, Stage::Lint, ex, exe, tools, Some(p)) {
                    Err(e) => ExampleVerdict::Fail {
                        why: format!("the example could not run: {e}"),
                    },
                    Ok((_, notes)) if !notes.is_empty() => ExampleVerdict::Fail {
                        why: format!(
                            "the types engine did not decide: {}",
                            notes
                                .iter()
                                .map(|n| n.text.as_str())
                                .collect::<Vec<_>>()
                                .join("; ")
                        ),
                    },
                    Ok((texts, _)) => match (kind, texts.is_empty()) {
                        (ExampleKind::Match, true) => ExampleVerdict::Fail {
                            why: "the rule reported nothing".into(),
                        },
                        (ExampleKind::NoMatch, false) => ExampleVerdict::Fail {
                            why: format!("the rule reported: {}", texts.join("; ")),
                        },
                        _ => ExampleVerdict::Pass,
                    },
                },
            };
            ExampleResult::new(&rule.id, "ast-grep", kind, n, verdict)
        })
        .collect()
}
