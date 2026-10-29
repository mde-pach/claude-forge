//! The loader's rules on small trees: strict YAML, layout, local packs and rules, the cache.

use std::path::Path;

use forge_packs::loader::{self, Place};
use forge_packs::tree::Disk;
use forge_packs::{CacheOutcome, ErrorKind, Level, LoadError, Options, Origin, PackSource};

/// The files of a tree: path, text.
type Files<'a> = &'a [(&'a str, &'a str)];

const PACK: &str =
    "id: acme\ndescription: Acme's conventions.\nlevel: project\ndetect: {always: true}\n";
const RULE: &str = "id: acme/no-todo\ndescription: No TODO comments.\nscope: {language: any, files: [\"src/**\"]}\nenforce:\n  - target: grep\n    mode: forbid\n    pattern: TODO\n";

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

/// Loads `files` as a pack tree.
fn tree(files: &[(&str, &str)]) -> Result<forge_packs::Loaded, LoadError> {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), files);
    let disk = Disk(dir.path().to_path_buf());
    loader::load_packs(&Place {
        tree: &disk,
        prefix: "",
        shown: "",
        source: PackSource::Local,
    })
}

fn error(files: &[(&str, &str)]) -> LoadError {
    tree(files).expect_err("the tree must not load")
}

#[test]
fn loads_a_pack_tree() {
    let child = "id: acme/web\ndescription: Acme's web apps.\nlevel: project\n";
    let loaded = tree(&[
        ("acme/pack.yaml", PACK),
        ("acme/rules/no-todo.yaml", RULE),
        ("acme/rules/notes.txt", "not a rule"),
        ("acme/init/ruff.toml", "line-length = 100\n"),
        ("acme/web/pack.yaml", child),
        ("acme/empty/.keep", ""),
    ]);
    // a directory holding only a dotfile is not empty: as in TypeScript, it is a stray directory
    let err = loaded.expect_err("acme/empty is not a pack");
    assert_eq!(err.kind, ErrorKind::Layout);
    assert!(
        err.to_string()
            .contains("acme/empty: not a pack (no pack.yaml)"),
        "{err}"
    );

    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            (
                "acme/pack.yaml",
                &format!("{PACK}context:\n  - id: tests-first\n    text: |\n      ctx\n    when: {{facts: {{python.pm: uv}}, packs: [acme/web]}}\n"),
            ),
            ("acme/rules/no-todo.yaml", RULE),
            ("acme/init/x", ""),
            ("acme/web/pack.yaml", child),
        ],
    );
    std::fs::create_dir_all(dir.path().join("acme/empty")).unwrap();
    let disk = Disk(dir.path().to_path_buf());
    let loaded = loader::load_packs(&Place {
        tree: &disk,
        prefix: "p/",
        shown: "",
        source: PackSource::Local,
    })
    .unwrap();
    let ids: Vec<&String> = loaded.packs.keys().collect();
    assert_eq!(ids, ["acme", "acme/web"]);
    let acme = &loaded.packs["acme"];
    assert_eq!(acme.children, ["acme/web"]);
    assert_eq!(acme.context.len(), 1);
    assert_eq!(acme.context[0].id.as_deref(), Some("tests-first"));
    assert_eq!(acme.context[0].text, "ctx\n");
    assert_eq!(acme.context[0].when.as_ref().unwrap().packs, ["acme/web"]);
    assert_eq!(acme.dir, "p/acme");
    assert_eq!(acme.init_dir.as_deref(), Some("p/acme/init"));
    assert_eq!(acme.rules.len(), 1);
    assert_eq!(acme.rules[0].file, "p/acme/rules/no-todo.yaml");
    assert_eq!(acme.rules[0].pack, "acme");
    assert_eq!(acme.rules[0].hash.len(), 64);
    assert_eq!(loaded.packs["acme/web"].parent.as_deref(), Some("acme"));
}

