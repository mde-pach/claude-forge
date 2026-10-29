//! Golden comparison with the shell original: every pack example run through the port and
//! through the TypeScript Forge's generated engine (the same `forge_*` function, in the same
//! repository, with the same check files); their findings must agree, after the deliberate
//! differences below are normalised.
//!
//! Opt-in: `FORGE_GOLDEN_ENGINE` names a generated `.forge/check.sh` (`bun run build`, then
//! `node dist/forge.js apply --yes` in a sample whose packs include docker, github-actions,
//! kubernetes, openapi, terraform and typescript/openapi). Tools: the real ones of this machine
//! (`FORGE_TEST_TOOLS=local` for the port, the engine's own for the original), fakes for
//! terraform and helm when they are not installed.

mod common;

use std::path::Path;
use std::process::Command;

use common::{Case, cases, ctx, gate, repo};

/// The engine's definitions: check.sh up to its first step.
fn engine(check_sh: &Path) -> String {
    let text = std::fs::read_to_string(check_sh).expect("check.sh");
    let end = text
        .find("\nforge_change_set || exit 1")
        .expect("check.sh's steps");
    text[..end].to_string()
}

fn check_number(engine: &str, rule: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(
        r"forge_check_meta_([0-9]+)\(\) \{{\n  c_id='{}'",
        regex::escape(rule)
    ))
    .unwrap();
    re.captures(engine).map(|c| c[1].to_string())
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The original's findings: its stdout without notes ("forge:" lines), with the deliberate
/// differences normalised.
fn original(engine: &str, case: &Case, root: &Path, files: &[String], fakes: &Path) -> Vec<String> {
    let n = check_number(engine, &case.rule.id).unwrap_or_default();
    let script = format!(
        "{engine}\nforge_change_set || exit 1\nc_id={}\nFORGE_FILES={}\nFORGE_CHECK_N={}\nforge_{} {}\n",
        sh_quote(&case.rule.id),
        sh_quote(&files.join("\n")),
        sh_quote(&n),
        case.builtin,
        case.args
            .iter()
            .map(|a| sh_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    );
    std::fs::create_dir_all(root.join(".forge")).unwrap();
    std::fs::write(root.join(".forge/check.sh"), script).unwrap();
    let path = format!(
        "{}:{}",
        std::env::var("PATH").unwrap_or_default(),
        fakes.display()
    );
    let out = Command::new("sh")
        .arg(".forge/check.sh")
        .arg("--changed")
        .current_dir(root)
        .env("PATH", path)
        .env_remove("CLAUDECODE")
        .output()
        .expect("sh");
    std::fs::remove_dir_all(root.join(".forge")).unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("forge:"))
        .map(|l| normalize(l, &case.builtin))
        .collect()
}

/// The original's text in the port's words (the differences kept on purpose).
fn normalize(l: &str, builtin: &str) -> String {
    let rules: &[(&str, &str)] = &[
        // the acceptance hints no longer teach forge.yaml's syntax
        (
            r"^Restore it, or have the user accept the change in forge\.yaml: .*$",
            "Restore it, or ask the user to accept this change.",
        ),
        (
            r"^Restore what clients rely on, or have the user accept the break in forge\.yaml: .*$",
            "Restore what clients rely on, or ask the user to accept the break.",
        ),
        // the layer-order text names requirements files where pip installs them
        (
            r"copy the manifest and lock file, install, then COPY \. \.$",
            "copy (the dependency files), install, then COPY . .",
        ),
        // tool findings: path:line message (columns and the redundant severity dropped)
        (
            r"^(\S+?):([0-9]+):[0-9]+: (Warning|Error|Notice|error) - ",
            "$1:$2 ",
        ),
        (r"^(\S+?):([0-9]+):[0-9]+: error ([^ ]+): ", "$1:$2 $3: "),
        (r"^(\S+?):([0-9]+):[0-9]+: ", "$1:$2 "),
        (r"^(\S+?):([0-9]+): Error: ", "$1:$2 Error: "),
        // Spectral: paths relative to the root (as the checker makes them), messages unquoted
        (r"^/\S*/repo/", ""),
        // (and the original glues bunx's last stderr line to an unterminated last line)
        (
            r#"^(\S+:[0-9]+ [a-z0-9-]+: )"(.*)"(Saved lockfile)?$"#,
            "$1$2",
        ),
        // OpenAPI sync and ratchet findings about a file: path:0
        (
            r"^(\S+): (the OpenAPI document changed|generated from the OpenAPI)",
            "$1:0 $2",
        ),
    ];
    let mut s = l.trim_end().to_string();
    for (re, to) in rules {
        s = regex::Regex::new(re).unwrap().replace(&s, *to).into_owned();
    }
    // a loosened linter config: the checker's ratchet hint
    if builtin == "openapi_ratchet" && s == "Restore it, or ask the user to accept this change." {
        s = "Restore the setting, or ask the user to accept this change.".into();
    }
    s
}

fn port_text(l: &str) -> String {
    regex::Regex::new(
        r"copy (the manifest and lock file|the requirements files), install, then COPY \. \.$",
    )
    .unwrap()
    .replace(l, "copy (the dependency files), install, then COPY . .")
    .into_owned()
}

#[test]
fn port_agrees_with_the_shell_engine() {
    let Some(check_sh) = std::env::var_os("FORGE_GOLDEN_ENGINE") else {
        eprintln!("FORGE_GOLDEN_ENGINE is not set: the golden comparison is skipped");
        return;
    };
    // SAFETY: set before any thread of this test binary reads it
    unsafe { std::env::set_var("FORGE_TEST_TOOLS", "local") };
    let (opts, dirs, _) = common::tools();
    let fakes = dirs[0].path().to_path_buf();
    let engine = engine(Path::new(&check_sh));
    let mut diffs = vec![];
    let mut compared = 0;
    for case in cases() {
        let r = repo(&case);
        let Some(files) = gate(&case, &r, &opts) else {
            continue;
        };
        let ctx = ctx(&case, &r);
        let extra = common::check(Some(case.rule.clone()), files.clone());
        let report =
            forge_builtins_infra::run(&opts, &case.builtin, &ctx, &extra, &case.args).unwrap();
        let mut port: Vec<String> = report
            .findings
            .iter()
            .map(|f| port_text(&f.agent_text()))
            .collect();
        let mut orig = original(&engine, &case, &r.root, &files, &fakes);
        // the shell drops the accepted lines through forge.yaml; so does the port
        orig.retain(|l| !ctx.accepted.iter().any(|a| a.contains(l.as_str())));
        port.sort();
        orig.sort();
        compared += 1;
        if port != orig {
            diffs.push(format!(
                "{}:\n  port:     {port:?}\n  original: {orig:?}\n  notes: {:?}",
                case.name, report.notes
            ));
        }
    }
    eprintln!("{compared} examples compared with the shell engine");
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}
