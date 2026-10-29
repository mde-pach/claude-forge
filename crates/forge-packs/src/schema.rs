//! The JSON Schemas packs, rules and forge.yaml are validated against. They are generated from
//! the Rust model ([`crate::model`]); `schemas/*.schema.json` are those schemas as committed
//! files, for editors (a test keeps them in step: [`file_text`]).

use std::sync::LazyLock;

use jsonschema::Validator;
use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema};
use serde_json::Value;

use crate::model::{Pack, ProjectConfig, Rule};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pack,
    Rule,
    Project,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Pack, Kind::Rule, Kind::Project];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Pack => "pack",
            Kind::Rule => "rule",
            Kind::Project => "project",
        }
    }

    /// The committed file, below the repository root.
    pub fn file(self) -> &'static str {
        match self {
            Kind::Pack => "schemas/pack.schema.json",
            Kind::Rule => "schemas/rule.schema.json",
            Kind::Project => "schemas/forge.schema.json",
        }
    }
}

/// An optional field is left out, never written `null`: `null` goes from every schema.
fn no_null(schema: &mut Schema) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };
    if let Some(Value::Array(types)) = obj.get_mut("type") {
        types.retain(|t| t != "null");
        if types.len() == 1 {
            let t = types[0].clone();
            obj.insert("type".into(), t);
        }
    }
    for key in ["anyOf", "oneOf"] {
        let only = match obj.get_mut(key) {
            Some(Value::Array(alts)) => {
                alts.retain(|a| {
                    a.as_object()
                        .is_none_or(|o| o.len() != 1 || o.get("type").is_none_or(|t| t != "null"))
                });
                (alts.len() == 1).then(|| alts.remove(0))
            }
            _ => None,
        };
        if let Some(Value::Object(m)) = only {
            obj.remove(key);
            for (k, v) in m {
                obj.entry(k).or_insert(v);
            }
        }
    }
}

fn generate<T: JsonSchema>() -> Schema {
    let settings = SchemaSettings::draft2020_12()
        .with_transform(schemars::transform::RecursiveTransform(no_null));
    settings.into_generator().into_root_schema_for::<T>()
}

/// The schema of a kind, as JSON.
pub fn schema(kind: Kind) -> Value {
    let s = match kind {
        Kind::Pack => generate::<Pack>(),
        Kind::Rule => generate::<Rule>(),
        Kind::Project => generate::<ProjectConfig>(),
    };
    s.to_value()
}

/// The committed file's text for a kind.
pub fn file_text(kind: Kind) -> String {
    let mut out = serde_json::to_string_pretty(&schema(kind)).expect("schema serializes");
    out.push('\n');
    out
}

fn compile(kind: Kind) -> Validator {
    jsonschema::draft202012::options()
        .should_validate_formats(false)
        .build(&schema(kind))
        .expect("generated schema compiles")
}

static PACK: LazyLock<Validator> = LazyLock::new(|| compile(Kind::Pack));
static RULE: LazyLock<Validator> = LazyLock::new(|| compile(Kind::Rule));
static PROJECT: LazyLock<Validator> = LazyLock::new(|| compile(Kind::Project));

/// Validates `data`; the error lists every problem ("/path message").
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