#[test]
fn context_is_pack_yaml_only() {
    let err = error(&[("acme/pack.yaml", PACK), ("acme/context.md", "ctx\n")]);
    assert_eq!(err.kind, ErrorKind::Layout);
    assert!(err.to_string().contains("context:"), "{err}");
    for (ctx, want) in [
        ("[{text: x, when: {facts: {python.pm: conda}}}]", "has no value \"conda\""),
        ("[{text: x, when: {facts: {os: linux}}}]", "unknown fact \"os\""),
        ("[{id: a, text: x}, {id: a, text: y}]", "used twice"),
    ] {
        let err = error(&[("acme/pack.yaml", &format!("{PACK}context: {ctx}\n"))]);
        assert!(err.to_string().contains(want), "{ctx}: {err}");
    }
}

#[test]
fn strict_yaml() {
    for (text, want) in [
        (
            "id: acme\nid: acme\ndescription: x\nlevel: project\n",
            "Map keys must be unique",
        ),
        (
            "id: acme\ndescription: &d x\nlevel: project\n",
            "anchors are not allowed",
        ),
        (
            "id: acme\ndescription: !!str x\nlevel: project\n",
            "tag !!str is not allowed",
        ),
        (
            "id: acme\ndescription: x\nlevel: project\n---\nid: other\n",
            "multiple documents",
        ),
        ("id: acme\ndescription: [x\nlevel: project\n", "line"),
    ] {
        let err = error(&[("acme/pack.yaml", text)]);
        assert_eq!(err.kind, ErrorKind::Yaml, "{text:?}: {err}");
        assert!(err.message.contains(want), "{text:?}: {err}");
        assert_eq!(err.file, "acme/pack.yaml");
    }
    let alias = "id: acme/no-todo\ndescription: &d No TODO.\nreview: *d\nscope: {language: any}\nenforce: [{target: grep, mode: forbid, pattern: TODO}]\n";
    let err = error(&[("acme/pack.yaml", PACK), ("acme/rules/no-todo.yaml", alias)]);
    assert!(err.message.contains("aliases are not allowed"), "{err}");
}

#[test]
fn schema_and_layout() {
    let cases: &[(Files, ErrorKind, &str)] = &[
        (
            &[("acme/pack.yaml", "id: acme\ndescription: x\n")],
            ErrorKind::Schema,
            "invalid pack",
        ),
        (
            &[(
                "acme/pack.yaml",
                "id: acme\ndescription: x\nlevel: project\nextra: 1\n",
            )],
            ErrorKind::Schema,
            "invalid pack",
        ),
        (
            &[(
                "acme/pack.yaml",
                "id: acme\ndescription: x\nlevel: user\ndetect: {always: true}\n",
            )],
            ErrorKind::Schema,
            "invalid pack",
        ),
        (
            &[(
                "acme/pack.yaml",
                "id: other\ndescription: x\nlevel: project\n",
            )],
            ErrorKind::Layout,
            "must equal its path in the pack tree, \"acme\"",
        ),
        (
            &[("acme/pack.yaml", PACK), ("acme/rules/other.yaml", RULE)],
            ErrorKind::Layout,
            "id must be \"acme/other\"",
        ),
        (
            &[
                ("acme/pack.yaml", PACK),
                (
                    "acme/web/pack.yaml",
                    "id: acme/web\ndescription: x\nlevel: user\n",
                ),
            ],
            ErrorKind::Layout,
            "level user differs from parent acme (project)",
        ),
        (
            &[("acme/pack.yaml", PACK), ("acme/stray/notes.md", "x")],
            ErrorKind::Layout,
            "not a pack (no pack.yaml)",
        ),
        (
            &[("acme/notes.md", "x")],
            ErrorKind::Layout,
            "not a pack (no pack.yaml)",
        ),
        (
            &[(
                "acme/pack.yaml",
                "id: acme\ndescription: x\nlevel: project\ndetect: {content_any: [{files: [a], pattern: \"(\"}]}\n",
            )],
            ErrorKind::Schema,
            "is not a regular expression",
        ),
        (
            &[
                ("acme/pack.yaml", PACK),
                (
                    "acme/rules/no-todo.yaml",
                    &RULE.replace("pattern: TODO", "pattern: 12"),
                ),
            ],
            ErrorKind::Schema,
            "invalid rule",
        ),
    ];
    for (files, kind, want) in cases {
        let err = error(files);
        assert_eq!(err.kind, *kind, "{err}");
        assert!(err.message.contains(want), "want {want:?}: {err}");
    }
}

