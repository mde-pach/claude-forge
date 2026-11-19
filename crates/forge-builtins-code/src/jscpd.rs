//! `forge_jscpd` (code/jscpd/duplicates): the project's own jscpd, with its config
//! (`.jscpd.json`, or the `jscpd` key of package.json: paths, ignores, formats, sizes), run as
//! a bare `jscpd` runs there; its JSON report read (§16, `adapters/jscpd.yaml`). A duplicate
//! counts when one of its two copies is in a file the change touched: a new duplicate always
//! is (the change wrote one of the copies), so no base run is needed.
//!
//! Duplicates are advice at Stop, once per turn: two similar blocks can be legitimate (tests,
//! declarations). They fail the check when the project's config makes jscpd fail on them, as
//! its CI does: a `threshold` the duplication goes over, or an `exitCode`.

use std::path::Path;

use forge_check::builtins::whole::Located;
use forge_core::{Finding, NoteKind, Report};

use crate::project_tools::{Run, relative};

/// One copy of a duplicated block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub name: String,
    pub start: u32,
    pub end: u32,
}

/// One duplicate: a block found twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    pub first: Copy,
    pub second: Copy,
    pub lines: u32,
}

impl Located for Duplicate {
    fn path(&self) -> &str {
        &self.first.name
    }

    fn about(&self) -> Vec<&str> {
        vec![&self.second.name]
    }

    fn key(&self) -> String {
        format!("{} {}", self.second.name, self.lines)
    }

    fn line(&self) -> u32 {
        self.first.start
    }
}

impl Duplicate {
    /// The finding, at the copy in a touched file (`touched`), naming the other one.
    pub fn finding(&self, rule: &str, touched: &dyn Fn(&str) -> bool) -> Finding {
        let (here, other) = if touched(&self.first.name) || !touched(&self.second.name) {
            (&self.first, &self.second)
        } else {
            (&self.second, &self.first)
        };
        let at = if here.name == other.name {
            format!("lines {}-{} of this file", other.start, other.end)
        } else {
            format!("{}:{}-{}", other.name, other.start, other.end)
        };
        Finding::error(
            rule,
            format!(
                "lines {}-{} repeat {at} ({} lines, jscpd as the project configures it): put the shared code in one function or module and use it from both places.",
                here.start, here.end, self.lines
            ),
        )
        .at(&here.name, Some(here.start))
    }
}

/// The project's jscpd config: the `.jscpd.json` file, else package.json's `jscpd`.
pub fn config(root: &Path) -> Option<serde_json::Value> {
    let read = |f: &str| {
        std::fs::read_to_string(root.join(f))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    };
    read(".jscpd.json").or_else(|| read("package.json").and_then(|p| p.get("jscpd").cloned()))
}

/// Whether the config makes a run with duplicates fail, given the duplication percentage.
pub fn fails(config: &serde_json::Value, percentage: f64) -> bool {
    let exit_code = config["exitCode"].as_i64().unwrap_or(0) != 0;
    let over = config["threshold"].as_f64().is_some_and(|t| percentage > t);
    exit_code || over
}

/// jscpd's report: the duplicates (paths relative to `root`) and the duplication percentage.
pub fn parse(text: &str, root: &Path) -> Result<(Vec<Duplicate>, f64), String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("its report is not JSON ({e})"))?;
    let copy = |f: &serde_json::Value| -> Option<Copy> {
        Some(Copy {
            name: relative(f["name"].as_str()?, root),
            start: f["start"]
                .as_u64()
                .or_else(|| f["startLoc"]["line"].as_u64())? as u32,
            end: f["end"].as_u64().or_else(|| f["endLoc"]["line"].as_u64())? as u32,
        })
    };
    let mut out = Vec::new();
    for d in v["duplicates"]
        .as_array()
        .ok_or("its report lists no duplicates")?
    {
        let (Some(first), Some(second)) = (copy(&d["firstFile"]), copy(&d["secondFile"])) else {
            continue;
        };
        out.push(Duplicate {
            first,
            second,
            lines: d["lines"].as_u64().unwrap_or(0) as u32,
        });
    }
    let pct = v["statistics"]["total"]["percentage"]
        .as_f64()
        .unwrap_or(0.0);
    Ok((out, pct))
}

/// `forge_jscpd`.
pub fn run(run: &Run<'_>) -> Report {
    let Some(config) = config(run.root()) else {
        return Report::default();
    };
    let dir = match tempfile::Builder::new().prefix("forge-jscpd.").tempdir() {
        Ok(d) => d,
        Err(e) => {
            return run.only_note(run.note(
                NoteKind::ForgeFailure,
                format!("not checked: no directory for jscpd's report ({e})"),
            ));
        }
    };
    let args = vec![dir.path().display().to_string()];
    let out = match run.tool(run.root(), "jscpd", "duplicates-json", &args, &[]) {
        Ok(o) => o,
        Err(n) => return run.only_note(n),
    };
    let report = dir.path().join("jscpd-report.json");
    // 1 with a report: over the threshold, or the config's exitCode
    let Some(text) = std::fs::read_to_string(&report)
        .ok()
        .filter(|_| matches!(out.status, 0 | 1))
    else {
        return run.only_note(run.failed("jscpd", &out));
    };
    let (dups, pct) = match parse(&text, run.root()) {
        Ok(x) => x,
        Err(why) => {
            return run.only_note(run.note(
                NoteKind::ForgeFailure,
                format!("not checked: jscpd's report could not be read: {why}"),
            ));
        }
    };
    let changed = |p: &str| run.changed(p);
    let findings: Vec<Finding> = run
        .touched(dups)
        .iter()
        .map(|d| d.finding(&run.ctx.rule, &changed))
        .collect();
    if fails(&config, pct) {
        run.report(findings, Vec::new(), Vec::new())
    } else {
        run.report(Vec::new(), findings, Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_and_what_fails() {
        let root = Path::new("/r");
        let text = r#"{"duplicates":[{"format":"typescript","lines":15,"firstFile":{"name":"src/a.ts","start":1,"end":15},"secondFile":{"name":"/r/src/b.ts","start":3,"end":17}}],"statistics":{"total":{"percentage":50.0}}}"#;
        let (d, pct) = parse(text, root).unwrap();
        assert_eq!(pct, 50.0);
        assert_eq!(d[0].second.name, "src/b.ts");
        let f = d[0].finding("r", &|p| p == "src/b.ts");
        assert_eq!(
            f.agent_text(),
            "src/b.ts:3 lines 3-17 repeat src/a.ts:1-15 (15 lines, jscpd as the project configures it): put the shared code in one function or module and use it from both places."
        );
        assert!(forge_core::internals::find(&f.agent_text()).is_empty());
        let cfg = |t: &str| serde_json::from_str::<serde_json::Value>(t).unwrap();
        assert!(!fails(&cfg("{}"), 50.0));
        assert!(!fails(&cfg(r#"{"threshold": 50}"#), 50.0));
        assert!(fails(&cfg(r#"{"threshold": 10}"#), 50.0));
        assert!(fails(&cfg(r#"{"exitCode": 1}"#), 1.0));
    }
}
