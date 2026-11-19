//! `forge_import_linter` (python/import-linter/contracts): the project's own import-linter
//! (`lint-imports`), with its contracts (`.importlinter`, `setup.cfg` `[importlinter]` or
//! `pyproject.toml` `[tool.importlinter]`), on the whole project; its report read (§16,
//! `adapters/lint-imports.yaml`).
//!
//! A broken contract lists the import chains that break it, each link an import in a module.
//! A chain the change made holds an import it added or rerouted, so it is in or about a module
//! the change touched: a chain counts when a module of one of its links is in a file the change
//! touched, and every chain counts when the change edited the contracts themselves. No base run
//! is needed for those. An entry that names no import (an `acyclic_siblings` contract's package
//! dependency, `.orders -> .billing (1 import)`) cannot be placed in a file: it counts when the
//! contracts changed, in CI's full run, and otherwise when import-linter run on the tree as it
//! was at the base did not report it (the change made that dependency; one the base already had
//! that gains an import does not count). A base tree that cannot be made leaves a note.
//!
//! Every chain that counts fails the check: the contracts are the project's declared
//! architecture, which its CI enforces.

use forge_check::builtins::whole::Located;
use forge_core::{Finding, NoteKind, Report};

use crate::project_tools::{Run, last_line};
use crate::util::git_lines;

/// One import of a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub importer: String,
    pub imported: String,
    pub lines: Vec<u32>,
}

/// One import chain that breaks a contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub contract: String,
    /// What the contract forbids (`shop.web is not allowed to import shop.db`).
    pub what: String,
    pub links: Vec<Link>,
}

/// What a broken contract lists, as read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Broken {
    pub chains: Vec<Chain>,
    /// Entries that name no import: contract, text.
    pub other: Vec<(String, String)>,
}

/// The report's broken contracts; `Err` when it has none to read.
pub fn parse(text: &str) -> Result<Broken, String> {
    let link = regex::Regex::new(r"^-?\s*([A-Za-z_][\w.]*) -> ([A-Za-z_][\w.]*) \(([^)]*)\)\s*$")
        .expect("regex");
    let line_no = regex::Regex::new(r"l\.(\d+)").expect("regex");
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let start = lines
        .iter()
        .position(|l| l.trim() == "Broken contracts")
        .ok_or("its report has no broken contract")?;
    let dashes = |l: &str| !l.trim().is_empty() && l.trim().chars().all(|c| c == '-');
    let mut out = Broken::default();
    let (mut contract, mut what) = (String::new(), String::new());
    let mut chain: Option<Chain> = None;
    let mut i = start + 1;
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim();
        if dashes(l) {
            i += 1;
            continue;
        }
        if !t.is_empty() && lines.get(i + 1).is_some_and(|n| dashes(n)) {
            out.chains.extend(chain.take());
            contract = t.to_owned();
            what.clear();
            i += 2;
            continue;
        }
        if t.is_empty() {
            out.chains.extend(chain.take());
        } else if let Some(rest) = t.strip_prefix('-') {
            out.chains.extend(chain.take());
            match link.captures(rest.trim()) {
                Some(c) => {
                    chain = Some(Chain {
                        contract: contract.clone(),
                        what: what.clone(),
                        links: vec![Link {
                            importer: c[1].to_owned(),
                            imported: c[2].to_owned(),
                            lines: line_no
                                .captures_iter(&c[3])
                                .filter_map(|n| n[1].parse().ok())
                                .collect(),
                        }],
                    });
                }
                None => out.other.push((contract.clone(), rest.trim().to_owned())),
            }
        } else if let (Some(ch), Some(c)) = (chain.as_mut(), link.captures(t)) {
            ch.links.push(Link {
                importer: c[1].to_owned(),
                imported: c[2].to_owned(),
                lines: line_no
                    .captures_iter(&c[3])
                    .filter_map(|n| n[1].parse().ok())
                    .collect(),
            });
        } else if chain.is_none() && t.ends_with(':') {
            what = t.trim_end_matches(':').to_owned();
        }
        i += 1;
    }
    out.chains.extend(chain);
    Ok(out)
}

/// The file of a module among the project's Python files: `a/b.py` or `a/b/__init__.py`, at
/// the root or under a directory (`src/`), the shortest path when several match.
pub fn module_file(files: &[String], module: &str) -> Option<String> {
    let base = module.replace('.', "/");
    let names = [format!("{base}.py"), format!("{base}/__init__.py")];
    files
        .iter()
        .filter(|f| {
            names
                .iter()
                .any(|n| *f == n || f.ends_with(&format!("/{n}")))
        })
        .min_by_key(|f| f.len())
        .cloned()
}

/// A chain with the files of its modules.
#[derive(Debug, Clone)]
pub struct PlacedChain {
    pub chain: Chain,
    /// Per link: the importer's file and the imported module's file.
    pub files: Vec<(Option<String>, Option<String>)>,
    /// The link the finding is at (its importer's file).
    pub at: usize,
}

