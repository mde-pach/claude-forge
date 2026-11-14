//! The test setup `forge apply` offers (§3.5), as edits of the project's own config files: pytest's
//! settings (in the file pytest reads them from), Vitest's config and bunfig.toml's `[test]`.
//! Each edit only adds: a value the project set is never changed, and a file whose shape is not
//! the one these edits understand is left alone (`None`).

use std::collections::BTreeMap;

use regex::Regex;

/// The file pytest reads its settings from, as pytest picks it: the first of pytest.ini,
/// .pytest.ini, pyproject.toml with `[tool.pytest.ini_options]` or `[tool.pytest]` (pytest 9's
/// native table), tox.ini with `[pytest]`, setup.cfg with `[tool:pytest]`; else pyproject.toml,
/// where the settings are added in a new `[tool.pytest.ini_options]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PytestFile {
    Ini {
        file: &'static str,
        section: &'static str,
    },
    Pyproject {
        /// `[tool.pytest]` (lists and booleans as such) rather than `[tool.pytest.ini_options]`.
        native: bool,
    },
    /// pyproject.toml without pytest settings yet.
    NewPyproject,
}

impl PytestFile {
    pub fn path(self) -> &'static str {
        match self {
            PytestFile::Ini { file, .. } => file,
            _ => "pyproject.toml",
        }
    }
}

/// pytest's settings as the project has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PytestConfig {
    pub file: PytestFile,
    /// `addopts`, one token each.
    pub addopts: Vec<String>,
    /// The other scalar settings, as text (`xfail_strict` → `true`).
    pub keys: BTreeMap<String, String>,
    pub filterwarnings: Vec<String>,
}

impl PytestConfig {
    /// Whether `addopts` has the option `name` (`--allow-hosts` matches `--allow-hosts=…`).
    pub fn has_opt(&self, name: &str) -> bool {
        self.addopts
            .iter()
            .any(|t| t.split('=').next() == Some(name))
    }

    /// Whether `addopts` turns the plugin off (`-p no:<plugin>`, `-pno:<plugin>`).
    pub fn disables(&self, plugin: &str) -> bool {
        let off = format!("no:{plugin}");
        self.addopts.iter().enumerate().any(|(i, t)| {
            t == &format!("-p{off}")
                || (t == "-p" && self.addopts.get(i + 1).is_some_and(|n| n == &off))
        })
    }

    /// Whether a setting is set (to anything).
    pub fn has_key(&self, key: &str) -> bool {
        self.keys.contains_key(key)
    }

    fn key_true(&self, key: &str) -> bool {
        self.keys
            .get(key)
            .is_some_and(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
    }

    /// Unknown markers are errors: `--strict-markers` (or `--strict`), or pytest 9's `strict` /
    /// `strict_markers`.
    pub fn strict_markers(&self) -> bool {
        self.has_opt("--strict-markers")
            || self.has_opt("--strict")
            || self.key_true("strict")
            || self.has_key("strict_markers")
    }

    /// Unknown settings are errors: `--strict-config`, or pytest 9's `strict` / `strict_config`.
    pub fn strict_config(&self) -> bool {
        self.has_opt("--strict-config") || self.key_true("strict") || self.has_key("strict_config")
    }

    /// An xfail test that passes fails: `xfail_strict` set either way (the project's choice),
    /// or pytest 9's `strict` / `strict_xfail`.
    pub fn strict_xfail(&self) -> bool {
        self.has_key("xfail_strict") || self.has_key("strict_xfail") || self.key_true("strict")
    }

    /// Whether filterwarnings has `entry` or a bare `error`.
    pub fn filters(&self, entry: &str) -> bool {
        let e = entry.replace(' ', "");
        self.filterwarnings
            .iter()
            .any(|f| f.replace(' ', "") == e || f.trim() == "error")
    }
}

/// A setting's value as Forge adds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Bool(bool),
    Int(i64),
}

impl Setting {
    fn text(self) -> String {
        match self {
            Setting::Bool(b) => b.to_string(),
            Setting::Int(n) => n.to_string(),
        }
    }
}

/// What to add to pytest's settings; each item is added as it is (the caller leaves out what the
/// project has).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PytestSetup {
    pub addopts: Vec<String>,
    pub keys: Vec<(&'static str, Setting)>,
    pub filterwarnings: Vec<String>,
}

impl PytestSetup {
    pub fn is_empty(&self) -> bool {
        self.addopts.is_empty() && self.keys.is_empty() && self.filterwarnings.is_empty()
    }
}

