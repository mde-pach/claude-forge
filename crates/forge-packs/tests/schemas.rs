//! The committed JSON Schemas (`schemas/*.schema.json`) are the ones the Rust model generates.
//! After a change to the model, rewrite them with
//! `UPDATE_SCHEMAS=1 cargo test -p forge-packs --test schemas`.

use std::path::Path;

use forge_packs::schema::{Kind, file_text, validate};
use serde_json::json;

#[test]
fn committed_schemas_are_generated_from_the_model() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let update = std::env::var_os("UPDATE_SCHEMAS").is_some();
    let mut stale = Vec::new();
    for kind in Kind::ALL {
        let path = root.join(kind.file());
        let want = file_text(kind);
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        if have != want {
            if update {
                std::fs::write(&path, &want).expect("write schema");
            } else {
                stale.push(kind.file());
            }
        }
    }
    assert!(
        stale.is_empty(),
        "{stale:?} differ from the model; run UPDATE_SCHEMAS=1 cargo test -p forge-packs --test schemas"
    );
}

#[test]
fn schemas_refuse_what_loading_adds_and_null() {
    let pack = json!({"id": "x", "description": "d", "level": "project"});
    assert!(validate(Kind::Pack, &pack).is_ok());
    let mut with_dir = pack.clone();
    with_dir["dir"] = json!("x");
    assert!(validate(Kind::Pack, &with_dir).is_err());
    let mut null_detect = pack.clone();
    null_detect["detect"] = json!(null);
    assert!(validate(Kind::Pack, &null_detect).is_err());
    let mut ctx = pack.clone();
    ctx["context"] = json!([{"text": "a norm\n", "when": {"facts": {"python.pm": "uv"}}}]);
    assert!(validate(Kind::Pack, &ctx).is_ok());
    ctx["context"] = json!([{"text": "a norm\n", "when": {}}]);
    assert!(validate(Kind::Pack, &ctx).is_err());
}

#[test]
fn guard_targets_are_structural() {
    let rule = |target: serde_json::Value| {
        json!({"id": "x/y", "description": "d", "scope": {"language": "any"}, "enforce": [target]})
    };
    let ok = rule(json!({"target": "guard", "decision": "deny",
        "command": {"name": "git", "sub": ["push"], "flags_any": ["--force"]}}));
    validate(Kind::Rule, &ok).unwrap();
    let check = rule(json!({"target": "guard", "decision": "deny", "check": "python-pm"}));
    validate(Kind::Rule, &check).unwrap();
    let raw = rule(json!({"target": "guard", "decision": "deny", "pattern": "git push"}));
    assert!(validate(Kind::Rule, &raw).is_err());
}
