//! The JavaScript package managers' release cooldowns (`forge_npm_cooldown`,
//! `forge_pnpm_cooldown`, `forge_yarn_cooldown`, `forge_bun_cooldown`): the setting that makes
//! the package manager install only versions published at least the cooldown ago, and, where an
//! older release ignores the setting, a package manager pinned new enough to read it.
//!
//! | package manager | setting | first release that reads it |
//! |---|---|---|
//! | npm | `.npmrc` `min-release-age` (days) | 11.10 (required in package.json) |
//! | pnpm | `pnpm-workspace.yaml` `minimumReleaseAge` (minutes) | 10.16 (a pin below it is reported) |
//! | Yarn | `.yarnrc.yml` `npmMinimalAgeGate` (minutes; a duration such as `7d` from 4.11) | 4.10 (a pin below it is reported) |
//! | Bun | `bunfig.toml` `[install] minimumReleaseAge` (seconds) | |
//!
//! Like `forge_uv_cooldown` (§10.5), with `--all` every missing or weak setting is reported; in a
//! change only what the change broke: a setting its file's base copy had right, or one in a file
//! the change created. A setting the project already lacked is its setup, which `forge apply`
//! offers ([`setup`]), not the agent's to fix.

use std::sync::Arc;

use forge_core::builtin::BuiltinCtx;
use forge_core::{Finding, Report};
use serde_json::Value;

use crate::Options;
use crate::ctx::{Extra, base_copy, is_all, read};
use crate::js::duration;
use forge_check::builtins::CheckBuiltin;

/// What apply writes: a week, more than the 3-day minimum, as `forge apply` does for uv.
const SETUP_SECONDS: u64 = 604_800;

/// A setting the config lacks: its key, the file it belongs in, the finding's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub key: &'static str,
    pub file: &'static str,
    pub text: String,
}

/// The files a package manager's cooldown reads.
pub fn files(pm: &str) -> &'static [&'static str] {
    match pm {
        "npm" => &[".npmrc", "package.json"],
        "pnpm" => &["pnpm-workspace.yaml", "package.json"],
        "yarn" => &[".yarnrc.yml", "package.json"],
        "bun" => &["bunfig.toml"],
        _ => &[],
    }
}

/// What `pm`'s cooldown lacks, the files read through `read` (None: absent), for a cooldown of
/// `min` seconds.
pub fn problems(pm: &str, read: &dyn Fn(&str) -> Option<String>, min: u64) -> Vec<Problem> {
    let mut out = Vec::new();
    let pkg = read("package.json")
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    match pm {
        "npm" => {
            let days = min.div_ceil(86_400).max(1);
            let rc = npmrc(&read(".npmrc").unwrap_or_default());
            let age = rc
                .iter()
                .rev()
                .find(|(k, _)| k == "min-release-age")
                .map(|(_, v)| v.as_str());
            if age
                .and_then(|a| a.parse::<f64>().ok())
                .is_none_or(|a| a < days as f64)
            {
                out.push(Problem {
                    key: "min-release-age",
                    file: ".npmrc",
                    text: format!(
                        ".npmrc: min-release-age is {}; set min-release-age={days} (days) or more so a freshly published (possibly compromised) version is not installed. Packages you must update sooner go in min-release-age-exclude.",
                        age.unwrap_or("unset")
                    ),
                });
            }
            let wanted = pinned(&pkg, "npm");
            if wanted.is_empty()
                || wanted
                    .iter()
                    .any(|w| lowest(w).is_none_or(|v| !version_ge(&v, "11.10.0")))
            {
                let req = if wanted.is_empty() {
                    "nothing".to_owned()
                } else {
                    wanted.join(", ")
                };
                out.push(Problem {
                    key: "npm-version",
                    file: "package.json",
                    text: format!(
                        "package.json: npm is required as {req}; require npm >= 11.10 (devEngines.packageManager {{\"name\": \"npm\", \"version\": \">=11.10.0\"}}, or packageManager \"npm@11.10.0\" or later): older npm ignores min-release-age."
                    ),
                });
            }
        }
        "pnpm" | "yarn" => {
            let (file, key, first, exclude) = if pm == "pnpm" {
                (
                    "pnpm-workspace.yaml",
                    "minimumReleaseAge",
                    "10.16.0",
                    "minimumReleaseAgeExclude",
                )
            } else {
                (
                    ".yarnrc.yml",
                    "npmMinimalAgeGate",
                    "4.10.0",
                    "npmPreapprovedPackages",
                )
            };
            let want = min.div_ceil(60);
            let v = yaml_scalar(&read(file).unwrap_or_default(), key);
            let minutes = v.as_deref().and_then(|v| {
                if pm == "pnpm" {
                    v.parse::<f64>().ok()
                } else {
                    yarn_minutes(v)
                }
            });
            if minutes.is_none_or(|m| m < want as f64) {
                out.push(Problem {
                    key,
                    file,
                    text: format!(
                        "{file}: {key} is {}; set {key}: {want} (minutes, {}) or more so a freshly published (possibly compromised) version is not installed. Packages you must update sooner go in {exclude}.",
                        v.as_deref().unwrap_or("unset"),
                        duration(want * 60)
                    ),
                });
            }
            // a pin below the first release that reads the setting
            let old: Vec<String> = pinned(&pkg, pm)
                .into_iter()
                .filter(|w| lowest(w).is_some_and(|v| !version_ge(&v, first)))
                .collect();
            if !old.is_empty() {
                let name = if pm == "pnpm" { "pnpm" } else { "Yarn" };
                out.push(Problem {
                    key: "pm-version",
                    file: "package.json",
                    text: format!(
                        "package.json: {pm} is required as {}; require {pm} >= {} (packageManager \"{pm}@{first}\" or later): older {name} does not read {key}.",
                        old.join(", "),
                        first.trim_end_matches(".0")
                    ),
                });
            }
        }
        "bun" => {
            let value = read("bunfig.toml").and_then(|t| install_value(&t));
            if !matches!(&value, Some(Value::Number(n)) if n.as_u64().is_some_and(|v| v >= min)) {
                let shown = match &value {
                    None => "unset".to_string(),
                    Some(Value::String(s)) => format!("\"{s}\""),
                    Some(v) => v.to_string(),
                };
                out.push(Problem {
                    key: "minimumReleaseAge",
                    file: "bunfig.toml",
                    text: format!(
                        "bunfig.toml: [install] minimumReleaseAge is {shown}; set it to at least {min} ({}, in seconds) so a freshly published (possibly compromised) version is not installed. Packages you must update sooner go in minimumReleaseAgeExcludes.",
                        duration(min)
                    ),
                });
            }
        }
        _ => {}
    }
    out
}