/// pytest's settings, from the file pytest reads; `None`: no such file and no pyproject.toml, or
/// a file that does not parse.
pub fn pytest_config(read: &dyn Fn(&str) -> Option<String>) -> Option<PytestConfig> {
    for file in ["pytest.ini", ".pytest.ini"] {
        if let Some(t) = read(file) {
            return ini_config(&t, file, "pytest");
        }
    }
    if let Some(t) = read("pyproject.toml") {
        let doc: toml::Table = t.parse().ok()?;
        let pytest = doc
            .get("tool")
            .and_then(|t| t.get("pytest"))
            .and_then(toml::Value::as_table);
        if let Some(pytest) = pytest {
            let (native, opts) = match pytest.get("ini_options").and_then(toml::Value::as_table) {
                Some(o) => (false, o),
                None => (true, pytest),
            };
            return Some(toml_config(opts, native));
        }
    }
    for (file, section) in [("tox.ini", "pytest"), ("setup.cfg", "tool:pytest")] {
        if let Some(t) = read(file)
            && section_range(&t, section).is_some()
        {
            return ini_config(&t, file, section);
        }
    }
    read("pyproject.toml").map(|_| PytestConfig {
        file: PytestFile::NewPyproject,
        addopts: Vec::new(),
        keys: BTreeMap::new(),
        filterwarnings: Vec::new(),
    })
}

fn toml_config(opts: &toml::Table, native: bool) -> PytestConfig {
    let lines = |v: Option<&toml::Value>, split: fn(&str) -> Vec<String>| -> Vec<String> {
        match v {
            Some(toml::Value::Array(a)) => a
                .iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect(),
            Some(toml::Value::String(s)) => split(s),
            _ => Vec::new(),
        }
    };
    let words = |s: &str| s.split_whitespace().map(str::to_owned).collect();
    let rows = |s: &str| {
        s.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let addopts = match opts.get("addopts") {
        // a list's items are single arguments, but one may hold several ("-p no:x")
        Some(toml::Value::Array(a)) => a
            .iter()
            .filter_map(toml::Value::as_str)
            .flat_map(str::split_whitespace)
            .map(str::to_owned)
            .collect(),
        v => lines(v, words),
    };
    let keys = opts
        .iter()
        .filter(|(k, _)| *k != "addopts" && *k != "filterwarnings")
        .filter_map(|(k, v)| {
            let text = match v {
                toml::Value::String(s) => s.clone(),
                toml::Value::Boolean(b) => b.to_string(),
                toml::Value::Integer(n) => n.to_string(),
                toml::Value::Float(f) => f.to_string(),
                _ => return None,
            };
            Some((k.clone(), text))
        })
        .collect();
    PytestConfig {
        file: PytestFile::Pyproject { native },
        addopts,
        keys,
        filterwarnings: lines(opts.get("filterwarnings"), rows),
    }
}

/// The lines of an INI section: (header line, end line exclusive).
fn section_range(text: &str, section: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let head = lines
        .iter()
        .position(|l| l.trim() == format!("[{section}]"))?;
    let end = lines[head + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| head + 1 + i);
    Some((head, end))
}

/// Each key of an INI section: (key, first line, last line of its value).
fn ini_keys(lines: &[&str], head: usize, end: usize) -> Vec<(String, usize, usize)> {
    let key = Regex::new(r"^([A-Za-z0-9_.-]+)\s*[=:]").expect("regex");
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    for (i, l) in lines.iter().enumerate().take(end).skip(head + 1) {
        if let Some(c) = key.captures(l) {
            out.push((c[1].to_string(), i, i));
        } else if (l.starts_with(' ') || l.starts_with('\t'))
            && !l.trim().is_empty()
            && let Some(last) = out.last_mut()
            && last.2 + 1 == i
        {
            last.2 = i;
        }
    }
    out
}

fn ini_value(lines: &[&str], first: usize, last: usize) -> String {
    let head = lines[first]
        .split_once(['=', ':'])
        .map_or("", |(_, v)| v)
        .trim();
    let mut parts = vec![head.to_string()];
    parts.extend(lines[first + 1..=last].iter().map(|l| l.trim().to_string()));
    parts.retain(|p| !p.is_empty());
    parts.join("\n")
}

fn ini_config(text: &str, file: &'static str, section: &'static str) -> Option<PytestConfig> {
    let lines: Vec<&str> = text.lines().collect();
    let (head, end) = section_range(text, section).unwrap_or((usize::MAX, 0));
    let mut c = PytestConfig {
        file: PytestFile::Ini { file, section },
        addopts: Vec::new(),
        keys: BTreeMap::new(),
        filterwarnings: Vec::new(),
    };
    if head == usize::MAX {
        return Some(c);
    }
    for (k, first, last) in ini_keys(&lines, head, end) {
        let v = ini_value(&lines, first, last);
        match k.as_str() {
            "addopts" => c.addopts = v.split_whitespace().map(str::to_owned).collect(),
            "filterwarnings" => c.filterwarnings = v.lines().map(str::to_owned).collect(),
            _ => {
                c.keys.insert(k, v);
            }
        }
    }
    Some(c)
}

/// The text of `cfg`'s file with `want` added; `None` when it cannot be edited as such (a
/// pyproject.toml whose `tool` is not a plain table, an INI file without the section).
pub fn with_pytest_setup(text: &str, cfg: &PytestConfig, want: &PytestSetup) -> Option<String> {
    if want.is_empty() {
        return None;
    }
    match cfg.file {
        PytestFile::Ini { section, .. } => ini_with(text, section, want),
        PytestFile::Pyproject { native } => toml_with(text, native, want),
        PytestFile::NewPyproject => toml_with(text, false, want),
    }
}

fn ini_with(text: &str, section: &str, want: &PytestSetup) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (head, end) = match section_range(text, section) {
        Some(r) => r,
        // an empty pytest.ini: the section is added
        None if text.trim().is_empty() => {
            return ini_with(&format!("[{section}]\n"), section, want);
        }
        None => return None,
    };
    let keys = ini_keys(&lines, head, end);
    let find = |k: &str| {
        keys.iter()
            .find(|(n, _, _)| n == k)
            .map(|(_, f, l)| (*f, *l))
    };
    // inserts after a line index: (index, text)
    let mut after: Vec<(usize, String)> = Vec::new();
    let mut replace: BTreeMap<usize, String> = BTreeMap::new();
    let mut new_keys: Vec<String> = Vec::new();
    let cont_indent = |first: usize, last: usize| -> String {
        if last > first {
            let l = lines[last];
            l[..l.len() - l.trim_start().len()].to_string()
        } else {
            "    ".to_string()
        }
    };
    if !want.addopts.is_empty() {
        let opts = want.addopts.join(" ");
        match find("addopts") {
            Some((first, last)) if last == first => {
                let l = lines[first].trim_end();
                replace.insert(first, format!("{l} {opts}"));
            }
            Some((first, last)) => {
                after.push((last, format!("{}{opts}", cont_indent(first, last))));
            }
            None => new_keys.push(format!("addopts = {opts}")),
        }
    }
    for (k, v) in &want.keys {
        new_keys.push(format!("{k} = {}", v.text()));
    }
    if !want.filterwarnings.is_empty() {
        match find("filterwarnings") {
            Some((first, last)) => {
                let ind = cont_indent(first, last);
                let text: Vec<String> = want
                    .filterwarnings
                    .iter()
                    .map(|f| format!("{ind}{f}"))
                    .collect();
                after.push((last, text.join("\n")));
            }
            None => {
                let mut t = "filterwarnings =".to_string();
                for f in &want.filterwarnings {
                    t.push_str(&format!("\n    {f}"));
                }
                new_keys.push(t);
            }
        }
    }
    if !new_keys.is_empty() {
        after.push((head, new_keys.join("\n")));
    }
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        out.push_str(replace.get(&i).map_or(*l, String::as_str));
        out.push('\n');
        for (_, t) in after.iter().filter(|(a, _)| *a == i) {
            out.push_str(t);
            out.push('\n');
        }
    }
    Some(out)
}

