//! New dependencies exist on their registry (`new_deps.py`, `new_deps.mjs`).
//!
//! PyPI: for each changed pyproject.toml, the names its `[project]` dependencies and
//! optional-dependencies, `[dependency-groups]`, `[tool.uv] dev-dependencies` and
//! `[tool.poetry.*dependencies]` gain over the base copy are looked up: a name that does not
//! exist, every release yanked, and a first upload less than the cooldown ago are findings. Names
//! a private index serves (`[tool.uv.sources]` pinned to an explicit `[[tool.uv.index]]`, Poetry
//! sources of priority explicit; path, git and URL sources) are not looked up.
//!
//! npm: for each changed package.json, the names its dependency sections gain with a registry
//! spec are looked up: a name that does not exist, a deprecated version (the newest the range
//! allows), a range no version matches, and a package first published less than the cooldown
//! ago. A scope `.npmrc` maps to another registry, or every name when `.npmrc` sets another
//! default registry, is not looked up.
//!
//! A registry that does not answer is a note: nothing is reported for the names it would have
//! answered. Answers are cached for [`Options::cache_ttl`].

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use forge_core::builtin::{Builtin, BuiltinCtx};
use forge_core::{Note, NoteKind, Report};
use nodejs_semver::{Range, Version};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::Options;
use crate::util::{
    Change, Lang, at, date_of, get, list, now_secs, obj, parse_iso8601, parse_toml, regex_static,
    strip_script,
};

pub struct NewDeps {
    opts: Options,
}

impl NewDeps {
    pub fn new(opts: Options) -> Self {
        NewDeps { opts }
    }
}

impl Builtin for NewDeps {
    fn name(&self) -> &'static str {
        "new_deps"
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        let (lang, rest) = strip_script(args);
        let cooldown: u64 = rest.first().and_then(|a| a.parse().ok()).unwrap_or(0);
        let ch = Change::new(ctx);
        let reg = Registry::new(&self.opts);
        let mut r = Report::default();
        let mut unreachable = None;
        if lang != Some(Lang::Js) {
            pypi(&ch, &ctx.rule, cooldown, &reg, &mut r, &mut unreachable);
        }
        if lang != Some(Lang::Python) {
            npm(&ch, &ctx.rule, cooldown, &reg, &mut r, &mut unreachable);
        }
        if let Some(why) = unreachable {
            r.notes.push(
                Note::new(
                    NoteKind::Environment,
                    format!(
                        "new dependencies were not looked up: the registry did not answer ({why})"
                    ),
                )
                .for_rule(&ctx.rule),
            );
        }
        r
    }
}

// ---------------------------------------------------------------- registry access

/// What a lookup gave: the document, the name missing, or no answer.
enum Lookup<T> {
    Found(T),
    Missing,
    Unreachable(String),
}

struct Registry {
    agent: ureq::Agent,
    cache: Option<PathBuf>,
    ttl: f64,
    pypi: String,
    npm: String,
}

#[derive(Serialize, Deserialize)]
struct Cached<T> {
    at: f64,
    /// None: the name does not exist.
    doc: Option<T>,
}

impl Registry {
    fn new(o: &Options) -> Self {
        let agent = ureq::Agent::config_builder()
            // a registry that does not answer is given up quickly; a large document (a package
            // with thousands of versions) may take longer to arrive while data keeps coming
            .timeout_connect(Some(o.timeout))
            .timeout_recv_response(Some(o.timeout))
            .timeout_per_read(Some(o.timeout))
            .timeout_global(Some(o.timeout * 12))
            .http_status_as_error(false)
            .build()
            .into();
        Registry {
            agent,
            cache: o.cache_dir.clone(),
            ttl: o.cache_ttl.as_secs_f64(),
            pypi: o.pypi.trim_end_matches('/').to_owned(),
            npm: o.npm.trim_end_matches('/').to_owned(),
        }
    }

    fn cache_file(&self, url: &str) -> Option<PathBuf> {
        let name: String = url
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Some(
            self.cache
                .as_ref()?
                .join("registry")
                .join(format!("{name}.json")),
        )
    }