impl Located for PlacedChain {
    fn path(&self) -> &str {
        self.files[self.at].0.as_deref().unwrap_or("")
    }

    fn about(&self) -> Vec<&str> {
        self.files
            .iter()
            .flat_map(|(a, b)| [a.as_deref(), b.as_deref()])
            .flatten()
            .collect()
    }

    fn key(&self) -> String {
        format!("{} {}", self.chain.contract, self.chain.text())
    }

    fn line(&self) -> u32 {
        self.chain.links[self.at]
            .lines
            .first()
            .copied()
            .unwrap_or(0)
    }
}

impl Chain {
    /// `a -> b (l.3) -> c (l.2, l.6)`.
    pub fn text(&self) -> String {
        let mut s = self
            .links
            .first()
            .map(|l| l.importer.clone())
            .unwrap_or_default();
        for l in &self.links {
            let at: Vec<String> = l.lines.iter().map(|n| format!("l.{n}")).collect();
            s.push_str(&format!(" -> {} ({})", l.imported, at.join(", ")));
        }
        s
    }

    /// What the agent reads.
    pub fn message(&self) -> String {
        let what = if self.what.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.what)
        };
        format!(
            "the import-linter contract `{}` is broken{what}: {}. Remove the import or move the code where the contract allows it; the contracts are the project's, change them only if the user asks.",
            self.contract,
            self.text()
        )
    }
}

/// The import-linter config of a file's text, as it compares between two versions: the whole
/// `.importlinter`, the `[importlinter…]` sections of `setup.cfg`, `[tool.importlinter]` of
/// `pyproject.toml`.
pub fn config_of(file: &str, text: &str) -> String {
    match file {
        "pyproject.toml" => text
            .parse::<toml::Table>()
            .ok()
            .and_then(|t| t.get("tool")?.get("importlinter").cloned())
            .map(|v| v.to_string())
            .unwrap_or_default(),
        "setup.cfg" => {
            let mut keep = false;
            let mut out = String::new();
            for l in text.lines() {
                if l.starts_with('[') {
                    keep = l.starts_with("[importlinter");
                }
                if keep {
                    out.push_str(l);
                    out.push('\n');
                }
            }
            out
        }
        _ => text.to_owned(),
    }
}

/// Whether the change edited the contracts.
fn config_changed(run: &Run<'_>) -> bool {
    [".importlinter", "setup.cfg", "pyproject.toml"]
        .iter()
        .filter(|f| run.changed(f))
        .any(|f| {
            let now = std::fs::read_to_string(run.root().join(f)).unwrap_or_default();
            let before = run.base_text(f).unwrap_or_default();
            config_of(f, &now) != config_of(f, &before)
        })
}

/// A package dependency entry without its import count: `.db -> .web (2 imports)` is `.db ->
/// .web`, so a dependency the base had that gains an import is the same one.
pub fn dependency(entry: &str) -> String {
    match entry.rfind(" (") {
        Some(i) if entry.ends_with(')') => entry[..i].trim().to_owned(),
        _ => entry.trim().to_owned(),
    }
}

