//! The content lints' findings on the built-in packs, as a snapshot: fixing a pack (or
//! adding a problem) shows up as a diff of `snapshots/lints__builtin_findings.snap`.

#[test]
fn builtin_findings() {
    let set =
        forge_packs::load_builtin(forge_packs::Options::default()).expect("built-in packs load");
    insta::assert_snapshot!(forge_packs::lint::render(&set.findings));
}
