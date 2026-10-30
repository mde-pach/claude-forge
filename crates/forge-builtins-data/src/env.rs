//! Environment variables (`env.sh`, and `forge_env_files` of `repository.sh`): the .env template
//! lists what the settings code reads; dotenv files stay out of the repository.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use forge_core::Report;

use crate::Run;
use crate::util::{dirname, join, re};

/// One settings field: its line, environment variable, whether it has no default, its class.
struct Field {
    line: usize,
    key: String,
    required: bool,
    class: String,
}

fn class_name(line: &str) -> String {
    re(r"^class[ \t]+([A-Za-z0-9_]*)")
        .captures(line)
        .map(|c| c[1].to_owned())
        .unwrap_or_default()
}

/// `forge_env_fields_py`: the fields of each pydantic-settings BaseSettings subclass
/// (env_prefix, alias, case_sensitive applied). Fields with AliasChoices / AliasPath are left
/// out.
fn fields_py(text: &str) -> Vec<Field> {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    // the class bodies' lines: (index, class, indent, body indent)
    let mut body_lines = vec![];
    let mut cls = String::new();
    let mut body: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if re(r"^class[ \t]").is_match(line) {
            cls = if re(r"\(.*BaseSettings.*\)[ \t]*:").is_match(line) {
                class_name(line)
            } else {
                String::new()
            };
            body = None;
            continue;
        }
        if re(r"^[^ \t#]").is_match(line) {
            cls.clear();
            continue;
        }
        if cls.is_empty() || re(r"^[ \t]*(#|$)").is_match(line) {
            continue;
        }
        let ind = line.len() - line.trim_start_matches([' ', '\t']).len();
        let b = *body.get_or_insert(ind);
        body_lines.push((i, cls.clone(), ind, b));
    }
    let mut prefix: BTreeMap<String, String> = BTreeMap::new();
    let mut sensitive: HashSet<String> = HashSet::new();
    for (i, cls, _, _) in &body_lines {
        let line = lines[*i];
        if let Some(c) = re(r#"env_prefix[ \t]*=[ \t]*["']([^"']*)["']"#).captures(line) {
            prefix.insert(cls.clone(), c[1].to_owned());
        }
        if re(r"case_sensitive[ \t]*=[ \t]*True").is_match(line) {
            sensitive.insert(cls.clone());
        }
    }
    let mut out = vec![];
    for (i, cls, ind, b) in body_lines {
        let line = lines[i];
        if ind != b {
            continue;
        }
        let Some(c) = re(r"^[ \t]*([A-Za-z_][A-Za-z0-9_]*)[ \t]*:(.*)$").captures(line) else {
            continue;
        };
        let (name, rest) = (&c[1], &c[2]);
        if name == "model_config" || re(r"ClassVar|AliasChoices|AliasPath").is_match(rest) {
            continue;
        }
        let mut key = format!("{}{name}", prefix.get(&cls).map_or("", |p| p.as_str()));
        if let Some(a) =
            re(r#"(?:validation_alias|alias)[ \t]*=[ \t]*["']([^"']+)["']"#).captures(rest)
        {
            key = a[1].to_owned();
        }
        if !sensitive.contains(&cls) {
            key = key.to_uppercase();
        }
        let mut required = true;
        if let Some(m) = re(r"[ \t]=(?:[ \t]|$)").find(rest) {
            let v = rest[m.end()..].trim_start_matches([' ', '\t']);
            required = false;
            if re(r"^(?:[A-Za-z_]+\.)?Field\(").is_match(v) {
                let v = re(r"^[^(]*\([ \t]*").replace(v, "");
                if re(r"^(?:\.\.\.|\))").is_match(&v)
                    || (re(r"^[a-z_]+[ \t]*=").is_match(&v)
                        && !re(r"default(?:_factory)?[ \t]*=").is_match(&v))
                {
                    required = true;
                }
            }
        }
        out.push(Field {
            line: i + 1,
            key,
            required,
            class: cls,
        });
    }
    out
}

/// `forge_env_fields_ts`: the keys of a Zod env schema (t3-env createEnv, or z.object(...)
/// parsed from process.env): UPPER_CASE keys whose value is a z. chain on the same line;
/// required = no .default(), .optional() or .catch().
fn fields_ts(text: &str) -> Vec<Field> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let c = re(r#"^[ \t]*["']?([A-Z][A-Z0-9_]*)["']?[ \t]*:[ \t]*z\."#).captures(l)?;
            Some(Field {
                line: i + 1,
                key: c[1].to_owned(),
                required: !re(r"\.(?:default|optional|catch)\(").is_match(l),
                class: "env".into(),
            })
        })
        .collect()
}

/// The .env templates of a directory, by name.
fn templates_in(run: &Run<'_>, dir: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(run.path(dir)) else {
        return vec![];
    };
    let mut v: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| re(r"^\.env(\.[A-Za-z0-9_-]+)?\.(example|template|sample|dist)$").is_match(n))
        .collect();
    v.sort();
    v
}

/// `forge_env_example <py|ts>`: not with `--all`. A settings field or env schema key added in
/// the change appears in the .env template of the nearest directory that has one: always when
/// it has no default, and also with a default when the template already lists at least half of
/// that class's variables. A template that lists none of them is not about it.
pub fn env_example(run: &Run<'_>, args: &[String]) -> Report {
    let mut r = Report::default();
    if run.all() {
        return r;
    }
    let lang = args.first().map(String::as_str).unwrap_or("py");
    for f in run.files() {
        let Some(text) = run.read(f) else { continue };
        let fits = match lang {
            "ts" => {
                (text.contains("createEnv(") || text.contains("process.env")) && text.contains("z.")
            }
            _ => text.contains("BaseSettings"),
        };
        if !fits {
            continue;
        }
        let mut d = dirname(f);
        let names = loop {
            let t = templates_in(run, &d);
            if !t.is_empty() || d == "." {
                break t;
            }
            d = dirname(&d);
        };
        if names.is_empty() {
            continue;
        }
        let paths: Vec<String> = names.iter().map(|n| join(&d, n)).collect();
        let mut have: BTreeSet<String> = BTreeSet::new();
        for p in &paths {
            for l in run.read(p).unwrap_or_default().lines() {
                if let Some(c) =
                    re(r"^[ \t]*(?:export[ \t]+)?#?[ \t]*([A-Za-z_][A-Za-z0-9_]*)[ \t]*=")
                        .captures(l)
                {
                    have.insert(c[1].to_uppercase());
                }
            }
        }
        let found = |k: &str| {
            let k = k.to_uppercase();
            have.contains(&k) || have.iter().any(|h| h.starts_with(&format!("{k}_")))
        };
        let fields = if lang == "ts" {
            fields_ts(&text)
        } else {
            fields_py(&text)
        };
        let mut tot: BTreeMap<&str, usize> = BTreeMap::new();
        let mut got: BTreeMap<&str, usize> = BTreeMap::new();
        for fl in &fields {
            *tot.entry(&fl.class).or_default() += 1;
            if found(&fl.key) {
                *got.entry(&fl.class).or_default() += 1;
            }
        }
        let shown = paths.join(", ");
        for fl in &fields {
            let g = got.get(fl.class.as_str()).copied().unwrap_or(0);
            if g == 0 || found(&fl.key) {
                continue;
            }
            if fl.required || g * 2 >= tot[fl.class.as_str()] {
                r.findings.push(run.at(
                    f,
                    Some(fl.line as u32),
                    format!(
                        "{} is not in {shown}; add it there (a placeholder value is fine) so the next person to set up the project knows it exists",
                        fl.key
                    ),
                ));
            }
        }
    }
    r
}
