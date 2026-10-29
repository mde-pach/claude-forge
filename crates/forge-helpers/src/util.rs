//! What the helpers share: the change they look at, value helpers with the semantics of the
//! languages the original helpers were written in, and small config parsers (JSONC, INI, YAML,
//! TOML) that read into `serde_json::Value`.

use std::path::{Path, PathBuf};
use std::process::Command;

use forge_core::builtin::BuiltinCtx;
use forge_core::{Finding, Note, NoteKind};
use serde_json::{Map, Value};

/// The change a helper checks: the files now (under the root) and their base copies.
pub struct Change<'a> {
    pub root: &'a Path,
    pub base: Option<&'a Path>,
    pub changed: &'a [String],
}

impl<'a> Change<'a> {
    pub fn new(ctx: &'a BuiltinCtx) -> Self {
        Change {
            root: &ctx.root,
            base: ctx.base.as_deref(),
            changed: &ctx.changed,
        }
    }

    pub fn path(&self, p: &str) -> PathBuf {
        self.root.join(p)
    }

    /// The file as it is now; None when it is absent or not a file.
    pub fn now(&self, p: &str) -> Option<String> {
        let path = self.path(p);
        if !path.is_file() {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }

    pub fn exists(&self, p: &str) -> bool {
        self.path(p).is_file()
    }

    /// The base copy of a changed file; None when it was not in the base.
    pub fn base_copy(&self, p: &str) -> Option<String> {
        let path = self.base?.join(p);
        if !path.is_file() {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }

    pub fn in_base(&self, p: &str) -> bool {
        self.base.is_some_and(|b| b.join(p).is_file())
    }

    pub fn is_changed(&self, p: &str) -> bool {
        self.changed.iter().any(|c| c == p)
    }

    /// The file as it was in the base: the file itself when it did not change.
    pub fn before(&self, p: &str) -> Option<String> {
        if !self.is_changed(p) {
            return self.now(p);
        }
        self.base_copy(p)
    }

    /// `git ls-files -co --exclude-standard <specs>` in the root.
    pub fn ls_files(&self, specs: &[&str]) -> Result<Vec<String>, String> {
        let out = Command::new("git")
            .args(["ls-files", "-co", "--exclude-standard", "--"])
            .args(specs)
            .current_dir(self.root)
            .output()
            .map_err(|e| format!("git could not run: {e}"))?;
        if !out.status.success() {
            return Err("git ls-files failed".into());
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    }
}

/// A finding at `path:line` (line 0: the file as a whole).
pub fn at(rule: &str, path: &str, line: usize, message: impl Into<String>) -> Finding {
    Finding::error(rule, message).at(path, (line > 0).then_some(line as u32))
}

/// A finding written as a whole line (`path: message`, about a file's settings).
pub fn line(rule: &str, text: impl Into<String>) -> Finding {
    Finding::error(rule, text)
}

/// A note that the helper could not do its work.
pub fn could_not_run(rule: &str, why: impl Into<String>) -> Note {
    Note::new(NoteKind::ForgeFailure, why).for_rule(rule)
}

/// A helper's arguments without the script name a pack wrote (`ratchet.py`, `pm_policy.mjs`).
pub fn strip_script(args: &[String]) -> (Option<Lang>, &[String]) {
    match args.first().map(String::as_str) {
        Some(a) if a.ends_with(".py") || a == "python" || a == "python3" => {
            (Some(Lang::Python), &args[1..])
        }
        Some(a) if a.ends_with(".mjs") || a.ends_with(".js") || a == "bun" || a == "node" => {
            (Some(Lang::Js), &args[1..])
        }
        _ => (None, args),
    }
}

/// Which original helper a call stands for, when two share a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Python,
    Js,
}

// ---- values

pub static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);

/// The object, or an empty one (`obj()` of the JS helpers, `x or {}` of the Python ones).
pub fn obj(v: Option<&Value>) -> &Map<String, Value> {
    match v {
        Some(Value::Object(m)) => m,
        _ => &EMPTY,
    }
}

pub fn get<'v>(m: &'v Map<String, Value>, k: &str) -> Option<&'v Value> {
    m.get(k).filter(|v| !v.is_null())
}

