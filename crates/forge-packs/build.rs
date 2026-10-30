//! FORGE_PACKS_BUILD_ID: a hash of everything that decides what the loader makes of the
//! built-in packs (this crate's sources, the shipped schemas, the packs themselves, the tool
//! adapters). It keys the parsed-packs cache, and its change makes Cargo re-embed `packs/`
//! (include_dir cannot tell Cargo which files it read).
//!
//! `$OUT_DIR/adapters.rs`: the adapted tools (§16) and their `direct` patterns, read from the
//! adapter files the checker embeds (`crates/forge-check/adapters/<tool>.yaml`), so the loader's
//! "run it through forge_do" check and the checker's adapters are one list.

use std::fs;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// A top-level `key: <scalar>` of an adapter file: a double-quoted YAML scalar (only `\\` and
/// `\"` escapes) or a plain one. The adapter files are parsed in full by the checker, whose
/// tests load every one, so a value this cannot read is a build error here.
fn top_level(text: &str, key: &str, file: &Path) -> Option<String> {
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?
        .trim();
    let Some(inner) = line.strip_prefix('"') else {
        assert!(
            !line.is_empty() && !line.contains(['\'', '#', '\\']),
            "{}: write {key} as a double-quoted string",
            file.display()
        );
        return Some(line.to_string());
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                assert!(
                    chars.as_str().trim().is_empty(),
                    "{}: text after {key}'s closing quote",
                    file.display()
                );
                return Some(out);
            }
            '\\' => match chars.next() {
                Some(e @ ('\\' | '"')) => out.push(e),
                e => panic!("{}: {key}: unsupported escape \\{e:?}", file.display()),
            },
            c => out.push(c),
        }
    }
    panic!("{}: {key} has no closing quote", file.display())
}

/// `$OUT_DIR/adapters.rs`: `ADAPTERS`, each adapter's tool and `direct` pattern, by file name.
fn adapters(dir: &Path) {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    files.sort();
    let mut src = String::from("const ADAPTERS: &[(&str, Option<&str>)] = &[\n");
    for f in &files {
        let text = fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let tool =
            top_level(&text, "tool", f).unwrap_or_else(|| panic!("{}: no tool", f.display()));
        assert_eq!(
            f.file_stem().and_then(|s| s.to_str()),
            Some(tool.as_str()),
            "{}: the file is named after its tool",
            f.display()
        );
        let direct = top_level(&text, "direct", f);
        src.push_str(&format!("    ({tool:?}, {direct:?}),\n"));
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("adapters.rs"), src).expect("adapters.rs");
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let adapter_dir = manifest.join("../forge-check/adapters");
    adapters(&adapter_dir);
    let inputs = [
        manifest.join("src"),
        manifest.join("Cargo.toml"),
        root.join("schemas"),
        root.join("packs"),
        adapter_dir,
    ];
    let mut hasher = blake3::Hasher::new();
    hasher.update(
        std::env::var("CARGO_PKG_VERSION")
            .unwrap_or_default()
            .as_bytes(),
    );
    for input in &inputs {
        println!("cargo::rerun-if-changed={}", input.display());
        let mut all = Vec::new();
        if input.is_dir() {
            files(input, &mut all);
        } else {
            all.push(input.clone());
        }
        all.sort();
        for f in all {
            let rel = f.strip_prefix(&root).unwrap_or(&f);
            hasher.update(rel.to_string_lossy().as_bytes());
            hasher.update(&[0]);
            hasher.update(&fs::read(&f).unwrap_or_default());
            hasher.update(&[0]);
        }
    }
    println!(
        "cargo::rustc-env=FORGE_PACKS_BUILD_ID={}",
        hasher.finalize().to_hex()
    );
}
