//! The Node version file agrees with package.json (`node_version.mjs`).
//!
//! The root `.nvmrc` / `.node-version` (a version: "22", "v22.11.0"; a partial version stands
//! for the newest release of that line, which nvm, fnm and setup-node install) must satisfy
//! package.json `engines.node` and `devEngines.runtime` (name "node") version ranges. An alias
//! (lts/*, node, lts/iron) is not a version and is reported. The finding is attributed to the
//! version file when it changed, else to package.json.

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};
use serde_json::Value;

use crate::new_deps::satisfies;
use crate::util::{Change, at, list, obj, regex_static};

pub struct NodeVersion;

impl Builtin for NodeVersion {
    fn name(&self) -> &'static str {
        "node_version"
    }

    fn run(&self, ctx: &BuiltinCtx, _args: &[String]) -> Report {
        let ch = Change::new(ctx);
        let mut r = Report::default();
        let Some((file, text)) = [".nvmrc", ".node-version"]
            .into_iter()
            .find_map(|f| ch.now(f).map(|t| (f, t)))
        else {
            return r;
        };
        let Some(Ok(Value::Object(pkg))) = ch.now("package.json").map(|t| serde_json::from_str(&t))
        else {
            return r;
        };
        let mut ranges: Vec<(&str, String)> = vec![];
        if let Some(n) = obj(pkg.get("engines")).get("node").and_then(Value::as_str) {
            ranges.push(("engines.node", n.to_owned()));
        }
        for rt in list(obj(pkg.get("devEngines")).get("runtime")) {
            let rt = obj(Some(rt));
            if rt.get("name").and_then(Value::as_str) == Some("node")
                && let Some(v) = rt.get("version").and_then(Value::as_str)
            {
                ranges.push(("devEngines.runtime", v.to_owned()));
            }
        }
        if ranges.is_empty() {
            return r;
        }
        let raw = text
            .lines()
            .map(|l| regex_static(r"#.*$").replace(l, "").trim().to_owned())
            .find(|l| !l.is_empty())
            .unwrap_or_default();
        let attributed = if ch.is_changed(file) || !ch.is_changed("package.json") {
            file
        } else {
            "package.json"
        };
        let Some(m) = regex_static(r"^v?(\d+)(?:\.(\d+))?(?:\.(\d+))?$").captures(&raw) else {
            let wanted = ranges
                .iter()
                .map(|(k, v)| format!("{k} \"{v}\""))
                .collect::<Vec<_>>()
                .join(", ");
            r.findings.push(at(
                &ctx.rule,
                attributed,
                0,
                format!("{file} says \"{raw}\", which is not a version; write the Node version package.json requires ({wanted})"),
            ));
            return r;
        };
        // a partial version is the newest release of its line
        let part = |i: usize| m.get(i).map_or("9999", |x| x.as_str());
        let version = format!("{}.{}.{}", &m[1], part(2), part(3));
        for (key, range) in &ranges {
            if !satisfies(&version, range) {
                r.findings.push(at(
                    &ctx.rule,
                    attributed,
                    0,
                    format!("{file} ({raw}) does not satisfy package.json {key} \"{range}\"; change one so they agree"),
                ));
            }
        }
        r
    }
}
