//! Golden tests: every pack example of a rule that runs a helper (`forge_helper`,
//! `forge_ratchet`) plus the fixtures below, run through the original helper
//! (templates/helpers/*.py with python3, *.mjs with bun) and through the Rust port; the
//! findings must agree, and must match what the example says (match: something found;
//! no_match: nothing).
//!
//! A runtime that is missing (python3 < 3.11, no bun) skips the original side only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use forge_core::builtin::{BuiltinCtx, Scope};
use forge_helpers::{Options, builtins, name_for_script};
use forge_packs::model::{Example, Target};

const COOLDOWN: &str = "259200";

fn templates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/helpers")
}

fn have(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git")
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn write(dir: &Path, files: &BTreeMap<String, String>) {
    for (p, text) in files {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// One helper call on one fixture.
#[derive(Debug, Clone)]
struct Case {
    name: String,
    rule: String,
    runtime: String,
    script: String,
    args: Vec<String>,
    head: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
    /// Some(true): must find something; Some(false): must find nothing.
    expect: Option<bool>,
}

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: PathBuf,
    tmp: PathBuf,
    changed: Vec<String>,
}

fn repo(case: &Case) -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    let mut head = case.head.clone();
    head.insert(".gitignore".into(), "node_modules/\n.venv/\n".into());
    write(&root, &head);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "head"]);
    write(&root, &case.files);
    let changed: Vec<String> = case
        .files
        .iter()
        .filter(|(p, t)| head.get(*p) != Some(*t))
        .map(|(p, _)| p.clone())
        .collect();
    // the change set as check.sh made it: changed, existing, inbase, base/
    let tmp = dir.path().join("forge-tmp");
    let base = tmp.join("base");
    std::fs::create_dir_all(&base).unwrap();
    let mut inbase = vec![];
    for p in &changed {
        if let Some(t) = head.get(p) {
            let b = base.join(p);
            std::fs::create_dir_all(b.parent().unwrap()).unwrap();
            std::fs::write(b, t).unwrap();
            inbase.push(p.clone());
        }
    }
    let lines = |v: &[String]| v.iter().map(|l| format!("{l}\n")).collect::<String>();
    std::fs::write(tmp.join("changed"), lines(&changed)).unwrap();
    std::fs::write(tmp.join("existing"), lines(&changed)).unwrap();
    std::fs::write(tmp.join("inbase"), lines(&inbase)).unwrap();
    Repo {
        _dir: dir,
        root,
        base,
        tmp,
        changed,
    }
}

/// The accepted changes of forge.yaml for the rule.
fn accepted(case: &Case) -> Vec<String> {
    let Some(text) = case.files.get("forge.yaml") else {
        return vec![];
    };
    let v = forge_packs::yaml::parse_strict(text).unwrap().value;
    v.get("accept")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter(|e| e.get("rule").and_then(|r| r.as_str()) == Some(case.rule.as_str()))
        .filter_map(|e| e.get("change").and_then(|c| c.as_str()).map(str::to_owned))
        .collect()
}

/// The original helper's findings, as check.sh shows them (accepted lines and skips dropped);
/// None when its runtime is missing or it skipped.
fn original(case: &Case, r: &Repo) -> Option<Vec<String>> {
    let (cmd, ok) = match case.runtime.as_str() {
        "python" => ("python3", have("python3", &["-c", "import tomllib"])),
        _ => ("bun", have("bun", &["--version"])),
    };
    if !ok {
        return None;
    }
    let script = templates().join(&case.script);
    let mut c = Command::new(cmd);
    c.arg(&script).current_dir(&r.root);
    if case.script.starts_with("ratchet") {
        c.arg(&r.base)
            .args(r.changed.iter().filter(|p| r.root.join(p).is_file()));
    } else {
        c.arg(&r.tmp).args(&case.args);
    }
    let out = c.output().expect("helper");
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    match out.status.code() {
        Some(0) => Some(vec![]),
        Some(1) => {
            let forge_yaml = case.files.get("forge.yaml").cloned().unwrap_or_default();
            Some(
                text.lines()
                    .filter(|l| !l.is_empty() && !forge_yaml.contains(*l))
                    .map(normalize)
                    .collect(),
            )
        }
        _ => {
            eprintln!("{}: original skipped: {}", case.name, text.trim());
            None
        }
    }
}