fn toml_with(text: &str, native: bool, want: &PytestSetup) -> Option<String> {
    use toml_edit::{DocumentMut, Item, Table, Value, value};
    let mut doc: DocumentMut = text.parse().ok()?;
    let implicit = || {
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    };
    let tool = doc.entry("tool").or_insert_with(implicit).as_table_mut()?;
    let pytest = tool
        .entry("pytest")
        .or_insert_with(implicit)
        .as_table_mut()?;
    let t = if native {
        pytest
    } else {
        pytest
            .entry("ini_options")
            .or_insert_with(|| Item::Table(Table::new()))
            .as_table_mut()?
    };
    // a new item formatted as the array's last one
    let push = |a: &mut toml_edit::Array, s: &str| {
        // the first item's decor has no leading space; a later one's does
        let decor = a.iter().last().map(|v| v.decor().clone()).filter(|d| {
            d.prefix()
                .and_then(|p| p.as_str())
                .is_some_and(|p| !p.is_empty())
        });
        match decor {
            Some(d) => {
                let mut v = Value::from(s);
                *v.decor_mut() = d;
                a.push_formatted(v);
            }
            None => a.push(s),
        }
    };
    if !want.addopts.is_empty() {
        match t.get_mut("addopts") {
            Some(item) => {
                if let Some(s) = item.as_str() {
                    let joined = format!("{} {}", s.trim_end(), want.addopts.join(" "));
                    *item = value(joined.trim_start());
                } else if let Some(a) = item.as_array_mut() {
                    for o in &want.addopts {
                        push(a, o);
                    }
                } else {
                    return None;
                }
            }
            None if native => {
                let a: toml_edit::Array = want.addopts.iter().map(String::as_str).collect();
                t.insert("addopts", value(a));
            }
            None => {
                t.insert("addopts", value(want.addopts.join(" ")));
            }
        }
    }
    for (k, v) in &want.keys {
        let item = match (v, native) {
            (Setting::Bool(b), _) => value(*b),
            // pytest 9's native table wants a plugin's string setting as a string
            (Setting::Int(n), true) => value(n.to_string()),
            (Setting::Int(n), false) => value(*n),
        };
        t.insert(k, item);
    }
    if !want.filterwarnings.is_empty() {
        match t.get_mut("filterwarnings") {
            Some(item) => {
                if let Some(a) = item.as_array_mut() {
                    for f in &want.filterwarnings {
                        push(a, f);
                    }
                } else if let Some(s) = item.as_str() {
                    let joined = format!("{}\n{}", s.trim_end(), want.filterwarnings.join("\n"));
                    *item = value(joined);
                } else {
                    return None;
                }
            }
            None => {
                let mut a = toml_edit::Array::new();
                for f in &want.filterwarnings {
                    a.push_formatted(Value::from(f.as_str()).decorated("\n    ", ""));
                }
                a.set_trailing(",\n");
                a.set_trailing_comma(false);
                t.insert("filterwarnings", value(a));
            }
        }
    }
    let out = doc.to_string();
    out.parse::<toml::Table>().ok().map(|_| out)
}

