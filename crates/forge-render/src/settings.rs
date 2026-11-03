//! Claude Code settings files (`.claude/settings.json`, `settings.local.json`,
//! `~/.claude/settings.json`): Forge appends its entries and removes exactly the ones it
//! recorded, editing the text in place so the rest of the file (comments, order, layout)
//! stays as the person wrote it (`src/settings.ts`).

use forge_detect::jsonc::{self, Member, Node};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An entry Forge wrote: an item of the array at `path`, or the scalar at `path`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsEntry {
    pub file: String,
    /// Dotted (`permissions.ask`, `hooks.PreToolUse`, `sandbox.enabled`).
    pub path: String,
    pub value: Value,
}

impl SettingsEntry {
    /// The identity of an entry (file, path, value).
    pub fn key(&self) -> String {
        format!("{}\0{}\0{}", self.file, self.path, self.value)
    }
}

/// An entry Forge wants: appended to an array, or set when absent.
#[derive(Debug, Clone, PartialEq)]
pub struct DesiredEntry {
    pub entry: SettingsEntry,
    pub append: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsError(pub String);

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SettingsError {}

fn tree(text: &str) -> Result<Node, SettingsError> {
    let t = if text.trim().is_empty() { "{}" } else { text };
    let node = jsonc::parse_tree(t)
        .map_err(|_| SettingsError("settings file is not valid JSON(C)".into()))?;
    if !matches!(node, Node::Object { .. }) {
        return Err(SettingsError("settings file is not a JSON object".into()));
    }
    Ok(node)
}

fn at<'a>(root: &'a Node, path: &[&str]) -> Option<&'a Node> {
    let mut cur = root;
    for k in path {
        cur = cur.get(k)?;
    }
    Some(cur)
}

/// The indentation unit a file uses (a tab, or the smallest run of spaces); two spaces by default.
pub fn indent_unit(text: &str) -> String {
    let indents: Vec<&str> = text
        .lines()
        .filter_map(|l| {
            let n = l.len() - l.trim_start_matches([' ', '\t']).len();
            (n > 0 && n < l.len()).then(|| &l[..n])
        })
        .collect();
    if indents.is_empty() {
        return "  ".into();
    }
    if indents.iter().filter(|i| i.starts_with('\t')).count() * 2 > indents.len() {
        return "\t".into();
    }
    let min = indents
        .iter()
        .filter(|i| !i.contains('\t'))
        .map(|i| i.len())
        .min()
        .unwrap_or(2);
    " ".repeat(min)
}

/// The indentation of the line holding byte `pos`.
fn line_indent(text: &str, pos: usize) -> &str {
    let start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[start..];
    let n = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..n]
}

/// Whether byte `pos` starts its line (only whitespace before it).
fn starts_line(text: &str, pos: usize) -> bool {
    let start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    text[start..pos].trim().is_empty()
}

/// `value` as pretty JSON, its continuation lines indented by `indent`.
fn pretty(value: &Value, unit: &str, indent: &str) -> String {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(unit.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    value.serialize(&mut ser).expect("JSON values serialize");
    let s = String::from_utf8(buf).expect("JSON is UTF-8");
    s.replace('\n', &format!("\n{indent}"))
}

/// After `pos`, skipping whitespace and comments: the position of a `,` if there is one.
fn comma_after(text: &str, pos: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = pos;
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if text[i..].starts_with("//") {
            i += text[i..].find('\n').unwrap_or(text.len() - i);
        } else if text[i..].starts_with("/*") {
            i += text[i..].find("*/").map_or(text.len() - i, |k| k + 2);
        } else {
            break;
        }
    }
    (b.get(i) == Some(&b',')).then_some(i)
}

