//! `forge.lock` (TOML, at the repository root; the user level's is `~/.claude/forge-user.lock`,
//! which the launcher does not read: a user-level setup pins no version for repositories).
//!
//! ```toml
//! forge = "0.6.0"                 # first key: the launcher reads it (forge_launcher::lock)
//! claude_code_min = "2.1.295"
//! rule_set = "<sha256>"           # the active rules, roles and packs (see `rule_set_hash`)
//!
//! [rules]                         # rule id → sha256 of its file
//! [files]                         # path → sha256 of what apply wrote (the rules files)
//! [roles]                         # file role → its effective patterns at the last apply
//! [tools]                         # tool pins (written by provisioning; kept as is)
//! [[settings]]                    # each settings entry apply added
//! [[blocks]]                      # Forge's block in a hook manager's file
//! ```
//!
//! Every command reading the lock validates it and refuses it as a whole when it names a path
//! Forge does not write (`remove` deletes what it lists).

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::precommit::BlockKind;
use crate::settings::SettingsEntry;

pub const LOCK_FILE: &str = "forge.lock";
pub const USER_LOCK_FILE: &str = ".claude/forge-user.lock";
/// The TypeScript Forge's locks (migrated by apply).
pub const OLD_PROJECT_LOCK: &str = ".forge/forge.lock.json";
pub const OLD_USER_LOCK: &str = ".claude/forge.lock.json";

/// The Claude Code version Forge needs: `onFailure` on hook entries (the guard fails closed).
pub const CLAUDE_CODE_MIN: &str = "2.1.295";

pub fn sha256(bytes: impl AsRef<[u8]>) -> String {
    let d = Sha256::digest(bytes.as_ref());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Forge's block in a hook manager's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockBlock {
    pub file: String,
    pub kind: BlockKind,
    /// Forge created the file.
    pub created: bool,
    /// sha256 of the block's lines, indentation removed.
    pub hash: String,
}

/// A settings entry as the lock stores it: a string value as `value`, anything else as
/// `json` (JSON text), so every value round-trips exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredEntry {
    file: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    json: Option<String>,
}

impl From<&SettingsEntry> for StoredEntry {
    fn from(e: &SettingsEntry) -> Self {
        match &e.value {
            Value::String(s) => StoredEntry {
                file: e.file.clone(),
                path: e.path.clone(),
                value: Some(s.clone()),
                json: None,
            },
            v => StoredEntry {
                file: e.file.clone(),
                path: e.path.clone(),
                value: None,
                json: Some(v.to_string()),
            },
        }
    }
}