/// The file the JavaScript offers create: loaded before each test file, it makes a request to
/// any host but this machine fail (nock 14 intercepts `http`, `https` and `fetch`).
pub const NO_NETWORK_FILE: &str = "test/no-network.ts";

/// [`NO_NETWORK_FILE`]'s text.
pub const NO_NETWORK: &str = r#"// Tests reach no host but this machine: a request to any other fails, so a test that
// depends on the network fails here instead of passing or flaking with it.
import nock from "nock";

nock.disableNetConnect();
nock.enableNetConnect((host) => /^(localhost|127\.0\.0\.1|\[?::1\]?)(:\d+)?$/.test(host));
"#;

/// A config edit for the JavaScript test runners: its new text, and what it added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerEdit {
    pub text: String,
    pub setup: bool,
    pub shuffle: bool,
}

/// A Vitest config (`None`: there is none) with `test.setupFiles` loading `setup` and
/// `test.sequence.shuffle` on, where it has neither: inserted at the start of its one
/// `test: {` object. A config with no such object, or several, is code Forge does not edit.
pub fn with_vitest_setup(
    text: Option<&str>,
    setup: Option<&str>,
    shuffle: bool,
) -> Option<RunnerEdit> {
    let Some(t) = text else {
        let mut items = Vec::new();
        if let Some(s) = setup {
            items.push(format!("    setupFiles: [\"{s}\"],\n"));
        }
        if shuffle {
            items.push("    sequence: { shuffle: true },\n".to_string());
        }
        if items.is_empty() {
            return None;
        }
        return Some(RunnerEdit {
            text: format!(
                "import {{ defineConfig }} from \"vitest/config\";\n\nexport default defineConfig({{\n  test: {{\n{}  }},\n}});\n",
                items.concat()
            ),
            setup: setup.is_some(),
            shuffle,
        });
    };
    let re = Regex::new(r"(?m)(?:^|[\s,{])test\s*:\s*\{").expect("regex");
    let found: Vec<_> = re.find_iter(t).collect();
    let [m] = found.as_slice() else {
        return None;
    };
    let word = |w: &str| Regex::new(&format!(r"\b{w}\b")).expect("regex").is_match(t);
    let setup = setup.filter(|_| !word("setupFiles"));
    let shuffle = shuffle && !word("shuffle") && !word("sequence");
    let mut items = Vec::new();
    if let Some(s) = setup {
        items.push(format!("setupFiles: [\"{s}\"]"));
    }
    if shuffle {
        items.push("sequence: { shuffle: true }".to_string());
    }
    if items.is_empty() {
        return None;
    }
    Some(RunnerEdit {
        text: insert_members(t, m.end(), &items, ","),
        setup: setup.is_some(),
        shuffle,
    })
}

/// `t` with `items` inserted as the first members of the object or array whose opening bracket
/// ends at `at`: on lines of their own, indented as the next line, when the members are on lines
/// of their own; inline otherwise. `sep` follows each one but the last of an empty object
/// (`","`; JSON's last member takes none).
fn insert_members(t: &str, at: usize, items: &[String], sep: &str) -> String {
    let rest = &t[at..];
    let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let next = &rest[spaces..];
    let empty = next.starts_with(['}', ']']);
    let (insert, skip) = if empty {
        (format!(" {} ", items.join(", ")), spaces)
    } else if next.starts_with('\n') || next.starts_with("\r\n") {
        let line = next.trim_start_matches(['\r', '\n']);
        let mut indent: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if line.trim_start().starts_with(['}', ']']) {
            indent.push_str("  ");
            let last = items.len() - 1;
            let text: String = items
                .iter()
                .enumerate()
                .map(|(i, x)| format!("\n{indent}{x}{}", if i < last { "," } else { sep }))
                .collect();
            (text, 0)
        } else {
            let text: String = items.iter().map(|i| format!("\n{indent}{i},")).collect();
            (text, 0)
        }
    } else {
        (format!(" {},", items.join(", ")), 0)
    };
    format!("{}{insert}{}", &t[..at], &t[at + skip..])
}