/// `forge_<pm>_cooldown`.
pub struct Cooldown {
    pub opts: Arc<Options>,
    pub pm: &'static str,
    pub name: &'static str,
}

impl Cooldown {
    pub fn all(opts: &Arc<Options>) -> Vec<Cooldown> {
        [
            ("npm", "npm_cooldown"),
            ("pnpm", "pnpm_cooldown"),
            ("yarn", "yarn_cooldown"),
            ("bun", "bun_cooldown"),
        ]
        .into_iter()
        .map(|(pm, name)| Cooldown {
            opts: opts.clone(),
            pm,
            name,
        })
        .collect()
    }
}

impl CheckBuiltin for Cooldown {
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&self, ctx: &BuiltinCtx, x: &Extra, _args: &[String]) -> Report {
        // compared with the base here: its findings are the change's as they are
        x.mark_attributed();
        let min = self.opts.cooldown_seconds;
        let now = problems(self.pm, &|f| read(&ctx.root, f), min);
        let changed = |f: &str| ctx.changed.iter().any(|c| c == f);
        let created = |f: &str| changed(f) && base_copy(ctx, f).is_none();
        let before = if is_all(ctx) {
            Vec::new()
        } else {
            // the files as they were: the base copy of a changed one, else as they are
            problems(
                self.pm,
                &|f| {
                    if changed(f) {
                        base_copy(ctx, f).and_then(|p| std::fs::read_to_string(p).ok())
                    } else {
                        read(&ctx.root, f)
                    }
                },
                min,
            )
        };
        let mut r = Report::default();
        for p in now {
            if is_all(ctx) || created(p.file) || !before.iter().any(|b| b.key == p.key) {
                r.findings.push(Finding::error(&ctx.rule, p.text));
            }
        }
        r
    }
}

/// One file `forge apply` writes for the cooldown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub file: &'static str,
    /// Its new text.
    pub text: String,
    /// What was added, for the person (`min-release-age=7`).
    pub added: String,
}