#[test]
fn reserved_directories_are_not_packs() {
    // rules/ and init/ hold no pack.yaml and are never child packs
    let loaded = tree(&[
        ("acme/pack.yaml", PACK),
        ("acme/rules/no-todo.yaml", RULE),
        ("acme/init/rules/x.txt", ""),
    ])
    .unwrap();
    assert_eq!(loaded.packs.len(), 1);
    // a pack.yaml inside them is not read as a child either
    let loaded = tree(&[
        ("acme/pack.yaml", PACK),
        ("acme/init/pack.yaml", "not: a pack"),
    ])
    .unwrap();
    assert_eq!(loaded.packs.len(), 1);
}

#[test]
fn target_kind_checks() {
    let check = |run: &str| {
        let rule = format!(
            "id: acme/lint\ndescription: Lint.\nscope: {{language: python}}\nenforce:\n  - target: check\n    stage: lint\n    run: {run}\n"
        );
        tree(&[("acme/pack.yaml", PACK), ("acme/rules/lint.yaml", &rule)])
    };
    let err = check("ruff check {files}").unwrap_err();
    assert_eq!(err.kind, ErrorKind::Rule);
    assert!(
        err.message
            .contains("run ruff as \"forge_do ruff <operation>\""),
        "{err}"
    );
    // every dashed project tool has an adapter today, whose check comes first
    let err = check("vue-tsc --noEmit").unwrap_err();
    assert!(
        err.message
            .contains("run vue-tsc as \"forge_do vue-tsc <operation>\""),
        "{err}"
    );
    let err = check("bun install").unwrap_err();
    assert!(
        err.message
            .contains("run bun as \"forge_do bun <operation>\""),
        "{err}"
    );
    check("forge_helper bun x.mjs").unwrap();
    let ast = "id: acme/dep\ndescription: Deprecated.\nscope: {language: python}\nenforce:\n  - target: ast-grep\n    mode: forbid\n    rule: {pattern: $X}\n    where: {X: {deprecated: true}}\n";
    let err = tree(&[("acme/pack.yaml", PACK), ("acme/rules/dep.yaml", ast)]).unwrap_err();
    assert!(
        err.message
            .contains("where deprecated is answered for TypeScript only"),
        "{err}"
    );
    let err = check("forge_role @no-such-role \"$f\"").unwrap_err();
    assert!(
        err.message.contains("unknown file role @no-such-role"),
        "{err}"
    );
    check("forge_do ruff lint {files}").unwrap();
    check("echo 'ruff check is run by the adapter'").unwrap();
    check("forge_role @python-tests \"$f\"").unwrap();
}

/// A repository with local packs and rules.
fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), files);
    dir
}

