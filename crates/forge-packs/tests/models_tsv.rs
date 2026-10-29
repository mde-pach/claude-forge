//! The ai-models data file (packs/ai-models/models.tsv) rendered into the rules that need it.
//! A rule line that follows a `# models.tsv: <kind>` comment holds the rendered value; this test
//! fails while a rule differs from the data file, and rewrites the rules after an edit of the
//! data file with `FORGE_BLESS=1 cargo test -p forge-packs --test models_tsv`.
//!
//! Kinds:
//! - `retired`: grep ERE, a quoted retired id (pydantic-ai / LiteLLM prefixes allowed);
//! - `rejects <feature>`: ast-grep regex, a string literal naming a model that rejects <feature>;
//! - `advise`: the check command, `forge_advise forge_model_ids <deprecated> <known>`.

use std::path::{Path, PathBuf};

struct Row {
    id: String,
    provider: String,
    status: String,
    retires: String,
    replacement: String,
    rejects: Vec<String>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn load(text: &str) -> Vec<Row> {
    let mut out = vec![];
    for (i, l) in text.lines().enumerate() {
        if l.trim().is_empty() || l.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        assert!(
            f.len() == 6 && ["active", "deprecated", "retired"].contains(&f[2]),
            "models.tsv: line {}: id<TAB>provider<TAB>active|deprecated|retired<TAB>retires<TAB>replacement<TAB>rejects",
            i + 1
        );
        assert!(
            !(f[2] == "deprecated" && f[4] == "-"),
            "models.tsv: deprecated {} names no replacement",
            f[0]
        );
        out.push(Row {
            id: f[0].into(),
            provider: f[1].into(),
            status: f[2].into(),
            retires: f[3].into(),
            replacement: f[4].into(),
            rejects: if f[5] == "-" {
                vec![]
            } else {
                f[5].split(',').map(str::to_owned).collect()
            },
        });
    }
    out
}

/// An id as a regex alternative: a trailing `*` also covers the suffixed variants.
fn alternative(id: &str) -> String {
    match id.strip_suffix('*') {
        Some(stem) => format!("{}(-[a-z0-9-]+)?", stem.replace('.', "\\.")),
        None => id.replace('.', "\\."),
    }
}

fn render(kind: &str, rows: &[Row]) -> String {
    let mut parts = kind.split_whitespace();
    let name = parts.next().unwrap_or_default();
    let arg = parts.next().unwrap_or_default();
    let alts = |keep: &dyn Fn(&Row) -> bool| {
        rows.iter()
            .filter(|r| keep(r))
            .map(|r| alternative(&r.id))
            .collect::<Vec<_>>()
            .join("|")
    };
    match name {
        "retired" => format!(
            "[\"'`]((anthropic|openai)[:/])?({})[\"'`]",
            alts(&|r| r.status == "retired")
        ),
        "rejects" => {
            let ids = alts(&|r| r.rejects.iter().any(|x| x == arg));
            assert!(!ids.is_empty(), "models.tsv: no model rejects {arg:?}");
            format!("^[\"'](anthropic\\.)?({ids})[\"']$")
        }
        "advise" => {
            let dep: Vec<String> = rows
                .iter()
                .filter(|r| r.status == "deprecated")
                .map(|r| format!("{}|{}|{}", r.id, r.retires, r.replacement))
                .collect();
            let known: Vec<&str> = rows
                .iter()
                .filter(|r| r.provider == "anthropic")
                .map(|r| r.id.as_str())
                .collect();
            for s in dep.iter().map(String::as_str).chain(known.iter().copied()) {
                assert!(
                    !s.contains(['\'', '"', ';', '\\']),
                    "models.tsv: {s} holds a quote, \";\" or \"\\\""
                );
            }
            format!(
                "forge_advise forge_model_ids '{}' '{}'",
                dep.join(";"),
                known.join(" ")
            )
        }
        _ => panic!("unknown models.tsv kind {kind:?}"),
    }
}

/// The rule file with every marked line rendered from the data file.
fn render_file(text: &str, rows: &[Row]) -> String {
    let key = regex::Regex::new(r"^(\s*(?:- )?[A-Za-z_]+: )").unwrap();
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    for i in 0..lines.len() {
        let Some(kind) = lines[i].trim().strip_prefix("# models.tsv: ") else {
            continue;
        };
        let kind = kind.trim().to_owned();
        let next = lines.get(i + 1).cloned().unwrap_or_default();
        let lead = key
            .captures(&next)
            .unwrap_or_else(|| panic!("\"# models.tsv: {kind}\" must precede a key: value line"))
            [1]
        .to_owned();
        lines[i + 1] = format!("{lead}'{}'", render(&kind, rows).replace('\'', "''"));
    }
    lines.join("\n")
}

fn rule_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            rule_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "yaml")
            && std::fs::read_to_string(&p)
                .unwrap()
                .contains("# models.tsv: ")
        {
            out.push(p);
        }
    }
}

#[test]
fn rules_are_rendered_from_models_tsv() {
    let rows = load(&std::fs::read_to_string(root().join("ai-models/models.tsv")).unwrap());
    let mut files = vec![];
    rule_files(&root(), &mut files);
    assert!(files.len() >= 4, "marked rule files: {files:?}");
    let bless = std::env::var_os("FORGE_BLESS").is_some();
    let mut stale = vec![];
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let next = render_file(&text, &rows);
        if next != text {
            if bless {
                std::fs::write(&f, next).unwrap();
            } else {
                stale.push(f.display().to_string());
            }
        }
    }
    assert!(
        stale.is_empty(),
        "rules differ from models.tsv (FORGE_BLESS=1 rewrites them): {stale:?}"
    );
}
