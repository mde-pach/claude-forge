//! Forge's own ruff config, for a project that does not configure ruff (§11.2): ruff's classic
//! rule set, plus what the active `tool: ruff` targets' snippets add (codes, banned APIs, …),
//! passed to ruff as `--config` overrides. The snippets' `extend-select` codes join
//! `lint.select`: ruff 0.17 drops an `extend-select` override given next to a `select` one.

use toml::{Table, Value};

/// The rules ruff selected by default before 0.17.0, Forge's baseline: pyflakes and the
/// pycodestyle errors no formatter fixes. Named, so a ruff release's own defaults do not change
/// what Forge checks.
const BASELINE: &[&str] = &["E4", "E7", "E9", "F"];

/// A snippet as TOML: a `[table] key = value` line is the table's header, then the entry.
fn parse(snippet: &str) -> Option<Table> {
    let mut text = String::new();
    for line in snippet.lines() {
        let l = line.trim();
        match l.strip_prefix('[').and_then(|r| r.split_once(']')) {
            Some((name, rest)) if !rest.trim().is_empty() && !name.contains(['"', '{']) => {
                text.push_str(&format!("[{name}]\n{}\n", rest.trim()));
            }
            _ => {
                text.push_str(l);
                text.push('\n');
            }
        }
    }
    text.parse::<Table>().ok()
}

/// `from` merged into `into`: tables deeply, arrays joined (each item once), else `from` wins.
fn merge(into: &mut Table, from: Table) {
    for (k, v) in from {
        match (into.get_mut(&k), v) {
            (Some(Value::Table(a)), Value::Table(b)) => merge(a, b),
            (Some(Value::Array(a)), Value::Array(b)) => {
                for item in b {
                    if !a.contains(&item) {
                        a.push(item);
                    }
                }
            }
            (_, v) => {
                into.insert(k, v);
            }
        }
    }
}

/// A key as TOML writes it: bare when it can be, else quoted.
fn key(k: &str) -> String {
    if !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        k.to_string()
    } else {
        Value::String(k.to_string()).to_string()
    }
}

/// Forge's ruff config from the active rules' ruff snippets: one `--config` value per setting
/// (`lint.extend-select = [...]`, `lint.flake8-tidy-imports = { banned-api = {...} }`). One
/// override per table, since ruff does not merge two overrides of the same table.
pub fn overrides<'a>(snippets: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut config = Table::new();
    let mut lint = Table::new();
    lint.insert(
        "select".into(),
        Value::Array(
            BASELINE
                .iter()
                .map(|c| Value::String((*c).into()))
                .collect(),
        ),
    );
    config.insert("lint".into(), Value::Table(lint));
    for sn in snippets {
        let Some(t) = parse(sn) else { continue };
        let Some(Value::Table(tool)) = t.get("tool") else {
            continue;
        };
        if let Some(Value::Table(ruff)) = tool.get("ruff") {
            merge(&mut config, ruff.clone());
        }
    }
    // one `lint.select`: ruff drops an `extend-select` override given next to a `select` one
    let mut extra: Vec<Value> = Vec::new();
    if let Some(Value::Array(a)) = config.remove("extend-select") {
        extra.extend(a);
    }
    if let Some(Value::Table(lint)) = config.get_mut("lint") {
        if let Some(Value::Array(a)) = lint.remove("extend-select") {
            extra.extend(a);
        }
        if let Some(Value::Array(select)) = lint.get_mut("select") {
            for v in extra {
                if !select.contains(&v) {
                    select.push(v);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (k, v) in &config {
        match (k.as_str(), v) {
            ("lint", Value::Table(lint)) => {
                for (k2, v2) in lint {
                    out.push(format!("lint.{} = {v2}", key(k2)));
                }
            }
            _ => out.push(format!("{} = {v}", key(k))),
        }
    }
    out
}

/// `--config <override>` for each.
pub fn config_args(overrides: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    overrides
        .into_iter()
        .flat_map(|o| ["--config".to_string(), o.into()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_merge_into_one_override_per_table() {
        let o = overrides([
            "[tool.ruff.lint] extend-select = [\"S307\"]",
            "[tool.ruff.lint]\nextend-select = [\"TID251\"]\n\n[tool.ruff.lint.flake8-tidy-imports.banned-api]\n\"pytz\".msg = \"use zoneinfo.ZoneInfo\"\n",
            "[tool.ruff.lint]\nextend-select = [\"TID251\"]\n\n[tool.ruff.lint.flake8-tidy-imports.banned-api]\n\"imp\".msg = \"removed\"\n",
            "not toml",
        ]);
        assert_eq!(
            o,
            vec![
                "lint.select = [\"E4\", \"E7\", \"E9\", \"F\", \"S307\", \"TID251\"]".to_string(),
                "lint.flake8-tidy-imports = { banned-api = { pytz = { msg = \"use zoneinfo.ZoneInfo\" }, imp = { msg = \"removed\" } } }".to_string(),
            ]
        );
    }

    #[test]
    fn without_snippets_the_baseline() {
        assert_eq!(
            overrides([]),
            vec!["lint.select = [\"E4\", \"E7\", \"E9\", \"F\"]".to_string()]
        );
    }
}
