//! The built-in packs have no content-lint findings: a finding fails the test, with the list.

#[test]
fn builtin_packs_have_no_findings() {
    let set =
        forge_packs::load_builtin(forge_packs::Options::default()).expect("built-in packs load");
    assert!(
        set.findings.is_empty(),
        "forge packs lint:\n{}",
        forge_packs::lint::render(&set.findings)
    );
}
