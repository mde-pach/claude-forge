//! The JSON Schemas packs, rules and forge.yaml are validated against: the files Forge ships
//! (`schemas/*.schema.json`, written from `src/schemas.ts` by the TypeScript build).

use std::sync::LazyLock;

use jsonschema::Validator;
use serde_json::Value;

pub const PACK_SCHEMA: &str = include_str!("../../../schemas/pack.schema.json");
pub const RULE_SCHEMA: &str = include_str!("../../../schemas/rule.schema.json");
pub const FORGE_SCHEMA: &str = include_str!("../../../schemas/forge.schema.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pack,
    Rule,
    Project,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Pack => "pack",
            Kind::Rule => "rule",
            Kind::Project => "project",
        }
    }
}

fn compile(text: &str) -> Validator {
    let schema: Value = serde_json::from_str(text).expect("shipped schema is JSON");
    jsonschema::draft202012::options()
        .should_validate_formats(false)
        .build(&schema)
        .expect("shipped schema compiles")
}

static PACK: LazyLock<Validator> = LazyLock::new(|| compile(PACK_SCHEMA));
static RULE: LazyLock<Validator> = LazyLock::new(|| compile(RULE_SCHEMA));
static PROJECT: LazyLock<Validator> = LazyLock::new(|| compile(FORGE_SCHEMA));

/// Validates `data`; the error lists every problem ("/path message"), as `validate` in
/// `src/schemas.ts` does (its wording is ajv's, this is the jsonschema crate's).
pub fn validate(kind: Kind, data: &Value) -> Result<(), String> {
    let v = match kind {
        Kind::Pack => &*PACK,
        Kind::Rule => &*RULE,
        Kind::Project => &*PROJECT,
    };
    if v.is_valid(data) {
        return Ok(());
    }
    let mut msgs: Vec<String> = Vec::new();
    for e in v.iter_errors(data) {
        let path = e.instance_path().to_string();
        let m = format!("{} {}", if path.is_empty() { "/" } else { &path }, e);
        if !msgs.contains(&m) {
            msgs.push(m);
        }
    }
    Err(format!("invalid {}: {}", kind.as_str(), msgs.join("; ")))
}
