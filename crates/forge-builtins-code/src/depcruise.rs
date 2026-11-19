//! `forge_depcruise` (typescript/dependency-cruiser/rules): the project's own
//! dependency-cruiser, with its config (`.dependency-cruiser.{js,cjs,mjs,json}`: its forbidden,
//! allowed and required rules, its options), on the project's top-level directories and root
//! scripts (git's: never `.git` or an ignored `node_modules`); its JSON result read (§16,
//! `adapters/depcruise.yaml`). What counts ([`crate::project_tools`]): a violation in a file the
//! change touched or about one (an import of a file it touched, a cycle through one), and
//! elsewhere what the change caused (a module it left an orphan, a rule its config change
//! broke), from a second run at the base.
//!
//! A violation of a rule of `error` severity fails the check: the rule is the project's own
//! declared contract, which its CI enforces. One of `warn` severity is advice at Stop, once per
//! turn; `info` and `ignore` are not reported.

use std::collections::BTreeMap;
use std::path::Path;

use forge_check::builtins::whole::Located;
use forge_core::{Finding, NoteKind, Report};

use crate::project_tools::{Run, line_quoting, relative};

/// The config files dependency-cruiser reads, in its order.
pub const CONFIGS: &[&str] = &[
    ".dependency-cruiser.js",
    ".dependency-cruiser.cjs",
    ".dependency-cruiser.mjs",
    ".dependency-cruiser.json",
];

/// The root scripts it cruises besides the top-level directories.
const SCRIPTS: &[&str] = &[
    ".js", ".cjs", ".mjs", ".jsx", ".ts", ".cts", ".mts", ".tsx", ".vue", ".svelte",
];

/// One violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// `dependency`, `cycle`, `module`, `reachability`, … (empty before it was given).
    pub kind: String,
    pub from: String,
    pub to: String,
    pub rule: String,
    pub error: bool,
    /// The modules of a cycle, or the path a reachability rule followed.
    pub through: Vec<String>,
    /// The import as written (`../db/client`).
    pub specifier: Option<String>,
    pub comment: Option<String>,
    pub line: u32,
}

impl Located for Violation {
    fn path(&self) -> &str {
        &self.from
    }