/// `forge_import_linter`.
pub fn run(run: &Run<'_>) -> Report {
    // rich wraps its report at 80 columns when it is not a terminal: one chain, one line
    let env = [("COLUMNS".to_owned(), "10000".to_owned())];
    let out = match run.tool(run.root(), "lint-imports", "lint", &[], &env) {
        Ok(o) => o,
        Err(n) => return run.only_note(n),
    };
    if out.status == 0 {
        return Report::default();
    }
    let broken = match parse(&out.stdout).or_else(|_| parse(&out.text())) {
        Ok(b) if out.status == 1 => b,
        _ => {
            return run.only_note(run.note(
                NoteKind::ForgeFailure,
                format!(
                    "not checked: import-linter could not run (exit {}: {})",
                    out.status,
                    last_line(&out.text())
                ),
            ));
        }
    };
    let every = run.all() || config_changed(run);
    let files: Vec<String> = git_lines(run.root(), &["ls-files", "-co", "--exclude-standard"])
        .into_iter()
        .filter(|f| f.ends_with(".py"))
        .collect();
    let located: Vec<PlacedChain> = broken
        .chains
        .into_iter()
        .map(|chain| {
            let files: Vec<(Option<String>, Option<String>)> = chain
                .links
                .iter()
                .map(|l| {
                    (
                        module_file(&files, &l.importer),
                        module_file(&files, &l.imported),
                    )
                })
                .collect();
            let at = files
                .iter()
                .position(|(f, _)| f.as_deref().is_some_and(|f| run.changed(f)))
                .or_else(|| files.iter().position(|(f, _)| f.is_some()))
                .unwrap_or(0);
            PlacedChain { chain, files, at }
        })
        .collect();
    let counted = if every { located } else { run.touched(located) };
    let mut errors: Vec<Finding> = counted
        .iter()
        .map(|c| {
            let f = Finding::error(run.ctx.rule.clone(), c.chain.message());
            match c.path() {
                "" => f,
                p => f.at(p, (c.line() > 0).then_some(c.line())),
            }
        })
        .collect();
    let mut notes = Vec::new();
    // the entries the base run reported too, by contract and dependency (not its import count)
    let before: Result<Vec<(String, String)>, String> = if every || broken.other.is_empty() {
        Ok(Vec::new())
    } else {
        run.base_tree().and_then(|tree| {
            let out = run
                .tool(tree.path(), "lint-imports", "lint", &[], &env)
                .map_err(|n| n.text)?;
            if out.status == 0 {
                return Ok(Vec::new());
            }
            parse(&out.stdout).or_else(|_| parse(&out.text())).map(|b| {
                b.other
                    .iter()
                    .map(|(c, t)| (c.clone(), dependency(t)))
                    .collect()
            })
        })
    };
    for (contract, text) in &broken.other {
        let new = match &before {
            Ok(base) => !base.contains(&(contract.clone(), dependency(text))),
            Err(_) => false,
        };
        if new {
            // the entry is where import-linter would cut the cycle, not necessarily the import
            // the change added
            errors.push(Finding::error(
                run.ctx.rule.clone(),
                format!(
                    "the change made a dependency cycle between sibling packages, which the import-linter contract `{contract}` forbids (import-linter would cut it at {text}). Remove the import the change added that closes the cycle; the contracts are the project's, change them only if the user asks."
                ),
            ));
        } else if every {
            errors.push(Finding::error(
                run.ctx.rule.clone(),
                format!(
                    "the import-linter contract `{contract}` is broken: {text}. Remove those imports; the contracts are the project's, change them only if the user asks."
                ),
            ));
        } else if let Err(why) = &before {
            notes.push(run.note(
                NoteKind::NotEnforced,
                format!(
                    "not checked: the import-linter contract `{contract}` is broken naming no import ({text}), and the base could not be checked to tell whether the change broke it: {why}"
                ),
            ));
        }
    }
    run.report(errors, Vec::new(), notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = "Logo\n\n---------\nContracts\n---------\n\nLayers BROKEN\n\n----------------\nBroken contracts\n----------------\n\nLayers\n------\n\nshop.db is not allowed to import shop.web:\n\n- shop.db.models -> shop.web.views (l.1)\n\n\nWeb never touches the database\n------------------------------\n\nshop.web is not allowed to import shop.db:\n\n-   shop.web.page -> shop.domain.svc (l.1)\n    shop.domain.svc -> shop.db.models (l.2, l.6)\n\n-   shop.web.views -> shop.db.models (l.3)\n\n\nSiblings\n--------\n\nNo cycles are allowed in shop.\nIt could be made acyclic by removing 1 dependencies:\n\n- .db -> .web (1 import)\n";

    #[test]
    fn chains_by_contract() {
        let b = parse(REPORT).unwrap();
        assert_eq!(b.chains.len(), 3, "{b:?}");
        assert_eq!(b.chains[0].contract, "Layers");
        assert_eq!(
            b.chains[0].what,
            "shop.db is not allowed to import shop.web"
        );
        let two = &b.chains[1];
        assert_eq!(two.contract, "Web never touches the database");
        assert_eq!(two.links.len(), 2);
        assert_eq!(two.links[1].lines, vec![2, 6]);
        assert_eq!(
            two.text(),
            "shop.web.page -> shop.domain.svc (l.1) -> shop.db.models (l.2, l.6)"
        );
        assert_eq!(
            b.other,
            vec![("Siblings".to_owned(), ".db -> .web (1 import)".to_owned())]
        );
        assert!(forge_core::internals::find(&two.message()).is_empty());
        assert!(parse("Contracts: 1 kept, 0 broken.").is_err());
    }

    #[test]
    fn modules_to_files() {
        let files: Vec<String> = [
            "src/shop/web/views.py",
            "src/shop/web/__init__.py",
            "tests/shop/web/views.py",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            module_file(&files, "shop.web.views").as_deref(),
            Some("src/shop/web/views.py")
        );
        assert_eq!(
            module_file(&files, "shop.web").as_deref(),
            Some("src/shop/web/__init__.py")
        );
        assert_eq!(module_file(&files, "requests"), None);
    }

    #[test]
    fn a_dependency_is_compared_without_its_count() {
        assert_eq!(dependency(".db -> .web (1 import)"), ".db -> .web");
        assert_eq!(dependency(".db -> .web (2 imports)"), ".db -> .web");
        assert_eq!(dependency(".db -> .web"), ".db -> .web");
    }

    #[test]
    fn only_the_contracts_compare() {
        let a = "[project]\nname = \"x\"\n\n[tool.importlinter]\nroot_package = \"shop\"\n";
        let b = "[project]\nname = \"y\"\n\n[tool.importlinter]\nroot_package = \"shop\"\n";
        assert_eq!(
            config_of("pyproject.toml", a),
            config_of("pyproject.toml", b)
        );
        let c = "[metadata]\nname = x\n\n[importlinter]\nroot_package = shop\n";
        assert_eq!(
            config_of("setup.cfg", c),
            "[importlinter]\nroot_package = shop\n"
        );
    }
}