/// What `forge apply` writes to give `pm` its cooldown: each file of [`files`] whose setting is
/// missing, with its new text (the file created when absent). A value the project set is never
/// changed, and a file it cannot read is left alone. A version requirement is added only where
/// package.json pins no version of the package manager. Yarn rejects a setting it does not know,
/// so its cooldown is added only where package.json pins Yarn 4.10 or later.
pub fn setup(pm: &str, read: &dyn Fn(&str) -> Option<String>) -> Vec<Setup> {
    let mut out: Vec<(&'static str, String, String)> = Vec::new();
    let minutes = SETUP_SECONDS / 60;
    match pm {
        "npm" => {
            let rc = read(".npmrc").unwrap_or_default();
            if !npmrc(&rc).iter().any(|(k, _)| k == "min-release-age") {
                let line = format!("min-release-age={}", SETUP_SECONDS / 86_400);
                out.push((".npmrc", append(&rc, &format!("{line}\n")), line));
            }
            if let Some(pkg) = read("package.json")
                && let Some(t) = with_npm_engine(&pkg)
            {
                out.push((
                    "package.json",
                    t,
                    "devEngines.packageManager npm >=11.10.0 (onFail: warn)".into(),
                ));
            }
        }
        "pnpm" | "yarn" => {
            let (file, key) = if pm == "pnpm" {
                ("pnpm-workspace.yaml", "minimumReleaseAge")
            } else {
                (".yarnrc.yml", "npmMinimalAgeGate")
            };
            let text = read(file).unwrap_or_default();
            let pkg = read("package.json")
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .unwrap_or(Value::Null);
            let yarn_reads_it = pinned(&pkg, "yarn")
                .iter()
                .any(|w| lowest(w).is_some_and(|v| version_ge(&v, "4.10.0")));
            if (pm == "pnpm" || yarn_reads_it) && !has_top_key(&text, key) {
                let line = format!("{key}: {minutes}");
                out.push((
                    file,
                    append(&text, &format!("{line} # {}\n", duration(SETUP_SECONDS))),
                    line,
                ));
            }
        }
        "bun" => {
            let text = read("bunfig.toml").unwrap_or_default();
            if let Ok(t) = text.parse::<toml::Table>()
                && t.get("install")
                    .and_then(|i| i.get("minimumReleaseAge"))
                    .is_none()
            {
                let line = format!(
                    "minimumReleaseAge = {SETUP_SECONDS} # {}",
                    duration(SETUP_SECONDS)
                );
                let header = text
                    .lines()
                    .position(|l| l.trim().replace(' ', "") == "[install]");
                let new = match header {
                    Some(i) => {
                        let mut lines: Vec<&str> = text.lines().collect();
                        lines.insert(i + 1, &line);
                        let mut s = lines.join("\n");
                        s.push('\n');
                        s
                    }
                    None if t.contains_key("install") => return Vec::new(),
                    None => {
                        let mut s = text.trim_end().to_string();
                        if !s.is_empty() {
                            s.push_str("\n\n");
                        }
                        s.push_str(&format!("[install]\n{line}\n"));
                        s
                    }
                };
                if new.parse::<toml::Table>().is_ok() {
                    out.push((
                        "bunfig.toml",
                        new,
                        format!("[install] minimumReleaseAge = {SETUP_SECONDS}"),
                    ));
                }
            }
        }
        _ => {}
    }
    out.into_iter()
        .map(|(file, text, added)| Setup { file, text, added })
        .collect()
}

/// `text` with `line` added at its end, on a line of its own.
fn append(text: &str, line: &str) -> String {
    let mut s = text.to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str(line);
    s
}

/// package.json requiring npm >= 11.10 through `devEngines.packageManager`, when it pins no npm
/// (neither `packageManager` nor `devEngines`); None otherwise, or when it is not a JSON object.
/// The requirement warns (`onFail: warn`): npm 11.10 and later read the cooldown, an older npm
/// says it is older than the project asks and still works (without the cooldown) instead of
/// refusing every install in the project.
fn with_npm_engine(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let o = v.as_object()?;
    if o.contains_key("packageManager") || o.contains_key("devEngines") {
        return None;
    }
    let open = text.find('{')?;
    let rest = &text[open + 1..];
    let entry = "\"devEngines\": {\"packageManager\": {\"name\": \"npm\", \"version\": \">=11.10.0\", \"onFail\": \"warn\"}}";
    let new = if o.is_empty() {
        let close = open + 1 + rest.find('}')?;
        format!("{}{{\n  {entry}\n}}{}", &text[..open], &text[close + 1..])
    } else {
        // the indentation of the first member, kept
        let ws: String = rest.chars().take_while(|c| c.is_whitespace()).collect();
        let indent = ws.rsplit('\n').next().unwrap_or("");
        let sep = if ws.contains('\n') {
            format!("\n{indent}")
        } else {
            " ".to_string()
        };
        format!("{}{{{sep}{entry},{}", &text[..open], rest)
    };
    serde_json::from_str::<Value>(&new).ok().map(|_| new)
}

/// `.npmrc` entries, in order (`key[]` as `key`, quotes removed).
fn npmrc(text: &str) -> Vec<(String, String)> {
    let mut m = Vec::new();
    for raw in text.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let k = k.strip_suffix("[]").unwrap_or(k).to_owned();
        let v = v.trim();
        let v = if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            &v[1..v.len() - 1]
        } else {
            v
        };
        m.push((k, v.to_owned()));
    }
    m
}

