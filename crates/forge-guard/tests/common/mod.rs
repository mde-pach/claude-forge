#![allow(dead_code)]

use std::path::Path;
use std::sync::LazyLock;

use forge_guard::{GuardCtx, GuardRule, GuardSet};
use forge_packs::{Options, PackSet, Rule};

pub static PACKS: LazyLock<PackSet> =
    LazyLock::new(|| forge_packs::load_builtin(Options::default()).expect("built-in packs load"));

/// Every built-in rule with a guard target.
pub fn guard_rules() -> Vec<&'static Rule> {
    PACKS
        .packs
        .values()
        .flat_map(|p| &p.rules)
        .filter(|r| r.enforce.iter().any(|t| t.kind() == "guard"))
        .collect()
}

/// All guard rules of all packs.
pub fn guard_set() -> GuardSet {
    GuardSet::new(
        guard_rules()
            .into_iter()
            .filter_map(GuardRule::from_pack)
            .collect(),
    )
    .expect("guard rules compile")
}

/// A repository holding `files` (and a `.git`).
pub fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(d.path().join(".git")).expect(".git");
    for (p, c) in files {
        write(d.path(), p, c);
    }
    d
}

pub fn write(root: &Path, p: &str, c: &str) {
    let f = root.join(p);
    std::fs::create_dir_all(f.parent().expect("parent")).expect("dirs");
    std::fs::write(&f, c).expect("write");
    #[cfg(unix)]
    if p.contains("/bin/") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

/// The corpus repository: `node_modules/.bin` holds tsc and vitest, and there is a
/// `packages/app`.
pub fn corpus_repo() -> tempfile::TempDir {
    repo(&[
        ("node_modules/.bin/tsc", ""),
        ("node_modules/.bin/vitest", ""),
        ("packages/app/src/a.test.ts", ""),
    ])
}

pub fn ctx(root: &Path) -> GuardCtx<'static> {
    // canonical: the temp dir may be behind a symlink (/var → /private/var on macOS)
    let root = root.canonicalize().expect("canonical");
    GuardCtx::new(root.clone(), root).with_home(Some("/home/u".into()))
}