/// bunfig.toml (`""`: none yet) with `[test] preload` loading `setup` and `randomize = true`,
/// where it has neither; `None`: nothing to add, or a file it cannot read as TOML.
pub fn with_bun_test_setup(text: &str, setup: Option<&str>, randomize: bool) -> Option<RunnerEdit> {
    use toml_edit::{Array, DocumentMut, Item, Table, value};
    let mut doc: DocumentMut = text.parse().ok()?;
    let test = doc
        .entry("test")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()?;
    let setup = setup.filter(|_| !test.contains_key("preload"));
    let randomize = randomize && !test.contains_key("randomize");
    if setup.is_none() && !randomize {
        return None;
    }
    if let Some(s) = setup {
        let a: Array = [s].into_iter().collect();
        test.insert("preload", value(a));
    }
    if randomize {
        test.insert("randomize", value(true));
    }
    let mut out = doc.to_string();
    if text.is_empty() {
        out = out.trim_start().to_string();
    }
    Some(RunnerEdit {
        text: out,
        setup: setup.is_some(),
        shuffle: randomize,
    })
}

/// Jest's no-network file, loaded after the test framework of each test file
/// (`setupFilesAfterEnv`): CommonJS, which Jest loads untransformed in every project (TypeScript
/// or not, ESM or not). Jest runs several test files in one process, where each loads its own nock
/// onto the same `http` module: the file restores it after its tests, or `http.get` breaks in the
/// next file. jsdom has no fetch `Request`, which nock needs to load: there it does nothing.
pub const NO_NETWORK_JEST_FILE: &str = "test/no-network.cjs";

/// [`NO_NETWORK_JEST_FILE`]'s text.
pub const NO_NETWORK_JEST: &str = r#"// Tests reach no host but this machine: a request to any other fails, so a test that
// depends on the network fails here instead of passing or flaking with it.
// A browser-like environment without fetch's Request (jsdom) cannot load nock: nothing is done there.
if (typeof Request !== "undefined") {
  const nock = require("nock");

  nock.disableNetConnect();
  nock.enableNetConnect((host) => /^(localhost|127\.0\.0\.1|\[?::1\]?)(:\d+)?$/.test(host));
  // the next test file in this process loads its own nock onto the same http module
  afterAll(() => nock.restore());
}
"#;

/// Jest's config with `setupFilesAfterEnv` loading `setup` and `randomize: true`, where it has
/// neither. `file` says which config it is: package.json (its `jest` object, added when it has
/// none), `jest.config.json` (the whole file), else a `jest.config.*` module, which is code:
/// the keys go at the start of its one exported object (`module.exports = {`, `export default {`,
/// `defineConfig({`, or one `const config = {`), and an existing `setupFilesAfterEnv` is left
/// alone. In JSON the file is prepended to an existing `setupFilesAfterEnv` list. A config with
/// `projects` (each project has its own setup) gets nothing.
pub fn with_jest_setup(
    file: &str,
    text: &str,
    setup: Option<&str>,
    randomize: bool,
) -> Option<RunnerEdit> {
    if file.ends_with(".json") {
        return jest_json(text, file == "package.json", setup, randomize);
    }
    let word = |w: &str| {
        Regex::new(&format!(r"\b{w}\b"))
            .expect("regex")
            .is_match(text)
    };
    if word("projects") {
        return None;
    }
    let re = Regex::new(
        r"(?m)(?:module\.exports\s*=|export\s+default|defineConfig\(|^\s*(?:export\s+)?(?:const|let|var)\s+[A-Za-z_$][\w$]*(?:\s*:\s*[\w.<>]+)?\s*=)\s*\{",
    )
    .expect("regex");
    let found: Vec<_> = re.find_iter(text).collect();
    let [m] = found.as_slice() else {
        return None;
    };
    let setup = setup.filter(|_| !word("setupFilesAfterEnv"));
    let randomize = randomize && !word("randomize");
    let mut items = Vec::new();
    if let Some(s) = setup {
        items.push(format!("setupFilesAfterEnv: [\"{s}\"]"));
    }
    if randomize {
        items.push("randomize: true".to_string());
    }
    if items.is_empty() {
        return None;
    }
    Some(RunnerEdit {
        text: insert_members(text, m.end(), &items, ","),
        setup: setup.is_some(),
        shuffle: randomize,
    })
}