/// The original's text in the port's words: `path:0` is `path`, and the acceptance hint is the
/// agent's.
fn normalize(l: &str) -> String {
    let l = regex::Regex::new(r"^(\S+):0 ")
        .unwrap()
        .replace(l, "$1 ")
        .into_owned();
    // parsers word their errors their own way
    let l = regex::Regex::new(r"cannot be parsed \(.*\)$")
        .unwrap()
        .replace(&l, "cannot be parsed (…)")
        .into_owned();
    l.replace(
        "; a removal intended for a major version is accepted in forge.yaml (accept)",
        "; if the removal is intended for a major version, ask the user to accept this change",
    )
}

fn port(case: &Case, r: &Repo) -> (Vec<String>, Vec<String>) {
    let name = name_for_script(&case.script).expect("a helper builtin");
    let all = builtins(&Options::default());
    let b = all.iter().find(|b| b.name() == name).expect("builtin");
    let ctx = BuiltinCtx {
        root: r.root.clone(),
        base: Some(r.base.clone()),
        changed: r.changed.clone(),
        rule: case.rule.clone(),
        scope: Scope::Changed,
        accepted: accepted(case),
        notes_file: None,
    };
    let mut args = vec![case.script.clone()];
    args.extend(case.args.iter().cloned());
    let report = forge_helpers::run(b.as_ref(), &ctx, &args);
    let lines: Vec<String> = report
        .findings
        .iter()
        .map(|f| f.agent_text())
        .filter(|l| l != forge_helpers::ACCEPT_HINT)
        .map(|l| normalize(&l))
        .collect();
    (lines, report.notes.iter().map(|n| n.to_string()).collect())
}

fn helper_call(run: &str) -> Option<(String, String, Vec<String>)> {
    let re = regex::Regex::new(r"forge_(?:helper|ratchet) (\S+) (\S+)((?: [^|;&{}]+)?)").unwrap();
    let c = re.captures(run)?;
    let args = c[3]
        .split_whitespace()
        .map(|a| a.replace("$FORGE_COOLDOWN_SECONDS", COOLDOWN))
        .collect();
    Some((c[1].to_owned(), c[2].to_owned(), args))
}

fn pack_cases() -> Vec<Case> {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
    let mut out = vec![];
    for pack in set.packs.values() {
        for rule in &pack.rules {
            for t in &rule.enforce {
                let Target::Check(c) = t else { continue };
                let Some((runtime, script, args)) = helper_call(&c.run) else {
                    continue;
                };
                let Some(ex) = &rule.examples else { continue };
                let kinds = [(true, &ex.match_), (false, &ex.no_match)];
                for (want, list) in kinds {
                    for (i, e) in list.iter().flatten().enumerate() {
                        let Example::Fixture(f) = e else { continue };
                        let mut args = args.clone();
                        // changed_run passes the changed files ({files})
                        if c.changed_run
                            .as_deref()
                            .is_some_and(|r| r.contains("{files}"))
                        {
                            let head = f.head.clone().unwrap_or_default();
                            args.extend(
                                f.files
                                    .iter()
                                    .filter(|(p, t)| head.get(*p) != Some(*t))
                                    .map(|(p, _)| p.clone()),
                            );
                        }
                        out.push(Case {
                            name: format!(
                                "{} {}[{i}]",
                                rule.id,
                                if want { "match" } else { "no_match" }
                            ),
                            rule: rule.id.clone(),
                            runtime: runtime.clone(),
                            script: script.clone(),
                            args,
                            head: f.head.clone().unwrap_or_default().into_iter().collect(),
                            files: f.files.clone().into_iter().collect(),
                            // a run that decides more than the helper (a database dialect) is
                            // the checker's to test
                            expect: (!c.run.contains("forge_db_url")).then_some(want),
                        });
                    }
                }
            }
        }
    }
    out
}