#[test]
fn local_packs_and_rules() {
    let local_rule = "id: git/no-conflict-markers\ndescription: Our own conflict-marker rule.\nscope: {language: any, files: [\"src/**\"]}\nenforce: [{target: grep, mode: forbid, pattern: \"^<<<<<<< \"}]\n";
    let orphan = "id: nowhere/rule\ndescription: A rule of no pack.\nscope: {language: any}\nenforce: [{target: permission, decision: deny, rule: \"Bash(rm -rf /)\"}]\n";
    let inactive = "id: docker/local\ndescription: A rule of an inactive pack.\nscope: {language: any}\nenforce: [{target: permission, decision: deny, rule: \"Bash(docker system prune)\"}]\n";
    let r = repo(&[
        (
            ".forge/packs/acme/pack.yaml",
            &PACK.replace("detect: {always: true}\n", "requires: [git]\n"),
        ),
        (".forge/packs/acme/rules/no-todo.yaml", RULE),
        (".forge/rules/conflicts.yaml", local_rule),
        (".forge/rules/orphan.yaml", orphan),
        (".forge/rules/inactive.yaml", inactive),
        (".forge/rules/README.md", "not a rule"),
    ]);
    let set = forge_packs::load(Some(r.path()), Options::default()).unwrap();
    let acme = &set.packs["acme"];
    assert_eq!(acme.source, PackSource::Local);
    assert_eq!(acme.dir, ".forge/packs/acme");
    assert_eq!(acme.rules[0].file, ".forge/packs/acme/rules/no-todo.yaml");
    assert_eq!(acme.rules[0].origin, Origin::Pack);
    // built-in packs first, local ones after
    assert_eq!(set.packs.keys().last().map(String::as_str), Some("acme"));
    let files: Vec<&str> = set.local_rules.iter().map(|r| r.file.as_str()).collect();
    assert_eq!(
        files,
        [
            ".forge/rules/conflicts.yaml",
            ".forge/rules/inactive.yaml",
            ".forge/rules/orphan.yaml"
        ]
    );
    assert!(set.local_rules.iter().all(|r| r.origin == Origin::Local));

    let (rules, warnings) = set.select_rules(&["git", "acme"], Level::Project);
    let conflict = rules
        .iter()
        .find(|r| r.id == "git/no-conflict-markers")
        .unwrap();
    assert_eq!(
        conflict.origin,
        Origin::Local,
        "a local rule replaces the pack rule with its id"
    );
    assert!(
        rules.iter().any(|r| r.id == "nowhere/rule"),
        "a rule of no known pack is kept"
    );
    assert!(rules.iter().any(|r| r.id == "acme/no-todo"));
    assert!(!rules.iter().any(|r| r.id == "docker/local"));
    assert_eq!(
        warnings,
        [
            ".forge/rules/inactive.yaml: pack \"docker\" is not active here; rule docker/local is ignored"
        ]
    );
    let mut sorted: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
    sorted.sort();
    assert_eq!(
        rules.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        sorted
    );
    // at user level, local rules do not apply
    let (rules, _) = set.select_rules(&["git"], Level::User);
    assert!(rules.iter().all(|r| r.origin == Origin::Pack));

    // the dump of a repository shows where local packs come from
    let bundle = set.bundle(None);
    let last = bundle.as_array().unwrap().last().unwrap();
    assert_eq!(last["source"], "local");
    assert!(
        set.bundle(Some(PackSource::Builtin))
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p.get("source").is_none())
    );
}

#[test]
fn local_pack_errors() {
    let r = repo(&[(
        ".forge/packs/git/pack.yaml",
        "id: git\ndescription: Ours.\nlevel: project\n",
    )]);
    let err = forge_packs::load(Some(r.path()), Options::default()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Layout);
    assert_eq!(err.file, ".forge/packs/git/pack.yaml");
    assert!(err.message.contains("is a built-in pack's"), "{err}");

    let r = repo(&[(
        ".forge/packs/acme/pack.yaml",
        &PACK.replace("detect: {always: true}\n", "requires: [nope]\n"),
    )]);
    let err = forge_packs::load(Some(r.path()), Options::default()).unwrap_err();
    assert!(
        err.message
            .contains("pack acme: requires unknown pack nope"),
        "{err}"
    );

    let r = repo(&[(".forge/rules/bad.yaml", "id: acme/x\nid: acme/y\n")]);
    let err = forge_packs::load(Some(r.path()), Options::default()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Yaml);
    assert_eq!(err.file, ".forge/rules/bad.yaml");
}

#[test]
fn project_config() {
    let r = repo(&[]);
    assert_eq!(forge_packs::load_project_config(r.path()).unwrap(), None);
    write(
        r.path(),
        &[(
            "forge.yaml",
            "forge: \"0.5\"\npacks: {add: [docker]}\ndisable: [{id: git/large-files, reason: we vendor}]\n",
        )],
    );
    let config = forge_packs::load_project_config(r.path()).unwrap().unwrap();
    assert_eq!(config.forge, "0.5");
    assert_eq!(config.disable.unwrap()[0].id, "git/large-files");
    // 0.5 unquoted is a number: the schema wants a string
    write(r.path(), &[("forge.yaml", "forge: 0.5\n")]);
    assert_eq!(
        forge_packs::load_project_config(r.path()).unwrap_err().kind,
        ErrorKind::Schema
    );
    write(
        r.path(),
        &[("forge.yaml", "forge: \"0.5\"\nforge: \"0.6\"\n")],
    );
    assert_eq!(
        forge_packs::load_project_config(r.path()).unwrap_err().kind,
        ErrorKind::Yaml
    );
}