impl StoredEntry {
    fn entry(self) -> Result<SettingsEntry, String> {
        let value = match (self.value, self.json) {
            (Some(s), None) => Value::String(s),
            (None, Some(j)) => serde_json::from_str(&j)
                .map_err(|e| format!("settings {}: json is not JSON ({e})", self.path))?,
            _ => return Err(format!("settings {}: needs value or json", self.path)),
        };
        Ok(SettingsEntry {
            file: self.file,
            path: self.path,
            value,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    forge: String,
    claude_code_min: String,
    #[serde(default)]
    rule_set: String,
    #[serde(default)]
    rules: BTreeMap<String, String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    roles: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    tools: toml::Table,
    #[serde(default)]
    settings: Vec<StoredEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    blocks: Vec<LockBlock>,
}

/// The lock.
#[derive(Debug, Clone, PartialEq)]
pub struct Lock {
    pub forge: String,
    pub claude_code_min: String,
    pub rule_set: String,
    pub rules: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
    pub roles: BTreeMap<String, Vec<String>>,
    /// Tool pins, as provisioning writes them; apply keeps them unchanged.
    pub tools: toml::Table,
    pub settings: Vec<SettingsEntry>,
    pub blocks: Vec<LockBlock>,
}

impl Lock {
    pub fn parse(text: &str) -> Result<Lock, String> {
        let s: Stored = toml::from_str(text).map_err(|e| e.message().to_string())?;
        Ok(Lock {
            forge: s.forge,
            claude_code_min: s.claude_code_min,
            rule_set: s.rule_set,
            rules: s.rules,
            files: s.files,
            roles: s.roles,
            tools: s.tools,
            settings: s
                .settings
                .into_iter()
                .map(StoredEntry::entry)
                .collect::<Result<_, _>>()?,
            blocks: s.blocks,
        })
    }

    pub fn render(&self) -> String {
        let s = Stored {
            forge: self.forge.clone(),
            claude_code_min: self.claude_code_min.clone(),
            rule_set: self.rule_set.clone(),
            rules: self.rules.clone(),
            files: self.files.clone(),
            roles: self.roles.clone(),
            tools: self.tools.clone(),
            settings: self.settings.iter().map(StoredEntry::from).collect(),
            blocks: self.blocks.clone(),
        };
        let body = toml::to_string(&s).expect("the lock serializes");
        format!("# Written by forge apply. Do not edit.\n{body}")
    }
}

/// The hash of the active rule set: the rules (id and file hash), the file roles they name
/// (effective patterns) and the active packs. Whatever caches results per rule set (the
/// checker) keys on it.
pub fn rule_set_hash(
    rules: &BTreeMap<String, String>,
    roles: &BTreeMap<String, Vec<String>>,
    packs: &[String],
) -> String {
    let mut text = String::new();
    for (id, h) in rules {
        text.push_str(&format!("rule {id} {h}\n"));
    }
    for (name, globs) in roles {
        text.push_str(&format!("role {name} {}\n", globs.join("\0")));
    }
    for p in packs {
        text.push_str(&format!("pack {p}\n"));
    }
    sha256(text)
}

/// A relative path without `.` or `..` segments, empty segments or backslashes.
pub fn plain_relative(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains(['\\', '\0'])
        && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Whether `base/p`, symbolic links resolved (its deepest existing ancestor when it does not
/// exist), is inside `base`.
pub fn resolves_inside(base: &Path, p: &str) -> bool {
    let Ok(root) = base.canonicalize() else {
        return false;
    };
    let mut at = base.join(p);
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    while !at.exists() {
        match (at.file_name(), at.parent()) {
            (Some(n), Some(parent)) => {
                rest.insert(0, n.to_os_string());
                at = parent.to_path_buf();
            }
            _ => return false,
        }
    }
    let Ok(mut real) = at.canonicalize() else {
        return false;
    };
    for r in rest {
        real.push(r);
    }
    match real.strip_prefix(&root) {
        Ok(rel) => {
            rel.components().next().is_some()
                && rel.components().all(|c| matches!(c, Component::Normal(_)))
        }
        Err(_) => false,
    }
}

/// The TypeScript Forge's lock (`.forge/forge.lock.json`), for migration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OldLock {
    pub forge_version: String,
    #[serde(default)]
    pub rules: BTreeMap<String, String>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub settings: Vec<SettingsEntry>,
    #[serde(default)]
    pub blocks: Vec<LockBlock>,
}

/// The hash the TypeScript Forge recorded for an owned file: JSON without whitespace; its
/// helper scripts also without quotes, commas, semicolons and parentheses.
pub fn old_owned_hash(path: &str, content: &str) -> String {
    if path.starts_with(".forge/helpers/") {
        return sha256(
            content
                .chars()
                .filter(|c| !" \t\r\n,;()'\"`".contains(*c))
                .collect::<String>(),
        );
    }
    if path.ends_with(".json") {
        return sha256(
            content
                .chars()
                .filter(|c| !" \t\r\n".contains(*c))
                .collect::<String>(),
        );
    }
    sha256(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_and_launcher_agrees() {
        let mut tools = toml::Table::new();
        tools.insert("uv".into(), toml::Value::String("0.9.0".into()));
        let lock = Lock {
            forge: "0.6.0".into(),
            claude_code_min: CLAUDE_CODE_MIN.into(),
            rule_set: "abc".into(),
            rules: BTreeMap::from([("python/no-eval".into(), "h".into())]),
            files: BTreeMap::from([(".claude/rules/forge-python.md".into(), "h2".into())]),
            roles: BTreeMap::from([("docs".into(), vec!["docs/**".into(), "!x".into()])]),
            tools,
            settings: vec![
                SettingsEntry {
                    file: ".claude/settings.json".into(),
                    path: "permissions.ask".into(),
                    value: json!("Edit(forge.yaml)"),
                },
                SettingsEntry {
                    file: ".claude/settings.json".into(),
                    path: "hooks.Stop".into(),
                    value: json!({"hooks": [{"type": "command", "command": "sh -c 'x \"y\"'", "timeout": 600}]}),
                },
            ],
            blocks: vec![LockBlock {
                file: ".husky/pre-commit".into(),
                kind: BlockKind::Husky,
                created: true,
                hash: "h3".into(),
            }],
        };
        let text = lock.render();
        assert_eq!(Lock::parse(&text).unwrap(), lock);
        assert_eq!(
            forge_launcher::parse_lock(&text).unwrap().forge.as_deref(),
            Some("0.6.0")
        );
    }

    #[test]
    fn paths() {
        assert!(plain_relative(".claude/rules/forge-x.md"));
        assert!(!plain_relative("../x"));
        assert!(!plain_relative("a//b"));
        let t = tempfile::tempdir().unwrap();
        assert!(resolves_inside(t.path(), "a/b"));
        assert!(!resolves_inside(t.path(), "../b"));
    }
}