    fn about(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for p in std::iter::once(self.to.as_str()).chain(self.through.iter().map(String::as_str)) {
            if p != self.from && !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    fn key(&self) -> String {
        let mut through = self.through.clone();
        through.sort();
        format!(
            "{}\t{}\t{}\t{}",
            self.rule,
            self.kind,
            self.to,
            through.join(" ")
        )
    }

    fn line(&self) -> u32 {
        self.line
    }
}

impl Violation {
    /// What the agent reads.
    pub fn text(&self) -> String {
        let why = self
            .comment
            .as_deref()
            .map(|c| format!(" ({})", c.trim().trim_end_matches('.')))
            .unwrap_or_default();
        let rule = format!("the project's dependency-cruiser rule `{}`{why}", self.rule);
        let what = match self.kind.as_str() {
            "cycle" => {
                let mut chain = vec![self.from.as_str()];
                chain.extend(self.through.iter().map(String::as_str));
                format!(
                    "is in an import cycle ({}), which {rule} forbids: break the cycle",
                    chain.join(" -> ")
                )
            }
            "dependency" | "" if self.to != self.from => format!(
                "imports {}, which {rule} forbids: import what the rule allows instead",
                self.to
            ),
            _ if self.to != self.from => {
                format!("-> {} breaks {rule}: change the code so it holds", self.to)
            }
            _ => format!("breaks {rule}: change the code so it holds"),
        };
        format!("{what}. The rules are the project's: change them only if the user asks.")
    }
}

fn names(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter_map(|e| {
            e.as_str()
                .or_else(|| e.get("name").and_then(|n| n.as_str()))
                .map(str::to_owned)
        })
        .collect()
}

/// dependency-cruiser's JSON result: the violations of `error` and `warn` severity, paths
/// relative to `root`, each import's line read from its file.
pub fn parse(stdout: &str, root: &Path) -> Result<Vec<Violation>, String> {
    let start = stdout.find('{').ok_or("its output holds no result")?;
    let v: serde_json::Value = serde_json::from_str(&stdout[start..])
        .map_err(|e| format!("its result is not JSON ({e})"))?;
    let summary = v.get("summary").ok_or("its result has no summary")?;
    let mut comments: BTreeMap<String, String> = BTreeMap::new();
    for kind in ["forbidden", "allowed", "required"] {
        for r in summary["ruleSetUsed"][kind]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let (Some(n), Some(c)) = (r["name"].as_str(), r["comment"].as_str()) {
                comments.insert(n.to_owned(), c.to_owned());
            }
        }
    }
    let mut out = Vec::new();
    for x in summary["violations"].as_array().into_iter().flatten() {
        let error = match x["rule"]["severity"].as_str() {
            Some("error") => true,
            Some("warn") => false,
            _ => continue,
        };
        let s = |k: &str| x[k].as_str().map(|p| relative(p, root)).unwrap_or_default();
        let rule = x["rule"]["name"].as_str().unwrap_or("").to_owned();
        let mut through = names(x.get("cycle"));
        through.extend(names(x.get("via")));
        let through = through.iter().map(|p| relative(p, root)).collect();
        let specifier = x["unresolvedTo"].as_str().map(str::to_owned);
        let from = s("from");
        let line = specifier
            .as_deref()
            .and_then(|sp| line_quoting(root, &from, sp))
            .unwrap_or(0);
        out.push(Violation {
            kind: x["type"].as_str().unwrap_or("").to_owned(),
            to: s("to"),
            comment: comments.get(&rule).cloned(),
            rule,
            error,
            through,
            specifier,
            from,
            line,
        });
    }
    Ok(out)
}

/// `forge_depcruise`.
pub fn run(run: &Run<'_>) -> Report {
    let Some(config) = CONFIGS.iter().find(|c| run.root().join(c).is_file()) else {
        return Report::default();
    };
    let paths = run.top_level(SCRIPTS);
    if paths.is_empty() {
        return Report::default();
    }
    let scan = |dir: &Path| {
        let mut args = vec![(*config).to_owned()];
        // the base may not have them all
        args.extend(paths.iter().filter(|p| dir.join(p).exists()).cloned());
        let out = run.tool(dir, "depcruise", "cruise-json", &args, &[])?;
        if out.status != 0 {
            return Err(run.failed("dependency-cruiser", &out));
        }
        parse(&out.stdout, dir).map_err(|why| {
            run.note(
                NoteKind::ForgeFailure,
                format!("not checked: dependency-cruiser's result could not be read: {why}"),
            )
        })
    };
    let (found, notes) = match run.with_base(&scan) {
        Ok(x) => x,
        Err(n) => return run.only_note(n),
    };
    let finding = |v: &Violation| {
        Finding::error(run.ctx.rule.clone(), v.text()).at(&v.from, (v.line > 0).then_some(v.line))
    };
    let (errors, advice): (Vec<&Violation>, Vec<&Violation>) = found.iter().partition(|v| v.error);
    run.report(
        errors.into_iter().map(finding).collect(),
        advice.into_iter().map(finding).collect(),
        notes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn violations_of_error_and_warn_rules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
        std::fs::write(
            dir.path().join("src/ui/a.ts"),
            "import { b } from \"./b\";\nimport { q } from \"../db/client\";\n",
        )
        .unwrap();
        let json = r#"{"modules":[],"summary":{"violations":[
            {"type":"dependency","from":"src/ui/a.ts","to":"src/db/client.ts","unresolvedTo":"../db/client","rule":{"severity":"error","name":"ui-not-to-db"}},
            {"type":"cycle","from":"src/ui/a.ts","to":"src/ui/b.ts","unresolvedTo":"./b","rule":{"severity":"warn","name":"no-circular"},"cycle":[{"name":"src/ui/b.ts"},{"name":"src/ui/a.ts"}]},
            {"type":"module","from":"src/lonely.ts","to":"src/lonely.ts","rule":{"severity":"info","name":"no-orphans"}}],
            "ruleSetUsed":{"forbidden":[{"name":"ui-not-to-db","comment":"The UI goes through the services."}]}}}"#;
        let got = parse(json, dir.path()).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].line, got[0].error), (2, true));
        assert_eq!(
            got[0].text(),
            "imports src/db/client.ts, which the project's dependency-cruiser rule `ui-not-to-db` (The UI goes through the services) forbids: import what the rule allows instead. The rules are the project's: change them only if the user asks."
        );
        assert_eq!((got[1].line, got[1].error), (1, false));
        assert_eq!(got[1].about(), vec!["src/ui/b.ts"]);
        assert!(
            got[1]
                .text()
                .contains("(src/ui/a.ts -> src/ui/b.ts -> src/ui/a.ts)")
        );
        assert!(forge_core::internals::find(&got[1].text()).is_empty());
        assert!(parse("error: unknown option", dir.path()).is_err());
    }
}