    /// GETs `url` and digests its JSON with `f`; the digest is cached.
    fn get<T: Serialize + for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        f: impl FnOnce(Value) -> T,
    ) -> Lookup<T> {
        let file = self.cache_file(url);
        if let Some(c) = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<Cached<T>>(&b).ok())
            .filter(|c| now_secs() - c.at < self.ttl)
        {
            return c.doc.map_or(Lookup::Missing, Lookup::Found);
        }
        let res = match self
            .agent
            .get(url)
            .header("Accept", "application/json")
            .call()
        {
            Ok(r) => r,
            Err(e) => return Lookup::Unreachable(e.to_string()),
        };
        let status = res.status().as_u16();
        let doc = if status == 404 {
            None
        } else if !(200..300).contains(&status) {
            return Lookup::Unreachable(format!("{url} answered {status}"));
        } else {
            let mut res = res;
            let text = match res
                .body_mut()
                .with_config()
                .limit(256 * 1024 * 1024)
                .read_to_string()
            {
                Ok(t) => t,
                Err(e) => return Lookup::Unreachable(e.to_string()),
            };
            match serde_json::from_str::<Value>(&text) {
                Ok(v) => Some(f(v)),
                Err(_) => {
                    return Lookup::Unreachable(format!(
                        "{url} answered something that is not JSON"
                    ));
                }
            }
        };
        if let Some(file) = file {
            let c = Cached {
                at: now_secs(),
                doc,
            };
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(bytes) = serde_json::to_vec(&c) {
                let tmp = file.with_extension("tmp");
                if std::fs::write(&tmp, bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, &file);
                }
            }
            return c.doc.map_or(Lookup::Missing, Lookup::Found);
        }
        doc.map_or(Lookup::Missing, Lookup::Found)
    }
}

// ---------------------------------------------------------------- PyPI

#[derive(Serialize, Deserialize)]
struct PypiFacts {
    files: usize,
    all_yanked: bool,
    first_upload: Option<f64>,
}

fn pypi_facts(doc: Value) -> PypiFacts {
    let mut files = 0;
    let mut yanked = 0;
    let mut first: Option<f64> = None;
    for rel in obj(doc.get("releases")).values() {
        for f in list(Some(rel)) {
            files += 1;
            let f = obj(Some(f));
            if f.get("yanked").is_some_and(|y| {
                y.as_bool().unwrap_or(false) || y.as_str().is_some_and(|s| !s.is_empty())
            }) {
                yanked += 1;
            }
            let up = get(f, "upload_time_iso_8601")
                .or_else(|| get(f, "upload_time"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            if let Some(t) = up.and_then(parse_iso8601) {
                first = Some(first.map_or(t, |x: f64| x.min(t)));
            }
        }
    }
    PypiFacts {
        files,
        all_yanked: files > 0 && yanked == files,
        first_upload: first,
    }
}

pub fn norm(n: &str) -> String {
    regex_static(r"[-_.]+").replace_all(n, "-").to_lowercase()
}

fn poetry_tables(poetry: &Map<String, Value>) -> Vec<&Map<String, Value>> {
    let mut t = vec![
        obj(get(poetry, "dependencies")),
        obj(get(poetry, "dev-dependencies")),
    ];
    for g in obj(get(poetry, "group")).values() {
        if let Value::Object(g) = g {
            t.push(obj(get(g, "dependencies")));
        }
    }
    t
}

/// Normalized names a private index (or a source that is not an index) serves.
fn private(doc: &Map<String, Value>) -> HashSet<String> {
    let mut out = HashSet::new();
    let tool = obj(get(doc, "tool"));
    let uv = obj(get(tool, "uv"));
    let explicit: Vec<&Value> = list(get(uv, "index"))
        .into_iter()
        .filter_map(|i| {
            let i = i.as_object()?;
            i.get("explicit")
                .is_some_and(|e| e.as_bool().unwrap_or(false))
                .then(|| i.get("name"))
                .flatten()
        })
        .collect();
    for (name, src) in obj(get(uv, "sources")) {
        for s in list(Some(src)) {
            if let Value::Object(s) = s
                && (s.get("index").is_some_and(|i| explicit.contains(&i))
                    || ["path", "git", "url", "workspace"]
                        .iter()
                        .any(|k| s.contains_key(*k)))
            {
                out.insert(norm(name));
            }
        }
    }
    let poetry = obj(get(tool, "poetry"));
    let psrc: Vec<&Value> = list(get(poetry, "source"))
        .into_iter()
        .filter_map(|s| {
            let s = s.as_object()?;
            (s.get("priority").and_then(Value::as_str) == Some("explicit"))
                .then(|| s.get("name"))
                .flatten()
        })
        .collect();
    for table in poetry_tables(poetry) {
        for (name, spec) in table {
            for s in list(Some(spec)) {
                if let Value::Object(s) = s
                    && (s.get("source").is_some_and(|x| psrc.contains(&x))
                        || ["path", "git", "url"].iter().any(|k| s.contains_key(*k)))
                {
                    out.insert(norm(name));
                }
            }
        }
    }
    out
}

/// The dependency names of a pyproject.toml, sorted.
fn py_deps(doc: Option<&Map<String, Value>>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(doc) = doc else { return out };
    let project = obj(get(doc, "project"));
    let mut strings: Vec<&Value> = list(get(project, "dependencies"));
    for v in obj(get(project, "optional-dependencies")).values() {
        strings.extend(list(Some(v)));
    }
    for v in obj(get(doc, "dependency-groups")).values() {
        strings.extend(list(Some(v)).into_iter().filter(|x| x.is_string()));
    }
    let tool = obj(get(doc, "tool"));
    strings.extend(list(get(obj(get(tool, "uv")), "dev-dependencies")));
    let name_re = regex_static(r"^\s*([A-Za-z0-9][A-Za-z0-9._-]*)");
    for s in strings {
        let Some(s) = s.as_str() else { continue };
        if s.split(';').next().unwrap_or("").contains('@') {
            continue;
        }
        if let Some(c) = name_re.captures(s) {
            out.insert(c[1].to_owned());
        }
    }
    for table in poetry_tables(obj(get(tool, "poetry"))) {
        out.extend(table.keys().filter(|k| norm(k) != "python").cloned());
    }
    out
}

/// Python's `round()`: halves to even.
fn round_even(x: f64) -> i64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        (r - x.signum()) as i64
    } else {
        r as i64
    }
}

