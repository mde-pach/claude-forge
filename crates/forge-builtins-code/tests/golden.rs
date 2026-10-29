//! Golden comparison with the original shell functions: every pack example of a rule calling
//! one of this crate's builtins, plus the corner cases below, run through the TypeScript Forge's
//! generated engine and through the Rust port, in the same fixture with the same change set; the
//! lines that count must agree, after the deliberate wording changes ([`ORIGINAL_WORDING`]).
//!
//! The original comes from a generated `.forge/check.sh` named by `FORGE_GOLDEN_CHECK_SH`
//! (`bun run build`, then `forge apply --yes` in a copy of a tests/generator sample): its
//! functions, plus `templates/engine/javascript.sh` and `ai-models.sh` for the modules the sample
//! does not activate. Without it the test is skipped. The pinned tools are taken from Forge's
//! cache (the original's own downloads are replaced by it).

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{Case, Repo, attribute, not_run, options, pack_cases, run_port};
use forge_builtins_code::code_builtins;
use regex::Regex;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The original's text in the port's words: the wording fixed on purpose.
const ORIGINAL_WORDING: &[(&str, &str)] = &[
    // the agent is told what to do (audit 5.12)
    (
        r"\(its install scripts would run\); the user adds trusted packages, not Claude$",
        "(its install scripts would run); remove ${n}: only the user approves install scripts",
    ),
    // "fixed in no release yet" (audit 5.13)
    (
        r", (\S+) \(fixed in no release yet\); use a fixed version$",
        ", $1, and no release fixes it yet; use another package or version",
    ),
    // no rule ids in agent text (audit 3.12)
    (
        r" \(snapshot updates still go through no-snapshot-update\)\.$",
        ".",
    ),
    // editorconfig-checker's column is dropped
    (r"^(\S+?:[0-9]+):[0-9]+: ", "$1 "),
];

fn normalize(l: &str) -> String {
    let mut l = l.to_string();
    for (re, to) in ORIGINAL_WORDING {
        let re = Regex::new(re).unwrap();
        let n = if l.contains("trustedDependencies adds ") {
            let adds = l
                .split("trustedDependencies adds ")
                .nth(1)
                .and_then(|s| s.split(" (").next())
                .map_or(1, |s| s.split(' ').count());
            if adds == 1 { "it" } else { "them" }
        } else {
            ""
        };
        l = re.replace(&l, to.replace("${n}", n).as_str()).into_owned();
    }
    l
}

