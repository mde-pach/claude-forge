//! spec/17-tool-provisioning.md's table of pinned tools is rendered from Forge's tool table
//! (`catalog.rs`), so the two cannot drift. `FORGE_BLESS=1 cargo test -p forge-tools --test
//! spec_pins` rewrites the table.

use std::path::PathBuf;

use forge_tools::catalog::{Source, TOOLS, TOOLS_PYTHON};

const BEGIN: &str = "<!-- generated: tool pins (crates/forge-tools/tests/spec_pins.rs; FORGE_BLESS=1 rewrites it) -->";
const END: &str = "<!-- end generated -->";

fn spec_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/17-tool-provisioning.md")
}

fn platforms(keys: impl Iterator<Item = &'static str>) -> String {
    keys.map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
}

fn table() -> String {
    let mut out = String::from(
        "| Tool | Version | From | Checked against |\n|---|---|---|---|\n",
    );
    for t in TOOLS {
        let (from, checked) = match t.source {
            Source::Uv { base, assets }
            | Source::Bun { base, assets }
            | Source::Release { base, assets, .. } => (
                format!("release asset, `{base}`"),
                format!(
                    "pinned sha256 per platform: {}",
                    platforms(assets.iter().map(|a| a.platform))
                ),
            ),
            Source::UvTool {
                package,
                binary_sha256,
                ..
            } => (
                format!("PyPI `{package}`, installed by Forge's uv"),
                if binary_sha256.is_empty() {
                    "the version only".to_owned()
                } else {
                    format!(
                        "the upstream binary's sha256 per platform: {}",
                        platforms(binary_sha256.iter().map(|(p, _)| *p))
                    )
                },
            ),
            Source::Npm { package, .. } => (
                format!("npm `{package}`, installed by Forge's Bun"),
                "the version only".to_owned(),
            ),
        };
        out.push_str(&format!(
            "| {} | {} | {from} | {checked} |\n",
            t.name, t.version
        ));
    }
    out.push_str(&format!(
        "| python | {TOOLS_PYTHON} | a uv-managed Python, for the PyPI tools | uv's own checks |\n"
    ));
    out
}

#[test]
fn the_spec_table_of_pins_is_the_catalog() {
    let path = spec_file();
    let text = std::fs::read_to_string(&path).unwrap();
    let (head, rest) = text.split_once(BEGIN).expect("begin marker");
    let (_, tail) = rest.split_once(END).expect("end marker");
    let want = format!("{head}{BEGIN}\n{}{END}{tail}", table());
    if std::env::var_os("FORGE_BLESS").is_some_and(|v| v == "1") {
        std::fs::write(&path, &want).unwrap();
        return;
    }
    assert!(
        text == want,
        "spec/17's tool pins differ from catalog.rs: run FORGE_BLESS=1 cargo test -p forge-tools --test spec_pins"
    );
}
