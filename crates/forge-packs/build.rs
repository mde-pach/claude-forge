//! FORGE_PACKS_BUILD_ID: a hash of everything that decides what the loader makes of the
//! built-in packs (this crate's sources, the shipped schemas, the packs themselves). It keys
//! the parsed-packs cache, and its change makes Cargo re-embed `packs/` (include_dir cannot
//! tell Cargo which files it read).

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

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let inputs = [
        manifest.join("src"),
        manifest.join("Cargo.toml"),
        root.join("schemas"),
        root.join("packs"),
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