#[test]
fn cache_hit_miss_stale_corrupt() {
    let r = repo(&[
        (".forge/packs/acme/pack.yaml", PACK),
        (".forge/packs/acme/rules/no-todo.yaml", RULE),
    ]);
    let cache = tempfile::tempdir().unwrap();
    let opts = Options {
        cache_dir: Some(cache.path()),
    };
    let load = || forge_packs::load(Some(r.path()), opts).unwrap();

    let first = load();
    assert_eq!(first.cache.local, Some(CacheOutcome::Miss));
    let second = load();
    assert_eq!(
        (second.cache.builtin, second.cache.local),
        (CacheOutcome::Hit, Some(CacheOutcome::Hit))
    );
    assert_eq!(second.packs, first.packs);

    // another input: stale, reparsed
    write(
        r.path(),
        &[(
            ".forge/packs/acme/rules/no-todo.yaml",
            &RULE.replace("No TODO comments.", "No TODO comments, ever."),
        )],
    );
    let third = load();
    assert_eq!(third.cache.local, Some(CacheOutcome::Stale));
    assert_eq!(
        third.packs["acme"].rules[0].description,
        "No TODO comments, ever."
    );
    assert_eq!(load().cache.local, Some(CacheOutcome::Hit));

    // a file added to .forge/rules changes the key too
    write(
        r.path(),
        &[(
            ".forge/rules/x.yaml",
            &RULE.replace("acme/no-todo", "acme/x"),
        )],
    );
    let fourth = load();
    assert_eq!(fourth.cache.local, Some(CacheOutcome::Stale));
    assert_eq!(fourth.local_rules.len(), 1);

    // a corrupt entry is reparsed and rewritten
    for garbage in [&b"not msgpack"[..], &[]] {
        std::fs::write(cache.path().join("local-packs.msgpack"), garbage).unwrap();
        let fifth = load();
        assert_eq!(fifth.cache.local, Some(CacheOutcome::Corrupt));
        assert_eq!(fifth.packs, fourth.packs);
        assert_eq!(load().cache.local, Some(CacheOutcome::Hit));
    }
    // a truncated one too
    let bytes = std::fs::read(cache.path().join("builtin-packs.msgpack")).unwrap();
    std::fs::write(
        cache.path().join("builtin-packs.msgpack"),
        &bytes[..bytes.len() / 2],
    )
    .unwrap();
    assert_eq!(load().cache.builtin, CacheOutcome::Corrupt);
    assert_eq!(load().cache.builtin, CacheOutcome::Hit);

    // a load error is never cached
    write(r.path(), &[(".forge/rules/x.yaml", "id: [\n")]);
    assert!(forge_packs::load(Some(r.path()), opts).is_err());
    assert!(forge_packs::load(Some(r.path()), opts).is_err());
}

#[test]
fn lints_on_a_local_pack() {
    let cut = "id: acme/no-todo\ndescription: No TODO comments (# TODO, # FIXME) in code.\nscope: {language: any}\nenforce: [{target: grep, mode: forbid, pattern: TODO}]\n";
    let docs = "id: acme/docs\ndescription: \"Run forge apply after editing .forge/rules; see FORGE_DEBUG.\"\nscope: {language: any, files: [\"@docs\"]}\nenforce: [{target: grep, mode: forbid, pattern: TODO}]\n";
    let r = repo(&[
        (
            ".forge/packs/acme/pack.yaml",
            &format!("{PACK}context:\n  - text: Accepted findings go in forge.yaml (accept:) # or ask\n  - text: kube-linter rejects it; this is not checked by Forge.\n"),
        ),
        (".forge/packs/acme/rules/no-todo.yaml", cut),
        (".forge/packs/acme/rules/docs.yaml", docs),
    ]);
    let set = forge_packs::load(Some(r.path()), Options::default()).unwrap();
    let local: Vec<String> = set
        .findings
        .iter()
        .filter(|f| f.file.starts_with(".forge/"))
        .map(ToString::to_string)
        .collect();
    insta::assert_snapshot!(local.join("\n"));
}