fn pypi(
    ch: &Change,
    rule: &str,
    cooldown: u64,
    reg: &Registry,
    r: &mut Report,
    unreachable: &mut Option<String>,
) {
    let re = regex_static(r"(^|/)pyproject\.toml$");
    for path in ch.changed {
        if !re.is_match(path) {
            continue;
        }
        let Some(text) = ch.now(path) else { continue };
        let Ok(Value::Object(now)) = parse_toml(&text) else {
            continue;
        };
        let base = ch.base_copy(path).and_then(|t| parse_toml(&t).ok());
        let then: HashSet<String> = py_deps(base.as_ref().and_then(Value::as_object))
            .iter()
            .map(|n| norm(n))
            .collect();
        let skip = private(&now);
        let lines: Vec<&str> = text.split('\n').collect();
        for name in py_deps(Some(&now)) {
            if then.contains(&norm(&name)) || skip.contains(&norm(&name)) {
                continue;
            }
            let pat = regex::Regex::new(&format!(
                r#"(?i)(^|["'\s]){}(["'\s\[<>=!~;,]|$)"#,
                regex::escape(&name)
            ))
            .expect("regex");
            let line = lines
                .iter()
                .position(|l| pat.is_match(l))
                .map_or(0, |i| i + 1);
            let url = format!("{}/pypi/{name}/json", reg.pypi);
            match reg.get(&url, pypi_facts) {
                Lookup::Unreachable(e) => *unreachable = Some(e),
                Lookup::Missing => r.findings.push(at(
                    rule,
                    path,
                    line,
                    format!("{name} does not exist on PyPI; check the name (a hallucinated or typo-squatted package?)"),
                )),
                Lookup::Found(f) => {
                    if f.all_yanked {
                        r.findings.push(at(
                            rule,
                            path,
                            line,
                            format!("{name}: every release on PyPI is yanked"),
                        ));
                    }
                    if let Some(first) = f.first_upload
                        && cooldown > 0
                        && now_secs() - first < cooldown as f64
                    {
                        r.findings.push(at(
                            rule,
                            path,
                            line,
                            format!(
                                "{name} was first uploaded {}, less than {} days ago; a new package needs the user's review before it is used",
                                date_of(first),
                                round_even(cooldown as f64 / 86400.0)
                            ),
                        ));
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- npm

const SECTIONS: &[&str] = &[
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

#[derive(Serialize, Deserialize)]
struct NpmFacts {
    /// version → deprecation message
    versions: Vec<(String, Option<String>)>,
    tags: Vec<(String, String)>,
    created: Option<f64>,
}

fn npm_facts(doc: Value) -> NpmFacts {
    let versions = obj(doc.get("versions"))
        .iter()
        .map(|(v, m)| {
            let dep = obj(Some(m))
                .get("deprecated")
                .and_then(Value::as_str)
                .map(str::to_owned);
            (v.clone(), dep)
        })
        .collect();
    let tags = obj(doc.get("dist-tags"))
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
        .collect();
    let created = obj(doc.get("time"))
        .get("created")
        .and_then(Value::as_str)
        .and_then(parse_iso8601);
    NpmFacts {
        versions,
        tags,
        created,
    }
}

/// The dependencies of a package.json: name → spec, first section first.
pub fn npm_deps(text: Option<&str>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let Some(Ok(Value::Object(pkg))) = text.map(serde_json::from_str::<Value>) else {
        return out;
    };
    for s in SECTIONS {
        for (k, v) in obj(pkg.get(*s)) {
            if let Some(v) = v.as_str()
                && !out.iter().any(|(n, _)| n == k)
            {
                out.push((k.clone(), v.to_owned()));
            }
        }
    }
    out
}

/// The registry name and range of a spec, or None when it is not a registry spec.
fn registry_spec(name: &str, spec: &str) -> Option<(String, String)> {
    let s = spec.trim();
    if s.starts_with("npm:") {
        let c = regex_static(r"^npm:((?:@[^/@]+/)?[^@]+)(?:@(.*))?$").captures(s)?;
        let range = c
            .get(2)
            .map(|m| m.as_str())
            .filter(|r| !r.is_empty())
            .unwrap_or("*");
        return Some((c[1].to_owned(), range.to_owned()));
    }
    if regex_static(
        r"^(workspace|file|link|portal|patch|catalog|git|git\+[a-z]+|github|gitlab|bitbucket|https?):",
    )
    .is_match(s)
        || regex_static(r"^[^@\s]+/\S+$").is_match(s)
        || s.starts_with('.')
        || s.starts_with('/')
    {
        return None;
    }
    Some((
        name.to_owned(),
        if s.is_empty() { "*" } else { s }.to_owned(),
    ))
}

struct Registries {
    default: Option<String>,
    scopes: Vec<(String, String)>,
}

fn registries(ch: &Change, dir: &str) -> Registries {
    let mut out = Registries {
        default: None,
        scopes: vec![],
    };
    let local = if dir.is_empty() || dir == "." {
        ".npmrc".to_owned()
    } else {
        format!("{dir}/.npmrc")
    };
    for f in [local.as_str(), ".npmrc"] {
        let Some(t) = ch.now(f) else { continue };
        for raw in t.lines() {
            let l = regex_static(r"[#;].*$").replace(raw, "");
            let l = l.trim();
            if let Some(c) = regex_static(r"^(@[^:\s]+):registry\s*=\s*(\S+)").captures(l)
                && !out.scopes.iter().any(|(s, _)| s == &c[1])
            {
                out.scopes.push((c[1].to_owned(), c[2].to_owned()));
            }
            if let Some(c) = regex_static(r"^registry\s*=\s*(\S+)").captures(l)
                && out.default.is_none()
            {
                out.default = Some(c[1].to_owned());
            }
        }
    }
    out
}

fn is_npmjs(url: &str) -> bool {
    regex_static(r"^https?://registry\.npmjs\.(org|com)/?$").is_match(url)
        || regex_static(r"^https?://registry\.yarnpkg\.com/?$").is_match(url)
}

/// `Bun.semver.satisfies`.
pub fn satisfies(version: &str, range: &str) -> bool {
    match (Version::parse(version), Range::parse(range)) {
        (Ok(v), Ok(r)) => r.satisfies(&v),
        _ => false,
    }
}

/// The newest version the range allows, releases first.
fn newest<'v>(versions: &'v [(String, Option<String>)], range: &str) -> Option<&'v str> {
    let pick = |pre: bool| {
        versions
            .iter()
            .filter(|(v, _)| (pre || !v.contains('-')) && satisfies(v, range))
            .filter_map(|(v, _)| Some((Version::parse(v).ok()?, v.as_str())))
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|x| x.1)
    };
    pick(false).or_else(|| pick(true))
}

fn npm(
    ch: &Change,
    rule: &str,
    cooldown: u64,
    reg: &Registry,
    r: &mut Report,
    unreachable: &mut Option<String>,
) {
    for file in ch.changed {
        if !regex_static(r"(^|/)package\.json$").is_match(file)
            || regex_static(r"(^|/)node_modules/").is_match(file)
        {
            continue;
        }
        let Some(text) = ch.now(file) else { continue };
        let now = npm_deps(Some(&text));
        let base = ch.base_copy(file);
        let then = npm_deps(base.as_deref());
        let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
        let regs = registries(ch, dir);
        let lines: Vec<&str> = text.split('\n').collect();
        for (key, spec) in &now {
            if then.iter().any(|(k, _)| k == key) {
                continue;
            }
            let Some((name, range)) = registry_spec(key, spec) else {
                continue;
            };
            let scope = name
                .starts_with('@')
                .then(|| name.split('/').next().unwrap_or(""));
            let reg_url = scope
                .and_then(|s| {
                    regs.scopes
                        .iter()
                        .find(|(k, _)| k == s)
                        .map(|(_, v)| v.clone())
                })
                .or_else(|| regs.default.clone());
            if reg_url.as_deref().is_some_and(|u| !is_npmjs(u)) {
                continue;
            }
            let quoted = format!("\"{key}\"");
            let line = lines
                .iter()
                .position(|l| l.contains(&quoted))
                .map_or(0, |i| i + 1);
            let url = format!("{}/{}", reg.npm, name.replacen('/', "%2f", 1));
            let facts = match reg.get(&url, npm_facts) {
                Lookup::Unreachable(e) => {
                    *unreachable = Some(e);
                    continue;
                }
                Lookup::Missing => {
                    r.findings.push(at(
                        rule,
                        file,
                        line,
                        format!("{name} does not exist on the npm registry; check the name (a hallucinated or typo-squatted package?)"),
                    ));
                    continue;
                }
                Lookup::Found(f) => f,
            };
            let v = facts
                .tags
                .iter()
                .find(|(t, _)| *t == range)
                .map(|(_, v)| v.as_str())
                .or_else(|| newest(&facts.versions, &range));
            if let Some(v) = v
                && let Some((_, Some(dep))) = facts.versions.iter().find(|(x, _)| x == v)
                && !dep.is_empty()
            {
                let dep = dep.split_whitespace().collect::<Vec<_>>().join(" ");
                r.findings.push(at(
                    rule,
                    file,
                    line,
                    format!("{name}@{v} is deprecated: {dep}"),
                ));
            }
            if v.is_none() && !facts.versions.is_empty() {
                r.findings.push(at(
                    rule,
                    file,
                    line,
                    format!("{name}: no published version matches {range}"),
                ));
            }
            if let Some(created) = facts.created
                && cooldown > 0
                && now_secs() - created < cooldown as f64
            {
                r.findings.push(at(
                    rule,
                    file,
                    line,
                    format!(
                        "{name} was first published {}, less than {} days ago; a new package needs the user's review before it is used",
                        date_of(created),
                        (cooldown as f64 / 86400.0).round() as i64
                    ),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs() {
        assert_eq!(
            registry_spec("x", "npm:@a/b@^1"),
            Some(("@a/b".into(), "^1".into()))
        );
        assert_eq!(registry_spec("x", "workspace:*"), None);
        assert_eq!(registry_spec("x", "user/repo"), None);
        assert_eq!(registry_spec("x", ""), Some(("x".into(), "*".into())));
        assert!(satisfies("1.2.3", "^1.0.0"));
        assert_eq!(round_even(2.5), 2);
        assert_eq!(round_even(3.5), 4);
    }

    #[test]
    fn deps_of_pyproject() {
        let doc = parse_toml(
            "[project]\ndependencies = [\"requests>=2\", \"x @ https://e/x.whl\"]\n[tool.poetry.dependencies]\npython = \"^3.11\"\nflask = \"*\"\n",
        )
        .unwrap();
        let d = py_deps(doc.as_object());
        assert_eq!(d.into_iter().collect::<Vec<_>>(), ["flask", "requests"]);
    }
}