/// Text with `insert` placed as a new last element of the container `node`
/// (`element` is the member or item text, already indented for a new line).
fn insert_element(text: &str, node: &Node, element: impl Fn(&str) -> String, unit: &str) -> String {
    let (start, end, last_end, first_start) = match node {
        Node::Object {
            start,
            end,
            members,
        } => (
            *start,
            *end,
            members.last().map(|m: &Member| m.value.end()),
            members.first().map(|m| m.key_start),
        ),
        Node::Array { start, end, items } => (
            *start,
            *end,
            items.last().map(Node::end),
            items.first().map(Node::start),
        ),
        Node::Scalar { .. } => unreachable!("containers only"),
    };
    let outer = line_indent(text, start).to_string();
    match (last_end, first_start) {
        (Some(last), Some(first)) => {
            let at = comma_after(text, last).map_or(last, |c| c + 1);
            let sep = if comma_after(text, last).is_some() {
                ""
            } else {
                ","
            };
            if starts_line(text, first) {
                let ind = line_indent(text, first).to_string();
                format!(
                    "{}{sep}\n{ind}{}{}",
                    &text[..at],
                    element(&ind),
                    &text[at..]
                )
            } else {
                format!("{}{sep} {}{}", &text[..at], element(""), &text[at..])
            }
        }
        _ => {
            let ind = format!("{outer}{unit}");
            format!(
                "{}\n{ind}{}\n{outer}{}",
                &text[..start + 1],
                element(&ind),
                &text[end - 1..]
            )
        }
    }
}