/// Whether a top-level `key:` line exists (whatever its value).
fn has_top_key(text: &str, key: &str) -> bool {
    text.lines().any(|l| {
        l.strip_prefix(key)
            .is_some_and(|r| r.trim_start().starts_with(':'))
    })
}

/// A top-level scalar of a YAML file: `key: value`, without its comment or quotes; None when
/// absent or not a scalar on the key's line.
fn yaml_scalar(text: &str, key: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|l| has_top_key(l, key))?
        .strip_prefix(key)?;
    let v = line.trim_start().strip_prefix(':')?;
    let v = v.split(" #").next().unwrap_or("").trim();
    let v = v.trim_matches(|c| c == '"' || c == '\'');
    (!v.is_empty()).then(|| v.to_string())
}

/// Yarn's `npmMinimalAgeGate` in minutes: a number of minutes, or a duration (`3d`, `72h`).
fn yarn_minutes(v: &str) -> Option<f64> {
    let unit = v.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    let n: f64 = v[..v.len() - unit.len()].parse().ok()?;
    let ms = match unit {
        "" | "m" => 60_000.0,
        "ms" => 1.0,
        "s" => 1000.0,
        "h" => 3_600_000.0,
        "d" => 86_400_000.0,
        "w" => 604_800_000.0,
        _ => return None,
    };
    Some(n * ms / 60_000.0)
}

/// The versions of `pm` that package.json requires (`devEngines.packageManager` entries named
/// `pm`, `packageManager` `pm@<version>`), as written.
fn pinned(pkg: &Value, pm: &str) -> Vec<String> {
    let mut out = Vec::new();
    let dev = pkg.pointer("/devEngines/packageManager");
    let entries: Vec<&Value> = match dev {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(v) => vec![v],
        None => vec![],
    };
    for e in entries {
        if e.get("name").and_then(Value::as_str) == Some(pm)
            && let Some(v) = e.get("version").and_then(Value::as_str)
        {
            out.push(v.to_owned());
        }
    }
    if let Some(v) = pkg
        .get("packageManager")
        .and_then(Value::as_str)
        .and_then(|s| s.strip_prefix(&format!("{pm}@")))
    {
        out.push(v.split('+').next().unwrap_or(v).to_owned());
    }
    out
}

fn parts(v: &str) -> Vec<u64> {
    v.split('.')
        .map(|x| {
            let digits: String = x.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().unwrap_or(0)
        })
        .collect()
}

fn version_ge(a: &str, b: &str) -> bool {
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
    let re = regex::Regex::new(r"^(?:>=|\^|~|=|v)?\s*(\d+(?:\.\d+){0,2})(?:[-+]\S*)?(?:\s|$)")
        .expect("regex");
    let mut low: Option<String> = None;
    for r in range.split("||").map(str::trim) {
        let v = re.captures(r)?[1].to_owned();
        if low.as_deref().is_none_or(|l| !version_ge(&v, l)) {
            low = Some(v);
        }
    }
    low
}

