//! §6 (rules): what loading refuses beyond the schema (fields that would do nothing, or that
//! depend on each other), checked on local rules (`.forge/rules/`), which go through the same
//! loader as the built-in packs.

use forge_packs::{ErrorKind, Options};

/// Loads the built-in packs plus one local rule; the error message when it is refused.
fn load_rule(yaml: &str) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join(".forge/rules/r.yaml");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, yaml).unwrap();
    // the built-in packs parsed once for the whole file
    static CACHE: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| tempfile::tempdir().unwrap());
    let opts = Options {
        cache_dir: Some(cache.path()),
    };
    forge_packs::load(Some(dir.path()), opts)
        .map(|_| ())
        .map_err(|e| {
            assert!(matches!(e.kind, ErrorKind::Rule | ErrorKind::Schema), "{e}");
            e.to_string()
        })
}

const HEAD: &str = "id: acme/r\ndescription: A rule.\nscope: {language: python}\n";

fn with(enforce: &str) -> String {
    format!("{HEAD}enforce:\n{enforce}")
}

#[test]
fn fields_that_would_do_nothing_are_refused() {
    let refused = [
        (
            "  - {target: check, stage: lint, run: 'true', rerun: 'x {files}'}\n",
            "rerun is for stage test",
        ),
        (
            "  - {target: check, stage: test, run: 'true', format_stdin: 'x {file}'}\n",
            "format_stdin is for stages format and lint",
        ),
        (
            "  - {target: check, stage: test, run: 'true', config_files: [pyproject.toml]}\n",
            "config_files is for stage lint",
        ),
        (
            "  - {target: check, stage: lint, run: 'true', fix_lints: true, fix_run: 'x {files}'}\n",
            "fix_lints needs fix_run and changed_run",
        ),
        (
            "  - {target: guard, decision: deny}\n",
            "a guard target needs command or check",
        ),
        (
            "  - {target: ast-grep, mode: forbid, rule: {pattern: x}, needs: [{ecosystem: pypi, name: sqlalchemy}]}\n",
            "needs requires where",
        ),
        (
            "  - {target: ast-grep, mode: no-increase, rule: {pattern: $X}, where: {X: {type: {module: m}}}}\n",
            "a where target's mode is forbid or advise",
        ),
        (
            "  - {target: check, stage: lint, run: 'forge_role @no-such-role a.py'}\n",
            "unknown file role @no-such-role",
        ),
        (
            "  - {target: check, stage: lint, run: 'pytest -q'}\n",
            "run pytest as \"forge_do pytest <operation>\"",
        ),
    ];
    for (enforce, why) in refused {
        let e = load_rule(&with(enforce)).expect_err(enforce);
        assert!(e.contains(why), "{enforce}: {e}");
    }
    // a review is the question of an ask: a deny guard cannot have one
    let e = load_rule(&format!(
        "{}review: Is it needed?\n",
        with("  - {target: guard, decision: deny, command: {name: rm}}\n")
    ))
    .unwrap_err();
    assert!(e.contains("review is the question for an ask"), "{e}");
    load_rule(&format!(
        "{}review: Is it needed?\n",
        with("  - {target: guard, decision: ask, command: {name: rm}}\n")
    ))
    .unwrap();
    // a version gate names a version
    let e = load_rule(&format!(
        "{}when: {{deps: [{{ecosystem: pypi, name: fastapi}}]}}\n",
        with("  - {target: grep, mode: forbid, pattern: x}\n")
    ))
    .unwrap_err();
    assert!(e.contains("dependency fastapi needs a version"), "{e}");
}

#[test]
fn the_schema_refuses_unknown_fields_and_values() {
    for (enforce, why) in [
        (
            "  - {target: grep, mode: forbid, pattern: x, extra: 1}\n",
            "extra",
        ),
        (
            "  - {target: grep, mode: sometimes, pattern: x}\n",
            "sometimes",
        ),
        (
            "  - {target: guard, decision: deny, pattern: 'rm -rf'}\n",
            "pattern",
        ),
        ("  - {target: check, stage: build, run: 'true'}\n", "build"),
        (
            "  - {target: check, stage: test, run: 'true', timeout: 601}\n",
            "601",
        ),
    ] {
        let e = load_rule(&with(enforce)).expect_err(enforce);
        assert!(e.contains(why), "{enforce}: {e}");
    }
    // a description is one line
    let e = load_rule(
        "id: acme/r\ndescription: \"two\\nlines\"\nscope: {language: python}\nenforce: [{target: grep, mode: forbid, pattern: x}]\n",
    )
    .unwrap_err();
    assert!(e.contains("description"), "{e}");
}

#[test]
fn accepted_forms_load() {
    for enforce in [
        "  - {target: check, stage: test, run: 'forge_do pytest test', rerun: 'forge_do pytest test-files {files}', timeout: 300}\n",
        "  - {target: check, stage: lint, run: 'forge_do ruff lint', changed_run: 'forge_do ruff lint {files}', fix_run: 'forge_do ruff fix {files}', fix_lints: true, config_files: [pyproject.toml]}\n",
        "  - {target: guard, decision: ask, check: ephemeral-exec}\n",
        "  - {target: guard, decision: deny, command: {name: git, sub: [push], flags_any: [--force, -f]}, reason: Force-pushing rewrites shared history.}\n",
        "  - {target: ast-grep, mode: advise, rule: {pattern: $S.query()}, where: {S: {type: {module: sqlalchemy.orm, name: Session}}}, needs: [{ecosystem: pypi, name: sqlalchemy}]}\n",
        "  - {target: grep, mode: no-increase, pattern: '# noqa', ignore_case: true}\n",
    ] {
        load_rule(&with(enforce)).unwrap_or_else(|e| panic!("{enforce}: {e}"));
    }
}