/// Appends or sets `entries` (the entries already there are left as they are). Arrays get
/// missing items appended; scalars are set only when absent. Returns the text and the
/// entries actually added.
pub fn add_entries(
    text: &str,
    entries: &[DesiredEntry],
) -> Result<(String, Vec<SettingsEntry>), SettingsError> {
    let mut out = if text.trim().is_empty() {
        "{}\n".to_string()
    } else {
        text.to_string()
    };
    let unit = indent_unit(text);
    let mut added = Vec::new();
    for d in entries {
        let e = &d.entry;
        let path: Vec<&str> = e.path.split('.').collect();
        let root = tree(&out)?;
        let cur = at(&root, &path);
        match (d.append, cur) {
            (true, Some(Node::Array { items, .. })) => {
                if items.iter().any(|i| i.to_value() == e.value) {
                    continue;
                }
                let node = cur.expect("array");
                out = insert_element(&out, node, |ind| pretty(&e.value, &unit, ind), &unit);
            }
            (true, Some(_)) => {
                return Err(SettingsError(format!(
                    "{}: {} is not an array; cannot append {}",
                    e.file, e.path, e.value
                )));
            }
            (false, Some(_)) => continue,
            (append, None) => {
                // the deepest existing object on the path, and what to create below it
                let mut depth = 0;
                let mut node = &root;
                while depth < path.len() {
                    match node.get(path[depth]) {
                        Some(n @ Node::Object { .. }) => {
                            node = n;
                            depth += 1;
                        }
                        Some(_) => {
                            return Err(SettingsError(format!(
                                "{}: {} is not an object",
                                e.file,
                                path[..=depth].join(".")
                            )));
                        }
                        None => break,
                    }
                }
                let mut value = if append {
                    Value::Array(vec![e.value.clone()])
                } else {
                    e.value.clone()
                };
                for k in path[depth + 1..].iter().rev() {
                    let mut m = serde_json::Map::new();
                    m.insert((*k).to_string(), value);
                    value = Value::Object(m);
                }
                let key = Value::String(path[depth].to_string());
                out = insert_element(
                    &out,
                    node,
                    |ind| format!("{key}: {}", pretty(&value, &unit, ind)),
                    &unit,
                );
            }
        }
        added.push(e.clone());
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok((out, added))
}

/// Removes the element at `idx` of container `node` from the text.
fn remove_element(text: &str, node: &Node, idx: usize) -> String {
    let spans: Vec<(usize, usize)> = match node {
        Node::Object { members, .. } => members
            .iter()
            .map(|m| (m.key_start, m.value.end()))
            .collect(),
        Node::Array { items, .. } => items.iter().map(|i| (i.start(), i.end())).collect(),
        Node::Scalar { .. } => unreachable!("containers only"),
    };
    let (s, e) = spans[idx];
    if spans.len() == 1 {
        let (start, end) = (node.start(), node.end());
        let empty = if matches!(node, Node::Object { .. }) {
            "{}"
        } else {
            "[]"
        };
        return format!("{}{empty}{}", &text[..start], &text[end..]);
    }
    if idx + 1 < spans.len() {
        format!("{}{}", &text[..s], &text[spans[idx + 1].0..])
    } else {
        format!("{}{}", &text[..spans[idx - 1].1], &text[e..])
    }
}

fn is_empty(n: &Node) -> bool {
    match n {
        Node::Object { members, .. } => members.is_empty(),
        Node::Array { items, .. } => items.is_empty(),
        Node::Scalar { .. } => false,
    }
}

/// Removes the member `key` of the object at `parent_path`.
fn remove_member(text: &str, parent_path: &[&str], key: &str) -> Result<String, SettingsError> {
    let root = tree(text)?;
    let Some(parent @ Node::Object { members, .. }) = at(&root, parent_path) else {
        return Ok(text.to_string());
    };
    match members.iter().rposition(|m| m.key == key) {
        Some(i) => Ok(remove_element(text, parent, i)),
        None => Ok(text.to_string()),
    }
}

/// Removes exactly the listed entries, then the containers the removal emptied. `None` when
/// the file ends up as `{}`.
pub fn remove_entries(
    text: &str,
    entries: &[SettingsEntry],
) -> Result<Option<String>, SettingsError> {
    let mut out = text.to_string();
    for e in entries {
        let path: Vec<&str> = e.path.split('.').collect();
        let root = tree(&out)?;
        let top;
        match at(&root, &path) {
            Some(node @ Node::Array { items, .. }) => {
                let Some(i) = items.iter().position(|v| v.to_value() == e.value) else {
                    continue;
                };
                out = remove_element(&out, node, i);
                top = path.len();
            }
            Some(n) if n.to_value() == e.value => {
                out = remove_member(&out, &path[..path.len() - 1], path[path.len() - 1])?;
                top = path.len() - 1;
            }
            _ => continue,
        }
        for n in (1..=top).rev() {
            let root = tree(&out)?;
            if !at(&root, &path[..n]).is_some_and(is_empty) {
                break;
            }
            out = remove_member(&out, &path[..n - 1], path[n - 1])?;
        }
    }
    let root = tree(&out)?;
    Ok((!is_empty(&root)).then_some(out))
}

/// Whether the entry is in the settings text.
pub fn has_entry(text: &str, e: &SettingsEntry) -> Result<bool, SettingsError> {
    let root = tree(text)?;
    let path: Vec<&str> = e.path.split('.').collect();
    Ok(match at(&root, &path) {
        Some(Node::Array { items, .. }) => items.iter().any(|i| i.to_value() == e.value),
        Some(n) => n.to_value() == e.value,
        None => false,
    })
}

/// Scalar entries the file already sets to another value (left unchanged), with that value.
pub fn conflicting_scalars(
    text: &str,
    entries: &[DesiredEntry],
) -> Result<Vec<(SettingsEntry, Value)>, SettingsError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let root = tree(text)?;
    Ok(entries
        .iter()
        .filter(|d| !d.append)
        .filter_map(|d| {
            let path: Vec<&str> = d.entry.path.split('.').collect();
            let cur = at(&root, &path)?.to_value();
            (cur != d.entry.value).then(|| (d.entry.clone(), cur))
        })
        .collect())
}