/// `[install] minimumReleaseAge` of a bunfig.toml (None: absent or unreadable).
fn install_value(text: &str) -> Option<Value> {
    let t: toml::Table = text.parse().ok()?;
    let v = t.get("install")?.get("minimumReleaseAge")?;
    serde_json::to_value(v).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |f| {
            files
                .iter()
                .find(|(n, _)| *n == f)
                .map(|(_, t)| t.to_string())
        }
    }

    #[test]
    fn settings_read_per_package_manager() {
        let min = 259_200;
        assert!(
            problems(
                "pnpm",
                &of(&[("pnpm-workspace.yaml", "minimumReleaseAge: 4320\n")]),
                min
            )
            .is_empty()
        );
        let p = problems(
            "pnpm",
            &of(&[("pnpm-workspace.yaml", "minimumReleaseAge: 1440 # a day\n")]),
            min,
        );
        assert_eq!(p.len(), 1);
        assert!(p[0].text.contains("is 1440"), "{p:?}");
        assert!(
            problems(
                "yarn",
                &of(&[(".yarnrc.yml", "npmMinimalAgeGate: 3d\n")]),
                min
            )
            .is_empty()
        );
        assert!(
            problems(
                "yarn",
                &of(&[(".yarnrc.yml", "npmMinimalAgeGate: \"10080\"\n")]),
                min
            )
            .is_empty()
        );
        assert_eq!(
            problems(
                "yarn",
                &of(&[(".yarnrc.yml", "npmMinimalAgeGate: 1d\n")]),
                min
            )
            .len(),
            1
        );
        // a pin below the first release that reads the setting
        let p = problems(
            "yarn",
            &of(&[
                (".yarnrc.yml", "npmMinimalAgeGate: 7d\n"),
                ("package.json", "{\"packageManager\": \"yarn@4.9.4\"}"),
            ]),
            min,
        );
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].key, "pm-version");
        assert!(
            problems(
                "bun",
                &of(&[("bunfig.toml", "[install]\nminimumReleaseAge = 259200\n")]),
                min
            )
            .is_empty()
        );
        assert!(
            problems(
                "npm",
                &of(&[
                    (".npmrc", "min-release-age=3\n"),
                    ("package.json", "{\"packageManager\": \"npm@11.10.0\"}")
                ]),
                min
            )
            .is_empty()
        );
    }

    #[test]
    fn setup_adds_only_what_is_missing() {
        let s = setup(
            "npm",
            &of(&[("package.json", "{\n  \"name\": \"shop\"\n}\n")]),
        );
        assert_eq!(
            (s[0].file, s[0].text.as_str()),
            (".npmrc", "min-release-age=7\n")
        );
        assert_eq!(
            s[1].text,
            "{\n  \"devEngines\": {\"packageManager\": {\"name\": \"npm\", \"version\": \">=11.10.0\", \"onFail\": \"warn\"}},\n  \"name\": \"shop\"\n}\n"
        );
        assert_eq!(
            setup(
                "npm",
                &of(&[("package.json", "{}"), (".npmrc", "min-release-age=1")])
            )[0]
            .text,
            "{\n  \"devEngines\": {\"packageManager\": {\"name\": \"npm\", \"version\": \">=11.10.0\", \"onFail\": \"warn\"}}\n}"
        );
        // a value the project set is kept
        assert!(
            setup(
                "npm",
                &of(&[
                    (".npmrc", "min-release-age=1\n"),
                    ("package.json", "{\"packageManager\": \"npm@10.0.0\"}")
                ])
            )
            .is_empty()
        );
        let text = |s: Vec<Setup>| s.into_iter().map(|s| (s.file, s.text)).collect::<Vec<_>>();
        assert_eq!(
            text(setup(
                "pnpm",
                &of(&[("pnpm-workspace.yaml", "packages:\n  - apps/*")])
            )),
            vec![(
                "pnpm-workspace.yaml",
                "packages:\n  - apps/*\nminimumReleaseAge: 10080 # 7 days\n".to_string()
            )]
        );
        assert_eq!(
            text(setup(
                "bun",
                &of(&[("bunfig.toml", "[install]\nexact = true\n")])
            )),
            vec![(
                "bunfig.toml",
                "[install]\nminimumReleaseAge = 604800 # 7 days\nexact = true\n".to_string()
            )]
        );
        assert_eq!(
            text(setup("bun", &of(&[]))),
            vec![(
                "bunfig.toml",
                "[install]\nminimumReleaseAge = 604800 # 7 days\n".to_string()
            )]
        );
        assert!(setup("yarn", &of(&[(".yarnrc.yml", "npmMinimalAgeGate: 1d\n")])).is_empty());
        // Yarn rejects a setting it does not know: only where Yarn 4.10 or later is pinned
        assert!(setup("yarn", &of(&[(".yarnrc.yml", "nodeLinker: pnp\n")])).is_empty());
        let pinned = [
            (".yarnrc.yml", "nodeLinker: pnp\n"),
            ("package.json", "{\"packageManager\": \"yarn@4.12.0\"}"),
        ];
        assert_eq!(
            setup("yarn", &of(&pinned))[0].text,
            "nodeLinker: pnp\nnpmMinimalAgeGate: 10080 # 7 days\n"
        );
    }
}