/// The shell library: the generated check.sh without its main part, the modules it lacks, and
/// the pinned tools from Forge's cache.
fn library(check_sh: &Path, dir: &Path) -> PathBuf {
    let text = std::fs::read_to_string(check_sh).expect("check.sh");
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.starts_with("trap "))
        .expect("trap line")
        + 1;
    let end = lines
        .iter()
        .position(|l| *l == "forge_step forge_integrity")
        .expect("main part");
    let engine = workspace().join("templates/engine");
    let js = std::fs::read_to_string(engine.join("javascript.sh"))
        .unwrap()
        .replace("@@LOCKFILE_LINT_VERSION@@", "5.1.0");
    let ai = std::fs::read_to_string(engine.join("ai-models.sh")).unwrap();
    let home = forge_home::Home::from_env().unwrap();
    let tools = home.tools_dir();
    // the snapshot check's ast-grep, from Forge's cache
    use forge_tools::ToolResolver;
    let p = forge_tools::Provisioner::new(home);
    let sg = p
        .resolve("ast-grep")
        .or_else(|| {
            (std::env::var("FORGE_TEST_INSTALL_TOOLS").as_deref() == Ok("1"))
                .then(|| p.ensure("ast-grep").ok())
                .flatten()
        })
        .map_or("ast-grep-is-not-installed".into(), |p| {
            p.display().to_string()
        });
    let overrides = format!(
        r#"
forge_release_bin() {{
  for _b in "{t}/$1"/*/"$1"; do [ -x "$_b" ] && {{ printf '%s' "$_b"; return 0; }}; done
  printf 'no %s in the cache' "$1"; return 1
}}
forge_pinned_lockfile_lint() {{ "{t}"/lockfile-lint/*/bin/lockfile-lint "$@"; }}
forge_pinned_ast_grep() {{ "{sg}" "$@"; }}
"#,
        t = tools.display()
    );
    // the sgconfig.yml the snapshot check names (FORGE_ROOT is only read for it)
    let sgdir = dir.join("root/.forge/ast-grep");
    std::fs::create_dir_all(sgdir.join("rules")).unwrap();
    std::fs::write(
        sgdir.join("sgconfig.yml"),
        "ruleDirs: [rules]\nlanguageGlobs:\n  tsx: [\"*.ts\", \"*.mts\", \"*.cts\"]\n",
    )
    .unwrap();
    // the change set is the fixture's, written before the library is read
    let body: Vec<&str> = lines[start..end]
        .iter()
        .map(|l| {
            if *l == "forge_change_set || exit 1" {
                ":"
            } else {
                l
            }
        })
        .collect();
    let lib = format!("{}\n{js}\n{ai}\n{overrides}\n", body.join("\n"));
    let p = dir.join("lib.sh");
    std::fs::write(&p, lib).unwrap();
    p
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The original function's lines that count; None when it skipped (a "forge:" line, a tool it
/// could not have).
fn original(lib: &Path, case: &Case, repo: &Repo) -> Option<Vec<String>> {
    let tmp = repo.dir.path().join("forge-tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let lines = |v: &[&String]| v.iter().map(|l| format!("{l}\n")).collect::<String>();
    let changed: Vec<&String> = repo.changed.iter().collect();
    let existing: Vec<&String> = changed
        .iter()
        .copied()
        .filter(|p| repo.root.join(p).is_file())
        .collect();
    let inbase: Vec<&String> = changed
        .iter()
        .copied()
        .filter(|p| repo.base.join(p).is_file())
        .collect();
    std::fs::write(tmp.join("changed"), lines(&changed)).unwrap();
    std::fs::write(tmp.join("existing"), lines(&existing)).unwrap();
    std::fs::write(tmp.join("inbase"), lines(&inbase)).unwrap();
    std::fs::write(tmp.join("scope"), lines(&existing)).unwrap();
    std::fs::write(tmp.join("deleted"), "").unwrap();
    let mut added = String::new();
    for (p, a) in &repo.added {
        match a {
            forge_builtins_code::ctx::Added::Whole => added.push_str(&format!("{p}\t*\n")),
            forge_builtins_code::ctx::Added::Lines(l) => {
                for n in l {
                    added.push_str(&format!("{p}\t{n}\n"));
                }
            }
        }
    }
    std::fs::write(tmp.join("added"), added).unwrap();
    let cp = Command::new("cp")
        .arg("-R")
        .arg(&repo.base)
        .arg(tmp.join("base"))
        .status()
        .unwrap();
    assert!(cp.success());
    let x = repo.extra(&case.rule);
    let script = format!(
        "IFS='\n'\nFORGE_TMP={tmp}\nFORGE_ROOT={sgroot}\nmode=--turn\nFORGE_ARG={tmp}\nc_id={id}\n. {lib}\ncd {root} || exit 9\nFORGE_FILES={files}\nforge_{name} {args}\n",
        tmp = quote(&tmp.to_string_lossy()),
        root = quote(&repo.root.to_string_lossy()),
        sgroot = quote(&lib.parent().unwrap().join("root").to_string_lossy()),
        id = quote(&case.rule.id),
        lib = quote(&lib.to_string_lossy()),
        files = quote(&x.files.join("\n")),
        name = case.name,
        args = case
            .args
            .iter()
            .map(|a| quote(a))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let out = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .env_remove("CLAUDECODE")
        .env_remove("FORGE_FOR_AGENT")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    if std::env::var_os("FORGE_GOLDEN_DEBUG").is_some() {
        eprintln!(
            "{}: exit {:?}\n{script}\nstderr: {}",
            case.label,
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // a "forge:" line is the person's; one other than a not-compared note means it skipped
    let skip = text
        .lines()
        .filter(|l| l.starts_with("forge:"))
        .find(|l| !l.ends_with("so their majors were not compared"));
    if let Some(l) = skip {
        eprintln!("{}: original skipped: {l}", case.label);
        return None;
    }
    let found: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("forge:"))
        .map(str::to_owned)
        .collect();
    let counted = if case.advised {
        found
    } else {
        attribute(&found, repo)
    };
    Some(counted.iter().map(|l| normalize(l)).collect())
}

fn files(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

/// Corner cases beyond the pack examples, on the rule of `like`.
fn own_cases(all: &[Case]) -> Vec<Case> {
    let like = |rule: &str| {
        all.iter()
            .find(|c| c.rule.id == rule)
            .cloned()
            .unwrap_or_else(|| panic!("{rule}"))
    };
    let case = |rule: &str, label: &str, head: &[(&str, &str)], now: &[(&str, &str)]| Case {
        label: format!("{rule} own: {label}"),
        want: None,
        head: files(head),
        files: files(now),
        ..like(rule)
    };
    vec![
        case(
            "typescript/bun/release-age",
            "string and large values",
            &[],
            &[("bunfig.toml", "[install]\nminimumReleaseAge = 604800\n")],
        ),
        case(
            "typescript/bun/release-age",
            "a comment after the value",
            &[],
            &[("bunfig.toml", "[install]\nminimumReleaseAge = 100 # soon\n")],
        ),
        case(
            "typescript/bun/trusted-deps",
            "several new entries, a nested package",
            &[("web/package.json", "{\"trustedDependencies\": []}\n")],
            &[(
                "web/package.json",
                "{\"trustedDependencies\": [\"esbuild\", \"sharp\"]}\n",
            )],
        ),
        case(
            "typescript/express/types-match",
            "alternatives and an unranged spec",
            &[],
            &[(
                "package.json",
                "{\"dependencies\": {\"express\": \"5 || 4\"}, \"devDependencies\": {\"@types/express\": \"^5.0.0\", \"@types/express-serve-static-core\": \"latest\"}}\n",
            )],
        ),
        case(
            "secrets/no-credential-files",
            "npmrc literal and reference, nested key",
            &[],
            &[
                (
                    ".npmrc",
                    "_auth = abc123\n//r/:_password=${PW}\n//r/:_authToken= tok\n",
                ),
                ("ops/keys/id_rsa", "x\n"),
                ("ops/keys/id_rsa.pub", "x\n"),
            ],
        ),
        case(
            "secrets/env-not-committed",
            "a template in a package, no .gitignore",
            &[],
            &[("api/.env.sample", "A=\n"), ("api/.env.test", "A=1\n")],
        ),
        case(
            "git/no-build-artifacts",
            "nested node_modules and a cache tag",
            &[],
            &[
                ("web/node_modules/x/index.js", "x\n"),
                (
                    "tools/cache/CACHEDIR.TAG",
                    "Signature: 8a477f597d28d172789f06886806bc55\n",
                ),
                ("tools/cache/a/b", "x\n"),
                ("src/x.pyc", "x\n"),
            ],
        ),
        case(
            "git/generated-files",
            "an @generated header past the first line",
            &[(
                "gen/api.ts",
                "/**\n * @generated\n */\nexport const a = 1;\n",
            )],
            &[(
                "gen/api.ts",
                "/**\n * @generated\n */\nexport const a = 2;\n",
            )],
        ),
        case(
            "ai-models/deprecated-model-ids",
            "prefixed ids, several on a line, a retired-preview id",
            &[],
            &[(
                "app/llm.py",
                "A = [\"anthropic/claude-sonnet-4-5\", 'openai:o3', \"claude-mythos-preview\"]\nB = \"claude-sonnet-4-5\"; C = \"claude-sonnet-4-5\"\n",
            )],
        ),
        case(
            "typescript/vitest/snapshot-stale",
            "an index import",
            &[
                ("src/fmt/index.ts", "export const f = () => ({ a: 1 });\n"),
                (
                    "src/fmt.test.ts",
                    "import { f } from \"./fmt\";\nit(\"x\", () => expect(f()).toMatchSnapshot());\n",
                ),
            ],
            &[(
                "src/fmt/index.ts",
                "export const f = () => ({ a: 1, b: \"x\" });\n",
            )],
        ),
        case(
            "ai-models/deprecated-model-ids",
            "a template literal",
            &[],
            &[("src/llm.ts", "const model = `claude-sonnet-4-5`;\n")],
        ),
        case(
            "ai-models/deprecated-model-ids",
            "documentation",
            &[],
            &[
                ("docs/models.md", "We moved off `claude-sonnet-4-5`.\n"),
                ("CHANGELOG.md", "- Replaced \"claude-sonnet-4-5\".\n"),
            ],
        ),
        case(
            "typescript/vitest/snapshot-stale",
            "the word golden in a test comment",
            &[
                ("src/fmt.ts", "export const f = () => ({ a: 1 });\n"),
                (
                    "src/fmt.test.ts",
                    "import { f } from \"./fmt\";\n// the golden path\nit(\"x\", () => expect(f().a).toBe(1));\n",
                ),
            ],
            &[(
                "src/fmt.ts",
                "export const f = () => ({ a: 1, b: \"x\" });\n",
            )],
        ),
        case(
            "git/editorconfig",
            "several files and rules",
            &[(
                ".editorconfig",
                "root = true\n\n[*]\nindent_style = space\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n",
            )],
            &[("a.yaml", "a:\n\tb: 1 \n"), ("b/c.txt", "no newline")],
        ),
    ]
}

/// The cases whose original and port differ on purpose: the port finds nothing.
const DIFFER: &[(&str, &str)] = &[
    (
        "ai-models/deprecated-model-ids own: documentation",
        "prose (docs, Markdown, changelogs) mentions model ids without using them (audit 4.5)",
    ),
    (
        "typescript/vitest/snapshot-stale own: the word golden in a test comment",
        "a test compares with golden files when it names a testdata/ or golden/ path, not when it says \"golden\" (audit 4.8)",
    ),
];

#[test]
fn port_matches_the_original() {
    let Some(check_sh) = std::env::var_os("FORGE_GOLDEN_CHECK_SH").map(PathBuf::from) else {
        eprintln!("FORGE_GOLDEN_CHECK_SH is not set: no golden comparison");
        return;
    };
    let opts = options();
    let names: Vec<&str> = code_builtins(&opts).iter().map(|b| b.name()).collect();
    let (_, mut cases) = pack_cases(&names);
    let own = own_cases(&cases);
    cases.extend(own);
    let dir = tempfile::tempdir().unwrap();
    let lib = library(&check_sh, dir.path());
    let (mut same, mut skipped, mut diffs) = (0, 0, vec![]);
    for case in &cases {
        let repo = case.repo();
        let (port, notes) = run_port(&opts, case, &repo);
        if not_run(&notes).is_some() {
            skipped += 1;
            continue;
        }
        let Some(mut orig) = original(&lib, case, &repo) else {
            skipped += 1;
            continue;
        };
        let mut port = port;
        port.sort();
        orig.sort();
        let differs = DIFFER.iter().any(|(l, _)| *l == case.label);
        if differs {
            // kept true: the original finds something there, the port nothing
            if orig.is_empty() || !port.is_empty() {
                diffs.push(format!(
                    "{}: listed as differing, but original {orig:?}, port {port:?}",
                    case.label
                ));
            } else {
                same += 1;
            }
        } else if port == orig {
            same += 1;
        } else {
            diffs.push(format!(
                "{}:\n  original: {orig:?}\n  port:     {port:?}",
                case.label
            ));
        }
    }
    eprintln!(
        "{} cases: {same} agree, {skipped} not compared, {} differ",
        cases.len(),
        diffs.len()
    );
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}