/// Sets the value at `path` (replacing what is there, creating the objects on the way),
/// the rest of the text unchanged.
pub fn set_value(text: &str, path: &[&str], value: &Value) -> Result<String, SettingsError> {
    let root = tree(text)?;
    if let Some(node) = at(&root, path) {
        let unit = indent_unit(text);
        let ind = line_indent(text, node.start()).to_string();
        let v = pretty(value, &unit, &ind);
        return Ok(format!(
            "{}{v}{}",
            &text[..node.start()],
            &text[node.end()..]
        ));
    }
    let d = DesiredEntry {
        entry: SettingsEntry {
            file: String::new(),
            path: path.join("."),
            value: value.clone(),
        },
        append: false,
    };
    Ok(add_entries(text, &[d])?.0)
}

/// The settings value at a dotted path.
pub fn get(text: &str, path: &str) -> Result<Option<Value>, SettingsError> {
    let root = tree(text)?;
    let path: Vec<&str> = path.split('.').collect();
    Ok(at(&root, &path).map(Node::to_value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn want(path: &str, value: Value, append: bool) -> DesiredEntry {
        DesiredEntry {
            entry: SettingsEntry {
                file: ".claude/settings.json".into(),
                path: path.into(),
                value,
            },
            append,
        }
    }

    #[test]
    fn creates_appends_and_removes() {
        let (t, added) = add_entries(
            "",
            &[
                want("permissions.ask", json!("Edit(a)"), true),
                want("permissions.ask", json!("Edit(b)"), true),
                want(
                    "hooks.Stop",
                    json!({"hooks": [{"type": "command", "command": "x"}]}),
                    true,
                ),
            ],
        )
        .unwrap();
        assert_eq!(added.len(), 3);
        assert_eq!(
            t,
            "{\n  \"permissions\": {\n    \"ask\": [\n      \"Edit(a)\",\n      \"Edit(b)\"\n    ]\n  },\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"x\"\n          }\n        ]\n      }\n    ]\n  }\n}\n"
        );
        assert_eq!(
            jsonc::parse(&t).unwrap()["permissions"]["ask"],
            json!(["Edit(a)", "Edit(b)"])
        );
        let entries: Vec<SettingsEntry> = added.clone();
        assert_eq!(remove_entries(&t, &entries).unwrap(), None);
    }

    #[test]
    fn keeps_the_person_s_text() {
        let before = "{\n\t// mine\n\t\"model\": \"opus\",\n\t\"permissions\": {\"allow\": [\"Bash(ls)\"], \"ask\": [\"Edit(x)\",]},\n}\n";
        let (t, _) =
            add_entries(before, &[want("permissions.ask", json!("Edit(y)"), true)]).unwrap();
        assert!(t.contains("// mine"));
        assert!(t.contains("[\"Edit(x)\", \"Edit(y)\"]"), "{t}");
        let back = remove_entries(&t, &[want("permissions.ask", json!("Edit(y)"), true).entry])
            .unwrap()
            .unwrap();
        assert_eq!(jsonc::parse(&back).unwrap(), jsonc::parse(before).unwrap());
        assert!(back.contains("// mine"));
        let (t2, _) = add_entries(before, &[want("sandbox.enabled", json!(true), false)]).unwrap();
        assert!(
            t2.contains("\t\"sandbox\": {\n\t\t\"enabled\": true\n\t}\n"),
            "{t2}"
        );
        assert!(
            add_entries(
                "{\"permissions\": {\"ask\": 1}}",
                &[want("permissions.ask", json!("x"), true)]
            )
            .is_err()
        );
    }

    #[test]
    fn prunes_emptied_containers() {
        let t = "{\n  \"model\": \"x\",\n  \"permissions\": {\n    \"ask\": [\n      \"Edit(a)\"\n    ]\n  }\n}\n";
        let out = remove_entries(t, &[want("permissions.ask", json!("Edit(a)"), true).entry])
            .unwrap()
            .unwrap();
        assert_eq!(out, "{\n  \"model\": \"x\"\n}\n");
    }
}
