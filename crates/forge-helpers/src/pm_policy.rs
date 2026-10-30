//! Package manager policies of npm, pnpm and Yarn (`pm_policy.mjs <mode>`).
//!
//! Modes (the change against the base copies):
//! - `npm-cooldown <s>`: .npmrc min-release-age is at least `<s>` seconds in days, and
//!   package.json requires npm >= 11.10 (older npm ignores it);
//! - `npm-scripts`: package.json allowScripts gains no approval; .npmrc gains no
//!   ignore-scripts=false, allow-scripts entry or dangerously-allow-all-scripts;
//! - `pnpm-builds`: pnpm-workspace.yaml gains no allowBuilds approval or onlyBuiltDependencies
//!   entry, dangerouslyAllowAllBuilds is not turned on, strictDepBuilds and blockExoticSubdeps
//!   are not turned off;
//! - `yarn-scripts`: .yarnrc.yml enableScripts is not turned on; package.json dependenciesMeta
//!   gains no `built: true`;
//! - `pnpm-workspace`: a changed package.json depends on a workspace package with `workspace:`;
//! - `pnpm-catalog`: a dependency added or changed whose name is in the catalog uses `catalog:`;
//! - `pnpm-overrides`: pnpm-workspace.yaml overrides entries new since the base.

use std::collections::HashSet;

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};
use serde_json::{Map, Value};

use crate::util::{
    Change, at, could_not_run, esc, js_str, line, line_of, list, obj, parse_yaml, regex_static,
    strip_script,
};

pub struct PmPolicy;

const ACCEPT: &str = "the user approves it, not Claude";

/// Why a mode could not run (a note).
struct Skip(String);

impl Builtin for PmPolicy {
    fn name(&self) -> &'static str {
        "pm_policy"
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        let (_, args) = strip_script(args);
        let ch = Change::new(ctx);
        let mut p = Policy {
            ch: &ch,
            rule: &ctx.rule,
            report: Report::default(),
        };
        let mode = args.first().map(String::as_str).unwrap_or("");
        let res = match mode {
            "npm-cooldown" => p.npm_cooldown(args.get(1).map(String::as_str)),
            "npm-scripts" => p.npm_scripts(),
            "pnpm-builds" => p.pnpm_builds(),
            "yarn-scripts" => p.yarn_scripts(),
            "pnpm-workspace" => p.pnpm_workspace(),
            "pnpm-catalog" => p.pnpm_catalog(),
            "pnpm-overrides" => p.pnpm_overrides(),
            other => Err(Skip(format!("unknown package manager policy {other}"))),
        };
        match res {
            Ok(()) => p.report,
            Err(Skip(why)) => {
                let mut r = Report::default();
                r.notes.push(could_not_run(&ctx.rule, why));
                r
            }
        }
    }
}

struct Policy<'a> {
    ch: &'a Change<'a>,
    rule: &'a str,
    report: Report,
}

type Res = Result<(), Skip>;

fn json(text: Option<&str>, path: &str) -> Result<Map<String, Value>, Skip> {
    let Some(text) = text else {
        return Ok(Map::new());
    };
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Ok(obj(Some(&v)).clone()),
        Err(_) => Err(Skip(format!("{path} is not valid JSON"))),
    }
}

fn yaml(text: Option<&str>, path: &str) -> Result<Map<String, Value>, Skip> {
    let Some(text) = text else {
        return Ok(Map::new());
    };
    match parse_yaml(text) {
        Ok(v) => Ok(obj(Some(&v)).clone()),
        Err(_) => Err(Skip(format!("{path} is not valid YAML"))),
    }
}

/// .npmrc: key → values (a key may be set several times; `key[]` appends).
fn npmrc(text: Option<&str>) -> Vec<(String, Vec<String>)> {
    let mut m: Vec<(String, Vec<String>)> = vec![];
    for raw in text.unwrap_or("").lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        let Some(i) = l.find('=') else { continue };
        let k = l[..i].trim();
        let k = k.strip_suffix("[]").unwrap_or(k).to_owned();
        let mut v = l[i + 1..].trim().to_owned();
        if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            v = v[1..v.len() - 1].to_owned();
        }
        match m.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1.push(v),
            None => m.push((k, vec![v])),
        }
    }
    m
}