fn files(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

fn case(
    name: &str,
    runtime: &str,
    script: &str,
    args: &[&str],
    head: &[(&str, &str)],
    now: &[(&str, &str)],
) -> Case {
    Case {
        name: name.into(),
        rule: "test/rule".into(),
        runtime: runtime.into(),
        script: script.into(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        head: files(head),
        files: files(now),
        expect: None,
    }
}

/// Fixtures beyond the pack examples: the corners of each helper.
fn own_cases() -> Vec<Case> {
    vec![
        case(
            "ratchet ruff many",
            "python",
            "ratchet.py",
            &[],
            &[(
                "pyproject.toml",
                "[tool.ruff]\nexclude = [\"a\"]\n[tool.ruff.lint]\nselect = [\"E\", \"F\", \"B\"]\nextend-select = [\"UP\"]\n[tool.ruff.lint.per-file-ignores]\n\"t/*\" = [\"S101\"]\n[tool.pytest.ini_options]\naddopts = \"-q\"\ntestpaths = [\"tests\", \"it\"]\nxfail_strict = true\n[tool.coverage.report]\nfail_under = 90.5\n[tool.coverage.run]\nomit = [\"a\"]\n",
            )],
            &[(
                "pyproject.toml",
                "[tool.ruff]\nexclude = [\"a\", \"b\"]\n[tool.ruff.lint]\nselect = [\"E\"]\nextend-ignore = [\"E501\"]\n[tool.ruff.lint.per-file-ignores]\n\"t/*\" = [\"S101\", \"D\"]\n\"src/*\" = \"E\"\n[tool.pytest.ini_options]\naddopts = \"-q --deselect=x -k foo --lf\"\ntestpaths = [\"tests\"]\nnorecursedirs = [\"legacy\"]\n[tool.coverage.report]\nfail_under = 80\nexclude_also = [\"pragma\"]\n[tool.coverage.run]\nomit = [\"a\", \"b/*\"]\n",
            )],
        ),
        case(
            "ratchet ty and pyright",
            "python",
            "ratchet.py",
            &[],
            &[
                (
                    "ty.toml",
                    "[rules]\na = \"error\"\nb = \"warn\"\nc = \"error\"\n[terminal]\nerror-on-warning = true\n",
                ),
                (
                    "pyrightconfig.json",
                    "{\n  // c\n  \"typeCheckingMode\": \"standard\",\n  \"reportAny\": true,\n  \"reportX\": \"warning\",\n  \"reportY\": \"error\",\n}\n",
                ),
            ],
            &[
                (
                    "ty.toml",
                    "[rules]\na = \"ignore\"\nc = \"error\"\n[src]\nexclude = [\"gen\"]\n[[overrides]]\ninclude = [\"x\"]\n",
                ),
                (
                    "pyrightconfig.json",
                    "{\"reportAny\": false, \"reportX\": \"none\", \"reportY\": \"warning\"}\n",
                ),
            ],
        ),
        case(
            "ratchet mypy toml and ini",
            "python",
            "ratchet.py",
            &[],
            &[
                (
                    "pyproject.toml",
                    "[tool.mypy]\nstrict = true\nwarn_unused_ignores = true\n",
                ),
                (
                    "mypy.ini",
                    "[mypy]\nstrict = True\n[mypy-a.*]\nignore_errors = false\n",
                ),
            ],
            &[
                (
                    "pyproject.toml",
                    "[tool.mypy]\nignore_missing_imports = true\n[[tool.mypy.overrides]]\nmodule = [\"a\", \"b\"]\nignore_missing_imports = true\n",
                ),
                (
                    "mypy.ini",
                    "[mypy]\nstrict = no\n[mypy-a.*, b.c]\nignore_errors = True\n",
                ),
            ],
        ),
        case(
            "ratchet broken after",
            "python",
            "ratchet.py",
            &[],
            &[("ruff.toml", "[lint]\nselect = [\"E\"]\n")],
            &[("ruff.toml", "[lint\nselect = [\"E\"]\n")],
        ),
        case(
            "ratchet js",
            "bun",
            "ratchet.mjs",
            &[],
            &[
                (
                    "biome.json",
                    "{\"linter\": {\"rules\": {\"recommended\": true, \"style\": \"error\", \"suspicious\": {\"noExplicitAny\": \"error\", \"noDebugger\": {\"level\": \"warn\"}}}}, \"files\": {\"includes\": [\"**\"]}, \"overrides\": []}\n",
                ),
                (
                    "tsconfig.json",
                    "{\"compilerOptions\": {\"strict\": true, \"noUnusedLocals\": true}, \"include\": [\"src\", \"test\"], \"extends\": \"./base.json\"}\n",
                ),
                (
                    "package.json",
                    "{\"scripts\": {\"test\": \"vitest run\", \"lint\": \"eslint .\", \"typecheck\": \"tsc\", \"build\": \"tsc -b\"}}\n",
                ),
            ],
            &[
                (
                    "biome.json",
                    "{\"linter\": {\"rules\": {\"recommended\": false, \"suspicious\": {\"noExplicitAny\": \"off\"}}}, \"files\": {\"includes\": [\"**\", \"!gen/**\"]}, \"overrides\": [{\"includes\": [\"a/**\", \"b/**\"], \"linter\": {\"enabled\": false}}]}\n",
                ),
                (
                    "tsconfig.json",
                    "{\n  // comment\n  \"compilerOptions\": {\"strict\": false, \"skipLibCheck\": true, \"ignoreDeprecations\": \"6.0\"},\n  \"include\": [\"src\"],\n  \"exclude\": [\"legacy\"],\n}\n",
                ),
                (
                    "package.json",
                    "{\"scripts\": {\"test\": \"vitest run --passWithNoTests\", \"lint\": \"eslint . || true\", \"build\": \"tsc -b --silent\"}}\n",
                ),
            ],
        ),
        case(
            "fk_index tables",
            "python",
            "fk_index.py",
            &[],
            &[(
                "app/models.py",
                "from sqlalchemy import Column, ForeignKey, Table\nclass A(Base):\n    __tablename__ = 'a'\n    b_id = Column(ForeignKey('b.id'))\n",
            )],
            &[(
                "app/models.py",
                "from sqlalchemy import Column, ForeignKey, Table, Index\nclass A(Base):\n    __tablename__ = 'a'\n    b_id = Column(ForeignKey('b.id'))\n    c_id: Mapped[int] = mapped_column(ForeignKey('c.id'))\n    d_id = mapped_column('dee', ForeignKey('d.id'), index=True)\n    e_id = Column(ForeignKey('e.id'), primary_key=True)\n    __table_args__ = (ForeignKeyConstraint(['f_id'], ['f.id']), Index('ix', 'g_id'), ForeignKeyConstraint(['g_id'], ['g.id']))\n\nassoc = Table('assoc', meta, Column('x_id', ForeignKey('x.id')), Column('y_id', ForeignKey('y.id')), UniqueConstraint('y_id', 'x_id'))\n",
            )],
        ),
        case(
            "public_api python",
            "python",
            "public_api.py",
            &[],
            &[
                ("lib/api.py", "__all__ = ['a', 'b', 'c', 'd', 'e', '_p']\n"),
                ("lib/gone.py", "__all__ = ['x', '_y']\n"),
            ],
            &[(
                "lib/api.py",
                "import warnings\nfrom typing_extensions import deprecated\n__all__ = ['a']\nclass MyDep(DeprecationWarning):\n    pass\ndef _warn(name, category=MyDep):\n    warnings.warn(name, category)\n@deprecated('use a')\ndef b(): ...\nc = deprecated('use a')(a)\ndef __getattr__(name):\n    if name in _OLD:\n        _warn(name)\n    raise AttributeError(name)\n_OLD = {'d': 1}\n",
            )],
        ),
        case(
            "public_api js",
            "bun",
            "public_api.mjs",
            &["removed"],
            &[
                (
                    "package.json",
                    "{\"name\": \"lib\", \"exports\": {\".\": \"./dist/index.js\"}}\n",
                ),
                (
                    "src/index.ts",
                    "export * from './util';\nexport function a() {}\nexport const b = 1;\nexport { c, d as e } from './more';\n",
                ),
                ("src/util.ts", "export const u = 1;\nexport class K {}\n"),
                ("src/more.ts", "export const c = 1;\nexport const d = 2;\n"),
            ],
            &[
                (
                    "src/index.ts",
                    "export * from './util';\nexport function a() {}\nexport const bee = 1;\n/** @deprecated Use bee */\nexport const b = bee;\nexport const c = 3;\nexport const e = c;\n",
                ),
                ("src/util.ts", "export const u = 1;\n"),
            ],
        ),
        case(
            "pytest_filterwarnings setup.cfg",
            "python",
            "pytest_filterwarnings.py",
            &["error::DeprecationWarning", "django"],
            &[],
            &[(
                "setup.cfg",
                "[tool:pytest]\nfilterwarnings =\n    ignore::UserWarning\n",
            )],
        ),
        case(
            "pytest_filterwarnings tox ok",
            "python",
            "pytest_filterwarnings.py",
            &["error::DeprecationWarning", "django"],
            &[],
            &[(
                "tox.ini",
                "[pytest]\nfilterwarnings =\n    error::DeprecationWarning\n",
            )],
        ),
        case(
            "node_version alias",
            "bun",
            "node_version.mjs",
            &[],
            &[],
            &[
                (".nvmrc", "lts/iron\n"),
                (
                    "package.json",
                    "{\"engines\": {\"node\": \">=20\"}, \"devEngines\": {\"runtime\": [{\"name\": \"node\", \"version\": \"^22\"}]}}\n",
                ),
            ],
        ),
        case(
            "node_version partial",
            "bun",
            "node_version.mjs",
            &[],
            &[("package.json", "{\"engines\": {\"node\": \">=20 <22\"}}\n")],
            &[(".node-version", "v22 # comment\n")],
        ),
        case(
            "pm_policy npm-scripts",
            "bun",
            "pm_policy.mjs",
            &["npm-scripts"],
            &[
                ("package.json", "{\"allowScripts\": {\"esbuild\": true}}\n"),
                (".npmrc", "allow-scripts=a\n"),
            ],
            &[
                (
                    "package.json",
                    "{\"allowScripts\": {\"esbuild\": true, \"sharp\": true, \"x\": false}}\n",
                ),
                (
                    ".npmrc",
                    "allow-scripts=a,b\nallow-scripts[]=c\nignore-scripts=false\ndangerously-allow-all-scripts=true\n",
                ),
            ],
        ),
        case(
            "pm_policy pnpm-builds",
            "bun",
            "pm_policy.mjs",
            &["pnpm-builds"],
            &[(
                "pnpm-workspace.yaml",
                "packages: [\"pkgs/*\"]\nonlyBuiltDependencies: [esbuild]\n",
            )],
            &[(
                "pnpm-workspace.yaml",
                "packages: [\"pkgs/*\"]\nonlyBuiltDependencies:\n  - esbuild\n  - \"sharp\"\nallowBuilds:\n  core-js: true\ndangerouslyAllowAllBuilds: true\nstrictDepBuilds: false\n",
            )],
        ),
        case(
            "pm_policy pnpm-catalog and overrides",
            "bun",
            "pm_policy.mjs",
            &["pnpm-catalog"],
            &[
                (
                    "pnpm-workspace.yaml",
                    "catalog:\n  react: ^19\ncatalogs:\n  legacy:\n    vue: ^2\n",
                ),
                ("package.json", "{\"dependencies\": {\"react\": \"^18\"}}\n"),
            ],
            &[(
                "package.json",
                "{\"dependencies\": {\"react\": \"^18\", \"vue\": \"^2.7\"}, \"devDependencies\": {\"react\": \"^19\"}}\n",
            )],
        ),
        case(
            "pm_policy pnpm-overrides",
            "bun",
            "pm_policy.mjs",
            &["pnpm-overrides"],
            &[("pnpm-workspace.yaml", "overrides:\n  a: 1.0.0\n")],
            &[(
                "pnpm-workspace.yaml",
                "overrides:\n  a: 1.0.0\n  \"b@<2\": \"2.1.0\"\n",
            )],
        ),
        case(
            "pm_policy pnpm-workspace",
            "bun",
            "pm_policy.mjs",
            &["pnpm-workspace"],
            &[
                (
                    "pnpm-workspace.yaml",
                    "packages:\n  - packages/*\n  - '!**/test/**'\n",
                ),
                ("packages/a/package.json", "{\"name\": \"@x/a\"}\n"),
                ("packages/test/package.json", "{\"name\": \"@x/t\"}\n"),
            ],
            &[(
                "packages/b/package.json",
                "{\"name\": \"@x/b\", \"dependencies\": {\"@x/a\": \"^1.0.0\", \"@x/t\": \"*\"}}\n",
            )],
        ),
        case(
            "pm_policy yarn-scripts",
            "bun",
            "pm_policy.mjs",
            &["yarn-scripts"],
            &[(".yarnrc.yml", "nodeLinker: node-modules\n")],
            &[
                (
                    ".yarnrc.yml",
                    "nodeLinker: node-modules\nenableScripts: true\n",
                ),
                (
                    "package.json",
                    "{\"dependenciesMeta\": {\"esbuild\": {\"built\": true}}}\n",
                ),
            ],
        ),
        case(
            "drf nested mixins",
            "python",
            "drf_permissions.py",
            &[],
            &[],
            &[
                (
                    "core/base.py",
                    "from rest_framework.generics import GenericAPIView\nclass Base(GenericAPIView):\n    def get_permissions(self):\n        return []\nclass Open(GenericAPIView):\n    pass\n",
                ),
                (
                    "app/views.py",
                    "from core.base import Base, Open\nclass A(Base):\n    pass\nclass B(Open):\n    permission_classes: list = []\nclass C(Open):\n    class Inner(Open):\n        pass\n@decorators.api_view(['GET'])\nasync def v(request):\n    pass\n",
                ),
            ],
        ),
    ]
}

#[test]
fn ports_agree_with_the_original_helpers() {
    let mut cases = pack_cases();
    assert!(cases.len() >= 40, "pack examples found: {}", cases.len());
    cases.extend(own_cases());
    let mut failures = vec![];
    let mut per_helper: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for case in &cases {
        let r = repo(case);
        let (port_lines, notes) = port(case, &r);
        let orig = original(case, &r);
        let e = per_helper.entry(case.script.clone()).or_default();
        e.0 += 1;
        if let Some(want) = case.expect {
            let networked = case.script.starts_with("new_deps") && !notes.is_empty();
            if !networked && want == port_lines.is_empty() {
                failures.push(format!(
                    "{}: expected {} findings, port gave {port_lines:?} (notes {notes:?})",
                    case.name,
                    if want { "some" } else { "no" }
                ));
            }
        }
        match orig {
            Some(o) => {
                e.1 += 1;
                if o == port_lines {
                    e.2 += 1;
                } else {
                    failures.push(format!(
                        "{}: original and port differ\n  original: {o:#?}\n  port:     {port_lines:#?}",
                        case.name
                    ));
                }
            }
            None => eprintln!("{}: original not run", case.name),
        }
    }
    for (h, (n, compared, same)) in &per_helper {
        eprintln!("{h}: {n} fixtures, {compared} compared with the original, {same} identical");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
