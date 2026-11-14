//! `forge_knip` (typescript/knip/unused): the project's own knip, as its config sets it up
//! (entry files, ignores, the issue types its `rules` turn on), on the whole project; its
//! codeclimate report read (§16, `adapters/knip.yaml`). What counts ([`crate::project_tools`]):
//! every issue in a file the change touched (package.json for a dependency), and elsewhere what
//! the change caused (an export or a file it left unused), from a second run at the base.
//!
//! An issue type the project's `rules` set to `error` (the default) fails the check: the
//! project opted into knip, and its CI fails on it. One set to `warn` is advice at Stop, once
//! per turn.

use std::path::Path;

use forge_check::builtins::whole::Located;
use forge_core::{Finding, NoteKind, Report};

use crate::project_tools::{Run, relative};

/// One knip issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub line: u32,
    /// The issue type's title (`Unused exports`).
    pub check: String,
    /// What it is about (`Unused export: sub`; before knip 6, `sub`).
    pub description: String,
    /// knip's `error` (true) or `warn`.
    pub error: bool,
}

impl Located for Issue {
    fn path(&self) -> &str {
        &self.path
    }

    fn key(&self) -> String {
        format!("{}\t{}", self.check, self.description)
    }

    fn line(&self) -> u32 {
        self.line
    }
}

impl Issue {
    /// What the agent reads.
    pub fn text(&self) -> String {
        let what = if self.description.contains(": ") {
            self.description.clone()
        } else {
            format!("{}: {}", self.check, self.description)
        };
        let lower = self.check.to_ascii_lowercase();
        let todo = if lower.starts_with("unused file") {
            "nothing imports it: use it where the request needs it, otherwise delete it"
        } else if lower.contains("dependenc") && lower.starts_with("unused") {
            "nothing uses it: remove it from package.json, or use it"
        } else if lower.starts_with("unlisted") {
            "add it to package.json"
        } else if lower.starts_with("unresolved") {
            "fix the import so it resolves"
        } else if lower.starts_with("duplicate") {
            "export it once"
        } else if lower.starts_with("unused") {
            "nothing uses it: use it where the request needs it, otherwise remove it (or its export)"
        } else {
            "fix it"
        };
        format!(
            "{what} (knip, as the project configures it): {todo}. If something uses it in a way knip cannot see, ask the user."
        )
    }
}

/// knip's codeclimate report: issues whose severity is `major` (error) or `minor` (warn),
/// paths relative to `root`.
pub fn parse(stdout: &str, root: &Path) -> Result<Vec<Issue>, String> {
    let json = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('['))
        .ok_or("its output holds no report")?;
    let v: serde_json::Value =
        serde_json::from_str(json.trim()).map_err(|e| format!("its report is not JSON ({e})"))?;
    let arr = v.as_array().ok_or("its report is not a list of issues")?;
    let mut out = Vec::new();
    for i in arr {
        let s = |k: &str| i.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
        let error = match s("severity").as_str() {
            "major" | "critical" | "blocker" => true,
            "minor" => false,
            _ => continue,
        };
        let loc = &i["location"];
        let Some(path) = loc.get("path").and_then(|p| p.as_str()) else {
            continue;
        };
        let line = loc["positions"]["begin"]["line"]
            .as_u64()
            .or_else(|| loc["lines"]["begin"].as_u64())
            .unwrap_or(0) as u32;
        out.push(Issue {
            path: relative(path, root),
            line,
            check: s("check_name"),
            description: s("description"),
            error,
        });
    }
    Ok(out)
}

/// `forge_knip`.
pub fn run(run: &Run<'_>) -> Report {
    let scan = |dir: &Path| {
        let out = run.tool(dir, "knip", "unused", &[], &[])?;
        // 0: no issue of error severity, 1: some (or warnings alone, from knip 6)
        if !matches!(out.status, 0 | 1) {
            return Err(run.failed("knip", &out));
        }
        parse(&out.stdout, dir).map_err(|why| {
            run.note(
                NoteKind::ForgeFailure,
                format!("not checked: knip's report could not be read: {why}"),
            )
        })
    };
    let (issues, notes) = match run.with_base(&scan) {
        Ok(x) => x,
        Err(n) => return run.only_note(n),
    };
    let finding = |i: &Issue| {
        Finding::error(run.ctx.rule.clone(), i.text()).at(&i.path, (i.line > 0).then_some(i.line))
    };
    let (errors, advice): (Vec<&Issue>, Vec<&Issue>) = issues.iter().partition(|i| i.error);
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
    fn codeclimate_of_knip_5_and_6() {
        let root = Path::new("/r");
        let six = r#"[{"type":"issue","check_name":"Unused dependencies","description":"Unused dependency: lodash","categories":["Bug Risk"],"location":{"path":"package.json","lines":{"begin":0,"end":0}},"severity":"major","fingerprint":"x"},{"type":"issue","check_name":"Unused exports","description":"Unused export: sub","categories":["Bug Risk"],"location":{"path":"src/math.ts","positions":{"begin":{"line":5,"column":17},"end":{"line":5,"column":17}}},"severity":"minor","fingerprint":"y"},{"type":"issue","check_name":"Unused files","description":"x","location":{"path":"src/o.ts","lines":{"begin":0,"end":0}},"severity":"info"}]"#;
        let got = parse(six, root).unwrap();
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(
            (got[0].path.as_str(), got[0].line, got[0].error),
            ("package.json", 0, true)
        );
        assert_eq!(
            (got[1].path.as_str(), got[1].line, got[1].error),
            ("src/math.ts", 5, false)
        );
        assert!(
            got[1].text().starts_with("Unused export: sub (knip"),
            "{}",
            got[1].text()
        );
        let five = r#"[{"check_name":"Unused exports","description":"sub","location":{"path":"/r/src/math.ts","positions":{"begin":{"line":5,"column":17}}},"severity":"major"}]"#;
        let got = parse(five, root).unwrap();
        assert_eq!(got[0].path, "src/math.ts");
        assert!(got[0].text().starts_with("Unused exports: sub (knip"));
        assert!(forge_core::internals::find(&got[0].text()).is_empty());
        assert!(parse("Unknown option", root).is_err());
    }
}