/// A value as a list: absent or null is empty, a scalar is one item.
pub fn list(v: Option<&Value>) -> Vec<&Value> {
    match v {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a.iter().collect(),
        Some(x) => vec![x],
    }
}

/// Python's `str()` of a value.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => py_num(n),
        Value::String(s) => s.clone(),
        _ => py_repr(v),
    }
}

fn py_num(n: &serde_json::Number) -> String {
    if n.is_f64() {
        let f = n.as_f64().unwrap_or_default();
        if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e16 {
            format!("{f:.1}")
        } else {
            format!("{f}")
        }
    } else {
        n.to_string()
    }
}

fn py_repr(v: &Value) -> String {
    match v {
        Value::String(s) => format!("'{s}'"),
        Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("'{k}': {}", py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => py_str(other),
    }
}

/// Python equality (`==`, used by `in`): numbers by value, booleans as 0 and 1.
pub fn py_eq(a: &Value, b: &Value) -> bool {
    match (py_number(a), py_number(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

pub fn py_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

pub fn py_in(x: &Value, items: &[&Value]) -> bool {
    items.iter().any(|i| py_eq(x, i))
}

/// JavaScript's `String()` of a value.
pub fn js_str(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => js_num(n),
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|x| if x.is_null() { String::new() } else { js_str(x) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

fn js_num(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i64),
        _ => n.to_string(),
    }
}

/// `JSON.stringify` (for comparing values).
pub fn js_json(v: &Value) -> String {
    match v {
        Value::Number(n) => js_num(n),
        Value::Array(a) => format!("[{}]", a.iter().map(js_json).collect::<Vec<_>>().join(",")),
        Value::Object(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}:{}", Value::String(k.clone()), js_json(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        other => other.to_string(),
    }
}

// ---- parsers

/// JSON with comments and trailing commas, as the original helpers read it.
pub fn parse_jsonc(text: &str) -> Result<Value, String> {
    let mut s = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' && i + 1 < b.len() {
                s.push_str(&text[i..i + 2 + utf8_extra(b[i + 1])]);
                i += 2 + utf8_extra(b[i + 1]);
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if text[i..].starts_with("//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        } else if text[i..].starts_with("/*") {
            i = text[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
            continue;
        }
        let w = 1 + utf8_extra(c);
        s.push_str(&text[i..i + w]);
        i += w;
    }
    static TRAILING: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r",(\s*[}\]])").expect("regex"));
    let s = TRAILING.replace_all(&s, "$1");
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

fn utf8_extra(lead: u8) -> usize {
    match lead {
        0xF0.. => 3,
        0xE0.. => 2,
        0xC0.. => 1,
        _ => 0,
    }
}

/// TOML as a JSON value (dates become strings).
pub fn parse_toml(text: &str) -> Result<Value, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| e.message().to_owned())?;
    Ok(toml_to_json(v))
}

fn toml_to_json(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            Value::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect())
        }
    }
}

/// YAML (1.2, as Bun.YAML reads it) as a JSON value.
pub fn parse_yaml(text: &str) -> Result<Value, String> {
    use saphyr::{LoadableYamlNode, YamlOwned};
    let docs = YamlOwned::load_from_str(text).map_err(|e| e.to_string())?;
    Ok(docs.into_iter().next().map_or(Value::Null, yaml_to_json))
}

fn yaml_to_json(y: saphyr::YamlOwned) -> Value {
    use saphyr::{ScalarOwned, YamlOwned};
    match y {
        YamlOwned::Value(s) => match s {
            ScalarOwned::Null => Value::Null,
            ScalarOwned::Boolean(b) => Value::Bool(b),
            ScalarOwned::Integer(i) => Value::from(i),
            ScalarOwned::FloatingPoint(f) => {
                serde_json::Number::from_f64(f.into_inner()).map_or(Value::Null, Value::Number)
            }
            ScalarOwned::String(s) => Value::String(s),
        },
        YamlOwned::Sequence(s) => Value::Array(s.into_iter().map(yaml_to_json).collect()),
        YamlOwned::Mapping(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| {
                    let key = match yaml_to_json(k) {
                        Value::String(s) => s,
                        other => js_str(&other),
                    };
                    (key, yaml_to_json(v))
                })
                .collect(),
        ),
        YamlOwned::Tagged(_, inner) => yaml_to_json(*inner),
        YamlOwned::Representation(s, _, _) => Value::String(s),
        _ => Value::Null,
    }
}

/// An INI file as Python's `configparser` (no interpolation) reads it: section → key → value,
/// keys lowercased, DEFAULT values inherited by every section.
#[derive(Debug, Default)]
pub struct Ini {
    pub defaults: Vec<(String, String)>,
    pub sections: Vec<(String, Vec<(String, String)>)>,
}

impl Ini {
    pub fn parse(text: &str) -> Result<Ini, String> {
        // values as configparser builds them: lists of lines, joined at the end; DEFAULT first
        type Raw = Vec<(String, Vec<String>)>;
        let mut all: Vec<(String, Raw)> = vec![("DEFAULT".to_owned(), Vec::new())];
        // the section and the key being continued
        let mut sect: Option<usize> = None;
        let mut key: Option<usize> = None;
        let mut indent_level = 0usize;
        for (n, raw) in text.lines().enumerate() {
            let stripped = raw.trim();
            if stripped.starts_with('#') || stripped.starts_with(';') {
                continue;
            }
            if stripped.is_empty() {
                if let (Some(sc), Some(k)) = (sect, key) {
                    all[sc].1[k].1.push(String::new());
                }
                continue;
            }
            let indent = raw.len() - raw.trim_start().len();
            if let (Some(sc), Some(k)) = (sect, key)
                && indent > indent_level
            {
                all[sc].1[k].1.push(stripped.to_owned());
                continue;
            }
            indent_level = indent;
            if stripped.starts_with('[') && stripped.len() > 2 && stripped[1..].contains(']') {
                let name = &stripped[1..stripped.rfind(']').unwrap_or(1)];
                key = None;
                if name == "DEFAULT" {
                    sect = Some(0);
                } else {
                    if all[1..].iter().any(|(s, _)| s == name) {
                        return Err(format!("section '{name}' already exists"));
                    }
                    all.push((name.to_owned(), Vec::new()));
                    sect = Some(all.len() - 1);
                }
                continue;
            }
            let Some(sc) = sect else {
                return Err(format!("File contains no section headers (line {})", n + 1));
            };
            let Some(pos) = stripped.find(['=', ':']) else {
                return Err(format!("Source contains parsing errors (line {})", n + 1));
            };
            let k = stripped[..pos].trim().to_lowercase();
            let v = stripped[pos + 1..].trim().to_owned();
            let entries = &mut all[sc].1;
            if entries.iter().any(|(ek, _)| *ek == k) {
                return Err(format!("option '{k}' already exists"));
            }
            entries.push((k, vec![v]));
            key = Some(entries.len() - 1);
        }
        let join = |raw: Raw| -> Vec<(String, String)> {
            raw.into_iter()
                .map(|(k, v)| (k, v.join("\n").trim_end().to_owned()))
                .collect()
        };
        let mut it = all.into_iter();
        let defaults = join(it.next().map(|d| d.1).unwrap_or_default());
        Ok(Ini {
            defaults,
            sections: it.map(|(s, e)| (s, join(e))).collect(),
        })
    }

    pub fn has_section(&self, name: &str) -> bool {
        self.sections.iter().any(|(s, _)| s == name)
    }

    /// A section's items, DEFAULT values first, as `p[section].items()` gives them.
    pub fn items(&self, name: &str) -> Vec<(String, String)> {
        let Some((_, own)) = self.sections.iter().find(|(s, _)| s == name) else {
            return vec![];
        };
        let mut out: Vec<(String, String)> = self.defaults.clone();
        for (k, v) in own {
            if let Some(e) = out.iter_mut().find(|(ek, _)| ek == k) {
                e.1 = v.clone();
            } else {
                out.push((k.clone(), v.clone()));
            }
        }
        out
    }

    pub fn get(&self, sect: &str, key: &str) -> Option<String> {
        self.items(sect)
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
}

/// `re.escape` / the JS `esc`: a literal for a regex.
pub fn esc(s: &str) -> String {
    regex::escape(s)
}

/// The first line (from 1) of `text` matching `re`, else 0.
pub fn line_of(text: &str, re: &regex::Regex) -> usize {
    text.split('\n').position(|l| re.is_match(l)).map_or(0, |i| i + 1)
}

pub fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// Seconds since the epoch of an ISO 8601 / RFC 3339 date-time (`2024-01-02T03:04:05.1Z`,
/// with an offset or none, which is read as UTC).
pub fn parse_iso8601(s: &str) -> Option<f64> {
    let re = regex_static(
        r"^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2})(?:\.(\d+))?)?)?\s*(Z|[+-]\d{2}:?\d{2})?$",
    );
    let c = re.captures(s.trim())?;
    let n = |i: usize| c.get(i).map_or(0, |m| m.as_str().parse::<i64>().unwrap_or(0));
    let days = days_from_civil(n(1), n(2), n(3));
    let mut secs = days * 86400 + n(4) * 3600 + n(5) * 60 + n(6);
    if let Some(off) = c.get(8).map(|m| m.as_str()).filter(|o| *o != "Z") {
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let digits: String = off[1..].chars().filter(char::is_ascii_digit).collect();
        let h: i64 = digits[..2].parse().unwrap_or(0);
        let m: i64 = digits[2..].parse().unwrap_or(0);
        secs -= sign * (h * 3600 + m * 60);
    }
    let frac = c
        .get(7)
        .map_or(0.0, |m| format!("0.{}", m.as_str()).parse().unwrap_or(0.0));
    Some(secs as f64 + frac)
}

/// The date (`YYYY-MM-DD`, UTC) of a time in seconds since the epoch.
pub fn date_of(secs: f64) -> String {
    let days = (secs / 86400.0).floor() as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A compiled regex, cached for the life of the process.
pub fn regex_static(pattern: &'static str) -> &'static regex::Regex {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<&'static str, &'static regex::Regex>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    cache
        .entry(pattern)
        .or_insert_with(|| Box::leak(Box::new(regex::Regex::new(pattern).expect("regex"))))
}

pub fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc() {
        let v = parse_jsonc("{\n // c\n \"a\": \"x//y\", /* b */ \"b\": [1,],\n}").unwrap();
        assert_eq!(v, serde_json::json!({"a": "x//y", "b": [1]}));
    }

    #[test]
    fn ini() {
        let i = Ini::parse(
            "[DEFAULT]\nx = 1\n[mypy]\nStrict = True\nfoo =\n  a\n  b\n# c\n[mypy-a.*,b]\nignore_errors = true\n",
        )
        .unwrap();
        assert_eq!(i.get("mypy", "strict").as_deref(), Some("True"));
        assert_eq!(i.get("mypy", "foo").as_deref(), Some("\na\nb"));
        assert_eq!(i.get("mypy", "x").as_deref(), Some("1"));
        assert!(i.has_section("mypy-a.*,b"));
        assert!(Ini::parse("x = 1\n").is_err());
    }

    #[test]
    fn dates() {
        let t = parse_iso8601("2024-03-05T10:00:00.5Z").unwrap();
        assert_eq!(date_of(t), "2024-03-05");
        assert_eq!(parse_iso8601("1970-01-02T00:00:00+01:00"), Some(82800.0));
    }

    #[test]
    fn py_and_js_strings() {
        assert_eq!(py_str(&serde_json::json!(90.0)), "90.0");
        assert_eq!(js_str(&serde_json::json!(90.0)), "90");
        assert!(py_eq(&serde_json::json!(1), &serde_json::json!(true)));
    }
}
