//! The Rust loader reads the built-in packs as the TypeScript one does: its dump equals
//! `dist/packs.json` (written by `bun install && bun run build` at the repository root),
//! compared as JSON values.

use std::path::Path;

use serde_json::Value;

/// The first difference between two JSON values, as "pointer: rust | ts".
fn first_difference(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                let p = format!("{at}/{k}");
                match (x.get(k), y.get(k)) {
                    (Some(u), Some(v)) => {
                        if let Some(d) = first_difference(u, v, &p) {
                            return Some(d);
                        }
                    }
                    (u, v) => return Some(format!("{p}: {u:?} | {v:?}")),
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{at}: {} items | {} items", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (u, v))| first_difference(u, v, &format!("{at}/{i}")))
        }
        _ if a == b => None,
        _ => Some(format!("{at}: {a} | {b}")),
    }
}

#[test]
fn differences_are_found() {
    use serde_json::json;
    let a = json!([{"id": "x", "n": 1, "l": [1, 2]}]);
    assert_eq!(first_difference(&a, &a.clone(), ""), None);
    assert!(first_difference(&a, &json!([{"id": "x", "n": 1, "l": [1, 3]}]), "").is_some());
    assert!(
        first_difference(
            &a,
            &json!([{"id": "x", "n": 1, "l": [1, 2], "extra": null}]),
            ""
        )
        .is_some()
    );
    assert!(first_difference(&a, &json!([{"id": "x", "n": 1.5, "l": [1, 2]}]), "").is_some());
}

#[test]
fn dump_equals_dist_packs_json() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/packs.json");
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; run `bun install && bun run build` at the repository root first",
            file.display()
        )
    });
    let ts: Value = serde_json::from_str(&text).expect("dist/packs.json is JSON");
    let set =
        forge_packs::load_builtin(forge_packs::Options::default()).expect("built-in packs load");
    let rust = set.bundle(Some(forge_packs::PackSource::Builtin));
    if let Some(d) = first_difference(&rust, &ts, "") {
        panic!(
            "the Rust dump differs from dist/packs.json (rebuild it if packs/ changed since): {d}"
        );
    }
}

#[test]
fn cached_load_equals_parsed_load() {
    let dir = tempfile::tempdir().unwrap();
    let opts = forge_packs::Options {
        cache_dir: Some(dir.path()),
    };
    let parsed = forge_packs::load_builtin(forge_packs::Options::default()).unwrap();
    let miss = forge_packs::load_builtin(opts).unwrap();
    let hit = forge_packs::load_builtin(opts).unwrap();
    assert_eq!(miss.cache.builtin, forge_packs::CacheOutcome::Miss);
    assert_eq!(hit.cache.builtin, forge_packs::CacheOutcome::Hit);
    assert_eq!(hit.packs, parsed.packs);
    assert_eq!(hit.findings, parsed.findings);
}