/// [`with_jest_setup`] on JSON: the edit is made in the text (its formatting kept) and then
/// checked: the file must parse to what it was, with only these keys added.
fn jest_json(
    text: &str,
    package: bool,
    setup: Option<&str>,
    randomize: bool,
) -> Option<RunnerEdit> {
    use serde_json::{Value, json};
    let before: Value = serde_json::from_str(text).ok()?;
    let root = before.as_object()?;
    let cfg = if package {
        match root.get("jest") {
            Some(Value::Object(o)) => Some(o),
            Some(_) => return None,
            None => None,
        }
    } else {
        Some(root)
    };
    if cfg.is_some_and(|c| c.contains_key("projects")) {
        return None;
    }
    let list = cfg.and_then(|c| c.get("setupFilesAfterEnv"));
    let setup = match list {
        None => setup,
        Some(Value::Array(a)) => setup.filter(|s| !a.iter().any(|x| x.as_str() == Some(*s))),
        Some(_) => None,
    };
    let randomize = randomize && !cfg.is_some_and(|c| c.contains_key("randomize"));
    if setup.is_none() && !randomize {
        return None;
    }
    let mut want = before.clone();
    let target = if package {
        want.as_object_mut()?
            .entry("jest")
            .or_insert_with(|| json!({}))
    } else {
        &mut want
    };
    let t = target.as_object_mut()?;
    if let Some(s) = setup {
        match t.get_mut("setupFilesAfterEnv") {
            Some(Value::Array(a)) => a.insert(0, json!(s)),
            _ => {
                t.insert("setupFilesAfterEnv".into(), json!([s]));
            }
        }
    }
    if randomize {
        t.insert("randomize".into(), json!(true));
    }
    let quoted = |s: &str| serde_json::to_string(s).unwrap_or_default();
    let mut new_keys = Vec::new();
    let mut out = text.to_string();
    // the file goes first in an existing list
    if let (Some(s), Some(Value::Array(_))) = (setup, list) {
        let re = Regex::new(r#""setupFilesAfterEnv"\s*:\s*\["#).expect("regex");
        let found: Vec<_> = re.find_iter(&out).collect();
        let [m] = found.as_slice() else {
            return None;
        };
        out = insert_members(&out, m.end(), &[quoted(s)], "");
    } else if let Some(s) = setup {
        new_keys.push(format!("\"setupFilesAfterEnv\": [{}]", quoted(s)));
    }
    if randomize {
        new_keys.push("\"randomize\": true".to_string());
    }
    if !new_keys.is_empty() {
        let at = match (package, cfg) {
            (true, None) => {
                // a new "jest" object, as the root's first member
                let open = out.find('{')? + 1;
                let inner = format!("\"jest\": {{{}}}", new_keys.join(", "));
                out = insert_members(&out, open, &[inner], "");
                None
            }
            (true, Some(_)) => {
                let re = Regex::new(r#""jest"\s*:\s*\{"#).expect("regex");
                let found: Vec<_> = re.find_iter(&out).collect();
                let [m] = found.as_slice() else {
                    return None;
                };
                Some(m.end())
            }
            (false, _) => Some(out.find('{')? + 1),
        };
        if let Some(at) = at {
            out = insert_members(&out, at, &new_keys, "");
        }
    }
    let after: Value = serde_json::from_str(&out).ok()?;
    (after == want).then_some(RunnerEdit {
        text: out,
        setup: setup.is_some(),
        shuffle: randomize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(files: &[(&str, &str)]) -> Option<PytestConfig> {
        let m: BTreeMap<String, String> = files
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        pytest_config(&|f| m.get(f).cloned())
    }

    fn want() -> PytestSetup {
        PytestSetup {
            addopts: vec!["--strict-markers".into(), "--disable-socket".into()],
            keys: vec![
                ("xfail_strict", Setting::Bool(true)),
                ("timeout", Setting::Int(120)),
            ],
            filterwarnings: vec!["error::pydantic.warnings.PydanticDeprecationWarning".into()],
        }
    }

    #[test]
    fn pytest_reads_the_file_pytest_reads() {
        let py = "[project]\nname = \"x\"\n\n[tool.pytest.ini_options]\naddopts = \"-ra -p no:randomly\"\nxfail_strict = true\n";
        let c = cfg(&[("pyproject.toml", py), ("setup.cfg", "[tool:pytest]\n")]).unwrap();
        assert_eq!(c.file, PytestFile::Pyproject { native: false });
        assert!(c.disables("randomly") && !c.disables("socket"));
        assert!(c.strict_xfail() && !c.strict_markers());
        let c = cfg(&[
            (
                "pytest.ini",
                "[pytest]\naddopts =\n    --strict-markers\n    -p no:socket\n",
            ),
            ("pyproject.toml", py),
        ])
        .unwrap();
        assert_eq!(c.file.path(), "pytest.ini");
        assert!(c.strict_markers() && c.disables("socket"));
        let c = cfg(&[("pyproject.toml", "[tool.pytest]\nstrict = true\n")]).unwrap();
        assert!(c.strict_markers() && c.strict_config() && c.strict_xfail());
        let c = cfg(&[
            ("pyproject.toml", "[project]\nname = \"x\"\n"),
            ("tox.ini", "[tox]\n"),
        ])
        .unwrap();
        assert_eq!(c.file, PytestFile::NewPyproject);
        assert!(cfg(&[("setup.py", "")]).is_none());
    }

    #[test]
    fn pytest_settings_are_added_to_a_new_table() {
        let py = "[project]\nname = \"x\"\n";
        let c = cfg(&[("pyproject.toml", py)]).unwrap();
        assert_eq!(
            with_pytest_setup(py, &c, &want()).unwrap(),
            "[project]\nname = \"x\"\n\n[tool.pytest.ini_options]\naddopts = \"--strict-markers --disable-socket\"\nxfail_strict = true\ntimeout = 120\nfilterwarnings = [\n    \"error::pydantic.warnings.PydanticDeprecationWarning\",\n]\n"
        );
    }

    #[test]
    fn pytest_settings_are_added_to_what_the_project_has() {
        let py = "[tool.pytest.ini_options]\naddopts = \"-ra\"\nfilterwarnings = [\n    \"ignore::DeprecationWarning:botocore.*\",\n]\n\n[tool.ruff]\nline-length = 100\n";
        let c = cfg(&[("pyproject.toml", py)]).unwrap();
        assert_eq!(
            with_pytest_setup(py, &c, &want()).unwrap(),
            "[tool.pytest.ini_options]\naddopts = \"-ra --strict-markers --disable-socket\"\nfilterwarnings = [\n    \"ignore::DeprecationWarning:botocore.*\",\n    \"error::pydantic.warnings.PydanticDeprecationWarning\",\n]\nxfail_strict = true\ntimeout = 120\n\n[tool.ruff]\nline-length = 100\n"
        );
        // pytest 9's table: lists, and a plugin's setting as a string
        let py = "[tool.pytest]\naddopts = [\"-ra\"]\n";
        let c = cfg(&[("pyproject.toml", py)]).unwrap();
        assert_eq!(
            with_pytest_setup(py, &c, &want()).unwrap(),
            "[tool.pytest]\naddopts = [\"-ra\", \"--strict-markers\", \"--disable-socket\"]\nxfail_strict = true\ntimeout = \"120\"\nfilterwarnings = [\n    \"error::pydantic.warnings.PydanticDeprecationWarning\",\n]\n"
        );
    }

    #[test]
    fn pytest_ini_settings_are_added_in_place() {
        let ini = "[pytest]\naddopts = -ra\nfilterwarnings =\n    ignore::UserWarning\ntestpaths = tests\n";
        let c = cfg(&[("pytest.ini", ini)]).unwrap();
        assert_eq!(
            with_pytest_setup(ini, &c, &want()).unwrap(),
            "[pytest]\nxfail_strict = true\ntimeout = 120\naddopts = -ra --strict-markers --disable-socket\nfilterwarnings =\n    ignore::UserWarning\n    error::pydantic.warnings.PydanticDeprecationWarning\ntestpaths = tests\n"
        );
        let cfgfile = "[metadata]\nname = x\n\n[tool:pytest]\naddopts =\n  -ra\n  -q\n";
        let c = cfg(&[("setup.cfg", cfgfile)]).unwrap();
        let w = PytestSetup {
            addopts: vec!["--strict-config".into()],
            ..PytestSetup::default()
        };
        assert_eq!(
            with_pytest_setup(cfgfile, &c, &w).unwrap(),
            "[metadata]\nname = x\n\n[tool:pytest]\naddopts =\n  -ra\n  -q\n  --strict-config\n"
        );
    }

    #[test]
    fn vitest_config_gets_setup_files_and_shuffle() {
        let cfg = "import { defineConfig } from \"vitest/config\";\n\nexport default defineConfig({\n  test: {\n    environment: \"node\",\n  },\n});\n";
        let e = with_vitest_setup(Some(cfg), Some("./test/no-network.ts"), true).unwrap();
        assert_eq!(
            e.text,
            "import { defineConfig } from \"vitest/config\";\n\nexport default defineConfig({\n  test: {\n    setupFiles: [\"./test/no-network.ts\"],\n    sequence: { shuffle: true },\n    environment: \"node\",\n  },\n});\n"
        );
        let inline = "export default { test: { globals: true } };\n";
        assert_eq!(
            with_vitest_setup(Some(inline), None, true).unwrap().text,
            "export default { test: { sequence: { shuffle: true }, globals: true } };\n"
        );
        let empty = "export default defineConfig({ test: {} });\n";
        assert_eq!(
            with_vitest_setup(Some(empty), Some("./s.ts"), false)
                .unwrap()
                .text,
            "export default defineConfig({ test: { setupFiles: [\"./s.ts\"] } });\n"
        );
        // a value the project set is kept; what is left is added
        let some = "export default defineConfig({\n  test: {\n    setupFiles: [\"./setup.ts\"],\n  },\n});\n";
        let e = with_vitest_setup(Some(some), Some("./n.ts"), true).unwrap();
        assert!(!e.setup && e.shuffle, "{e:?}");
        // no test object, or two: code Forge does not edit
        assert!(
            with_vitest_setup(
                Some("export default defineConfig({ plugins: [] });\n"),
                Some("./n.ts"),
                true
            )
            .is_none()
        );
        assert!(
            with_vitest_setup(
                Some("const a = { test: {} };\nconst b = { test: {} };\n"),
                None,
                true
            )
            .is_none()
        );
        // none yet: created
        assert!(
            with_vitest_setup(None, Some("./n.ts"), true)
                .unwrap()
                .text
                .contains("setupFiles: [\"./n.ts\"],\n    sequence: { shuffle: true },")
        );
    }

    #[test]
    fn bunfig_gets_preload_and_randomize() {
        let e = with_bun_test_setup("", Some("./test/no-network.ts"), true).unwrap();
        assert_eq!(
            e.text,
            "[test]\npreload = [\"./test/no-network.ts\"]\nrandomize = true\n"
        );
        let some = "[install]\nminimumReleaseAge = 604800\n\n[test]\npreload = [\"./setup.ts\"]\n";
        let e = with_bun_test_setup(some, Some("./n.ts"), true).unwrap();
        assert_eq!(
            e.text,
            "[install]\nminimumReleaseAge = 604800\n\n[test]\npreload = [\"./setup.ts\"]\nrandomize = true\n"
        );
        assert!(!e.setup);
        assert!(
            with_bun_test_setup(
                "[test]\npreload = \"./a.ts\"\nrandomize = false\n",
                Some("./n.ts"),
                true
            )
            .is_none()
        );
    }

    #[test]
    fn jest_config_gets_setup_and_randomize() {
        let f = "<rootDir>/test/no-network.cjs";
        // package.json without a jest key: a new one, first
        let pkg = "{\n  \"name\": \"shop\",\n  \"devDependencies\": {\n    \"jest\": \"^30.0.0\"\n  }\n}\n";
        let e = with_jest_setup("package.json", pkg, Some(f), true).unwrap();
        assert_eq!(
            e.text,
            "{\n  \"jest\": {\"setupFilesAfterEnv\": [\"<rootDir>/test/no-network.cjs\"], \"randomize\": true},\n  \"name\": \"shop\",\n  \"devDependencies\": {\n    \"jest\": \"^30.0.0\"\n  }\n}\n"
        );
        assert!(e.setup && e.shuffle);
        // its jest object: the file goes first in the list it has, randomize is added
        let pkg = "{\n  \"jest\": {\n    \"setupFilesAfterEnv\": [\n      \"@testing-library/jest-dom\"\n    ],\n    \"testEnvironment\": \"node\"\n  }\n}\n";
        assert_eq!(
            with_jest_setup("package.json", pkg, Some(f), true)
                .unwrap()
                .text,
            "{\n  \"jest\": {\n    \"randomize\": true,\n    \"setupFilesAfterEnv\": [\n      \"<rootDir>/test/no-network.cjs\",\n      \"@testing-library/jest-dom\"\n    ],\n    \"testEnvironment\": \"node\"\n  }\n}\n"
        );
        // a value the project set is kept; projects, a config path: nothing
        assert!(
            with_jest_setup(
                "package.json",
                "{\"jest\": {\"randomize\": false}}",
                None,
                true
            )
            .is_none()
        );
        assert!(
            with_jest_setup(
                "package.json",
                "{\"jest\": {\"projects\": [\"a\"]}}",
                Some(f),
                true
            )
            .is_none()
        );
        assert!(
            with_jest_setup("package.json", "{\"jest\": \"./jest.json\"}", Some(f), true).is_none()
        );
        assert_eq!(
            with_jest_setup("jest.config.json", "{}\n", None, true)
                .unwrap()
                .text,
            "{ \"randomize\": true }\n"
        );
        // a module: its one exported object
        let js = "/** @type {import('jest').Config} */\nmodule.exports = {\n  testEnvironment: \"node\",\n};\n";
        assert_eq!(
            with_jest_setup("jest.config.js", js, Some(f), true)
                .unwrap()
                .text,
            "/** @type {import('jest').Config} */\nmodule.exports = {\n  setupFilesAfterEnv: [\"<rootDir>/test/no-network.cjs\"],\n  randomize: true,\n  testEnvironment: \"node\",\n};\n"
        );
        let ts = "import type { Config } from \"jest\";\n\nconst config: Config = {\n  setupFilesAfterEnv: [\"./setup.ts\"],\n};\n\nexport default config;\n";
        let e = with_jest_setup("jest.config.ts", ts, Some(f), true).unwrap();
        assert!(!e.setup && e.shuffle);
        assert!(
            e.text
                .contains("const config: Config = {\n  randomize: true,\n  setupFilesAfterEnv")
        );
        // two objects, or projects: code Forge does not edit
        assert!(
            with_jest_setup(
                "jest.config.js",
                "const a = {};\nmodule.exports = { ...a };\n",
                Some(f),
                true
            )
            .is_none()
        );
        assert!(
            with_jest_setup(
                "jest.config.js",
                "module.exports = { projects: [\"a\"] };\n",
                Some(f),
                true
            )
            .is_none()
        );
    }
}