fn all<'m>(m: &'m [(String, Vec<String>)], k: &str) -> &'m [String] {
    m.iter().find(|(x, _)| x == k).map_or(&[], |(_, v)| v)
}

fn last<'m>(m: &'m [(String, Vec<String>)], k: &str) -> Option<&'m str> {
    all(m, k).last().map(String::as_str)
}

fn re(p: &str) -> regex::Regex {
    regex::Regex::new(p).expect("regex")
}

fn parts(v: &str) -> Vec<i64> {
    v.split('.')
        .map(|x| {
            let digits: String = x.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().unwrap_or(0)
        })
        .collect()
}

fn ge(a: &str, b: &str) -> bool {
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return p > q;
        }
    }
    true
}

/// The lowest version a semver range allows, or None.
fn lowest(range: &str) -> Option<String> {
    let mut low: Option<String> = None;
    for r in range.split("||").map(str::trim) {
        let c = regex_static(r"^(?:>=|\^|~|=|v)?\s*(\d+(?:\.\d+){0,2})(?:[-+]\S*)?(?:\s|$)")
            .captures(r)?;
        let v = c[1].to_owned();
        if low.as_deref().is_none_or(|l| !ge(&v, l)) {
            low = Some(v);
        }
    }
    low
}

const SECTIONS: &[&str] = &[
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

struct Dep {
    section: &'static str,
    name: String,
    spec: String,
}

fn deps(pkg: &Map<String, Value>) -> Vec<Dep> {
    let mut out = vec![];
    for s in SECTIONS {
        for (name, spec) in obj(pkg.get(*s)) {
            if let Some(spec) = spec.as_str() {
                out.push(Dep {
                    section: s,
                    name: name.clone(),
                    spec: spec.to_owned(),
                });
            }
        }
    }
    out
}

fn dep_line(text: &str, d: &Dep) -> usize {
    line_of(
        text,
        &re(&format!(r#""{}"\s*:\s*"{}""#, esc(&d.name), esc(&d.spec))),
    )
}

impl Policy<'_> {
    fn out(&mut self, s: String) {
        self.report.findings.push(line(self.rule, s));
    }

    fn out_at(&mut self, path: &str, l: usize, s: String) {
        self.report.findings.push(at(self.rule, path, l, s));
    }

    fn manifests(&self) -> Vec<String> {
        self.ch
            .changed
            .iter()
            .filter(|p| {
                regex_static(r"(^|/)package\.json$").is_match(p)
                    && !regex_static(r"(^|/)node_modules/").is_match(p)
                    && self.ch.exists(p)
            })
            .cloned()
            .collect()
    }

    fn npm_cooldown(&mut self, seconds: Option<&str>) -> Res {
        let secs: f64 = seconds.and_then(|s| s.parse().ok()).unwrap_or(f64::NAN);
        let days = (secs / 86400.0).ceil();
        if days.is_nan() || days <= 0.0 {
            return Err(Skip(
                "the npm release cooldown check needs the cooldown in seconds".into(),
            ));
        }
        let days = days as i64;
        let rc = npmrc(self.ch.now(".npmrc").as_deref());
        let age = last(&rc, "min-release-age");
        let ok = age.is_some_and(|a| {
            regex_static(r"^\d+(\.\d+)?$").is_match(a)
                && a.parse::<f64>().unwrap_or(0.0) >= days as f64
        });
        if !ok {
            self.out(format!(".npmrc: min-release-age is {}; set min-release-age={days} (days) or more so a freshly published (possibly compromised) version is not installed. Packages you must update sooner go in min-release-age-exclude.", age.unwrap_or("unset")));
        }
        let pkg = json(self.ch.now("package.json").as_deref(), "package.json")?;
        let mut wanted: Vec<String> = vec![];
        let dev = obj(pkg.get("devEngines"));
        for e in list(dev.get("packageManager")) {
            let e = obj(Some(e));
            if e.get("name").and_then(Value::as_str) == Some("npm")
                && let Some(v) = e.get("version").and_then(Value::as_str)
            {
                wanted.push(v.to_owned());
            }
        }
        if let Some(pm) = pkg.get("packageManager").and_then(Value::as_str)
            && let Some(v) = pm.strip_prefix("npm@")
        {
            wanted.push(regex_static(r"\+.*$").replace(v, "").into_owned());
        }
        let low: Vec<Option<String>> = wanted.iter().map(|w| lowest(w)).collect();
        if low.is_empty()
            || low
                .iter()
                .any(|v| v.as_deref().is_none_or(|v| !ge(v, "11.10.0")))
        {
            let req = if wanted.is_empty() {
                "nothing".to_owned()
            } else {
                wanted.join(", ")
            };
            self.out(format!("package.json: npm is required as {req}; require npm >= 11.10 (devEngines.packageManager {{\"name\": \"npm\", \"version\": \">=11.10.0\"}}, or packageManager \"npm@11.10.0\" or later): older npm ignores min-release-age."));
        }
        Ok(())
    }

    fn npm_scripts(&mut self) -> Res {
        if self.ch.is_changed("package.json") {
            let before = json(self.ch.before("package.json").as_deref(), "package.json")?;
            let before = obj(before.get("allowScripts")).clone();
            let text = self.ch.now("package.json");
            let now = json(text.as_deref(), "package.json")?;
            let text = text.unwrap_or_default();
            for (k, v) in obj(now.get("allowScripts")) {
                let b = before.get(k);
                if *v != Value::Bool(false)
                    && b != Some(v)
                    && !(b.is_some() && b != Some(&Value::Bool(false)))
                {
                    let l = line_of(&text, &re(&format!(r#""{}"\s*:"#, esc(k))));
                    self.out_at(
                        "package.json",
                        l,
                        format!(
                            "allowScripts approves {k} (its install scripts would run); {ACCEPT}"
                        ),
                    );
                }
            }
        }
        if self.ch.is_changed(".npmrc") {
            let before = npmrc(self.ch.before(".npmrc").as_deref());
            let text = self.ch.now(".npmrc").unwrap_or_default();
            let now = npmrc(Some(&text));
            if last(&now, "ignore-scripts") == Some("false")
                && last(&before, "ignore-scripts") != Some("false")
            {
                let l = line_of(&text, regex_static(r"^\s*ignore-scripts\s*="));
                self.out_at(".npmrc", l, format!("ignore-scripts=false; {ACCEPT}"));
            }
            if last(&now, "dangerously-allow-all-scripts") == Some("true")
                && last(&before, "dangerously-allow-all-scripts") != Some("true")
            {
                let l = line_of(
                    &text,
                    regex_static(r"^\s*dangerously-allow-all-scripts\s*="),
                );
                self.out_at(".npmrc", l, format!("dangerously-allow-all-scripts=true runs every dependency's install scripts; {ACCEPT}"));
            }
            let names = |m: &[(String, Vec<String>)]| -> Vec<String> {
                let mut out: Vec<String> = vec![];
                for v in all(m, "allow-scripts") {
                    for s in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        if !out.iter().any(|x| x == s) {
                            out.push(s.to_owned());
                        }
                    }
                }
                out
            };
            let had = names(&before);
            let added: Vec<String> = names(&now)
                .into_iter()
                .filter(|n| !had.contains(n))
                .collect();
            if !added.is_empty() {
                let l = line_of(&text, regex_static(r"^\s*allow-scripts(\[\])?\s*="));
                self.out_at(
                    ".npmrc",
                    l,
                    format!(
                        "allow-scripts adds {} (their install scripts would run); {ACCEPT}",
                        added.join(", ")
                    ),
                );
            }
        }
        Ok(())
    }

    fn key_line(text: &str, key: &str) -> usize {
        line_of(text, &re(&format!(r#"^\s*["']?{}["']?\s*:"#, esc(key))))
    }

    fn pnpm_builds(&mut self) -> Res {
        let p = "pnpm-workspace.yaml";
        if !self.ch.is_changed(p) {
            return Ok(());
        }
        let before = yaml(self.ch.before(p).as_deref(), p)?;
        let text = self.ch.now(p);
        let now = yaml(text.as_deref(), p)?;
        let text = text.unwrap_or_default();
        let (ab0, ab) = (obj(before.get("allowBuilds")), obj(now.get("allowBuilds")));
        let t = Value::Bool(true);
        for (k, v) in ab {
            if *v == t && ab0.get(k) != Some(&t) {
                let l = Self::key_line(&text, k);
                self.out_at(
                    p,
                    l,
                    format!("allowBuilds approves {k} (its install scripts would run); {ACCEPT}"),
                );
            }
        }
        let ob0: HashSet<String> = list(before.get("onlyBuiltDependencies"))
            .into_iter()
            .map(js_str)
            .collect();
        for d in list(now.get("onlyBuiltDependencies"))
            .into_iter()
            .map(js_str)
        {
            if !ob0.contains(&d) {
                let l = line_of(
                    &text,
                    &re(&format!(r#"^\s*-\s*["']?{}["']?\s*(#.*)?$"#, esc(&d))),
                );
                self.out_at(
                    p,
                    l,
                    format!(
                        "onlyBuiltDependencies adds {d} (its install scripts would run); {ACCEPT}"
                    ),
                );
            }
        }
        if now.get("dangerouslyAllowAllBuilds") == Some(&t)
            && before.get("dangerouslyAllowAllBuilds") != Some(&t)
        {
            let l = Self::key_line(&text, "dangerouslyAllowAllBuilds");
            self.out_at(p, l, format!("dangerouslyAllowAllBuilds: true runs every dependency's install scripts; {ACCEPT}"));
        }
        let f = Value::Bool(false);
        for key in ["strictDepBuilds", "blockExoticSubdeps"] {
            if now.get(key) == Some(&f) && before.get(key) != Some(&f) {
                let l = Self::key_line(&text, key);
                self.out_at(p, l, format!("{key}: false; {ACCEPT}"));
            }
        }
        Ok(())
    }

    fn yarn_scripts(&mut self) -> Res {
        let t = Value::Bool(true);
        if self.ch.is_changed(".yarnrc.yml") {
            let text = self.ch.now(".yarnrc.yml");
            let now = yaml(text.as_deref(), ".yarnrc.yml")?;
            let before = yaml(self.ch.before(".yarnrc.yml").as_deref(), ".yarnrc.yml")?;
            if now.get("enableScripts") == Some(&t) && before.get("enableScripts") != Some(&t) {
                let l = line_of(
                    &text.unwrap_or_default(),
                    regex_static(r"^\s*enableScripts\s*:"),
                );
                self.out_at(
                    ".yarnrc.yml",
                    l,
                    format!(
                        "enableScripts: true runs every dependency's install scripts; {ACCEPT}"
                    ),
                );
            }
        }
        for p in self.manifests() {
            let text = self.ch.now(&p).unwrap_or_default();
            let now = json(Some(&text), &p)?;
            let before = json(self.ch.before(&p).as_deref(), &p)?;
            let bm = obj(before.get("dependenciesMeta"));
            for (k, v) in obj(now.get("dependenciesMeta")) {
                if obj(Some(v)).get("built") == Some(&t) && obj(bm.get(k)).get("built") != Some(&t)
                {
                    let l = line_of(&text, &re(&format!(r#""{}"\s*:"#, esc(k))));
                    self.out_at(&p, l, format!("dependenciesMeta lets {k} build (built: true runs its install scripts); {ACCEPT}"));
                }
            }
        }
        Ok(())
    }

    fn pnpm_workspace(&mut self) -> Res {
        let ws = yaml(
            self.ch.now("pnpm-workspace.yaml").as_deref(),
            "pnpm-workspace.yaml",
        )?;
        let globs: Vec<String> = list(ws.get("packages")).into_iter().map(js_str).collect();
        if globs.is_empty() {
            return Ok(());
        }
        let files = self
            .ch
            .ls_files(&["package.json", "*/package.json"])
            .map_err(Skip)?;
        let clean = |g: &str| -> String {
            let g = g.strip_prefix("./").unwrap_or(g);
            g.strip_suffix('/').unwrap_or(g).to_owned()
        };
        let compile = |g: &str| {
            globset::GlobBuilder::new(&clean(g))
                .literal_separator(true)
                .build()
                .map(|g| g.compile_matcher())
                .ok()
        };
        let inc: Vec<_> = globs
            .iter()
            .filter(|g| !g.starts_with('!'))
            .filter_map(|g| compile(g))
            .collect();
        let exc: Vec<_> = globs
            .iter()
            .filter_map(|g| g.strip_prefix('!'))
            .filter_map(compile)
            .collect();
        let mut names: HashSet<String> = HashSet::new();
        for f in files {
            if regex_static(r"(^|/)node_modules/").is_match(&f) {
                continue;
            }
            let dir = f.rsplit_once('/').map_or(".", |(d, _)| d);
            let out = if dir == "." {
                !globs.iter().any(|g| g == ".")
            } else {
                !inc.iter().any(|g| g.is_match(dir))
                    || exc
                        .iter()
                        .any(|g| g.is_match(dir) || g.is_match(format!("{dir}/x")))
            };
            if out {
                continue;
            }
            let pkg = json(self.ch.now(&f).as_deref(), &f)?;
            if let Some(n) = pkg.get("name").and_then(Value::as_str) {
                names.insert(n.to_owned());
            }
        }
        for p in self.manifests() {
            let text = self.ch.now(&p).unwrap_or_default();
            for d in deps(&json(Some(&text), &p)?) {
                if names.contains(&d.name) && !d.spec.contains(':') {
                    let want = if d.spec == "*" { "*" } else { "^" };
                    let l = dep_line(&text, &d);
                    self.out_at(&p, l, format!("{} is a package of this workspace: depend on it as \"workspace:{want}\" so pnpm links it instead of installing a registry copy", d.name));
                }
            }
        }
        Ok(())
    }

    fn pnpm_catalog(&mut self) -> Res {
        let ws = yaml(
            self.ch.now("pnpm-workspace.yaml").as_deref(),
            "pnpm-workspace.yaml",
        )?;
        if ws.get("catalogMode").and_then(Value::as_str) == Some("strict") {
            return Ok(());
        }
        let mut named: Vec<(String, String)> = vec![];
        for n in obj(ws.get("catalog")).keys() {
            named.push((n.clone(), "catalog:".into()));
        }
        for (c, entries) in obj(ws.get("catalogs")) {
            for n in obj(Some(entries)).keys() {
                if !named.iter().any(|(x, _)| x == n) {
                    let v = if c == "default" {
                        "catalog:".to_owned()
                    } else {
                        format!("catalog:{c}")
                    };
                    named.push((n.clone(), v));
                }
            }
        }
        if named.is_empty() {
            return Ok(());
        }
        for p in self.manifests() {
            let text = self.ch.now(&p).unwrap_or_default();
            let had: HashSet<String> = deps(&json(self.ch.before(&p).as_deref(), &p)?)
                .into_iter()
                .map(|d| format!("{}\0{}\0{}", d.section, d.name, d.spec))
                .collect();
            for d in deps(&json(Some(&text), &p)?) {
                let Some((_, use_)) = named.iter().find(|(n, _)| *n == d.name) else {
                    continue;
                };
                if !d.spec.starts_with("catalog:")
                    && !d.spec.starts_with("workspace:")
                    && !had.contains(&format!("{}\0{}\0{}", d.section, d.name, d.spec))
                {
                    let l = dep_line(&text, &d);
                    self.out_at(&p, l, format!("{} is in the pnpm catalog: use \"{use_}\" instead of \"{}\" so every package gets the same version", d.name, d.spec));
                }
            }
        }
        Ok(())
    }

    fn pnpm_overrides(&mut self) -> Res {
        let p = "pnpm-workspace.yaml";
        if !self.ch.is_changed(p) {
            return Ok(());
        }
        let text = self.ch.now(p);
        let before = yaml(self.ch.before(p).as_deref(), p)?;
        let now = yaml(text.as_deref(), p)?;
        let text = text.unwrap_or_default();
        let bo = obj(before.get("overrides"));
        for (k, v) in obj(now.get("overrides")) {
            if bo.get(k) != Some(v) {
                let l = Self::key_line(&text, k);
                self.out_at(p, l, format!("overrides {k} to {}: say why (a security fix, an incompatible transitive version); an override can hide a vulnerability or force a version its dependents do not support", js_str(v)));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(lowest(">=11.10").as_deref(), Some("11.10"));
        assert_eq!(lowest("^10.0.0 || ^11.10.0").as_deref(), Some("10.0.0"));
        assert!(lowest("latest").is_none());
        assert!(ge("11.10.0", "11.10"));
        assert!(!ge("11.9", "11.10.0"));
    }
}
