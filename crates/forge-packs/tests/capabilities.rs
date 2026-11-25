//! Capability files (§20.1): loading, the schema, local ones, the lints.

use std::path::Path;

use forge_packs::lint::LintKind;
use forge_packs::model::Deliver;
use forge_packs::{ErrorKind, Level, Options, Origin};

const PACK: &str =
    "id: acme\ndescription: Acme's conventions.\nlevel: project\ndetect: {always: true}\n";

/// A capability file of `id`, with `extra` lines appended.
fn cap(id: &str, extra: &str) -> String {
    format!(
        "id: {id}\ndescription: A persistent web browser, to act as a user would.\nrelevant:\n  - The task may involve something a person sees or does in a web page.\ndeliver: on-demand\ntext: |\n  Open pages with the browser.\n{extra}"
    )
}

fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let p = dir.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn load(root: &Path) -> Result<forge_packs::PackSet, forge_packs::LoadError> {
    forge_packs::load(Some(root), Options::default())
}

// A pack's capabilities/ holds its capabilities; each id is <pack id>/<file name>, and loading
// adds the file, its hash, its origin and its pack.
#[test]
fn a_pack_loads_its_capabilities() {
    let r = repo(&[
        (".forge/packs/acme/pack.yaml", PACK),
        (
            ".forge/packs/acme/capabilities/web-browser.yaml",
            &cap(
                "acme/web-browser",
                "threshold: 0.5\nexamples:\n  selected: [\"Fix the login page\"]\n  not_selected: [\"Bump the version\"]\n",
            ),
        ),
        (
            ".forge/packs/acme/capabilities/notes.txt",
            "not a capability",
        ),
    ]);
    let set = load(r.path()).unwrap();
    let c = &set.packs["acme"].capabilities[0];
    assert_eq!(set.packs["acme"].capabilities.len(), 1);
    assert_eq!(c.id, "acme/web-browser");
    assert_eq!(c.name(), "web-browser");
    assert_eq!(c.pack, "acme");
    assert_eq!(c.file, ".forge/packs/acme/capabilities/web-browser.yaml");
    assert_eq!(c.origin, Origin::Pack);
    assert_eq!(c.hash.len(), 64);
    assert_eq!(c.deliver, Deliver::OnDemand);
    assert_eq!(c.threshold(), 0.5);
    assert!(set.findings.is_empty(), "{:?}", set.findings);
    // `capabilities` is reserved: it is not a child pack
    assert_eq!(set.packs["acme"].children, Vec::<String>::new());
    // the dump holds them
    let dump = set.bundle(None);
    let acme = dump
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "acme")
        .unwrap();
    assert_eq!(acme["capabilities"][0]["id"], "acme/web-browser");
}

// The threshold is 0.3 when the file sets none.
#[test]
fn the_default_threshold_is_three_tenths() {
    let r = repo(&[
        (".forge/packs/acme/pack.yaml", PACK),
        (
            ".forge/packs/acme/capabilities/web-browser.yaml",
            &cap("acme/web-browser", ""),
        ),
    ]);
    let set = load(r.path()).unwrap();
    assert_eq!(set.packs["acme"].capabilities[0].threshold(), 0.3);
}

// A file its schema refuses, or whose id is not its place, stops the load, naming the file.
#[test]
fn what_loading_refuses() {
    let file = ".forge/packs/acme/capabilities/web-browser.yaml";
    for (extra_or_whole, want, kind) in [
        (cap("acme/other", ""), "id must be \"acme/web-browser\"", ErrorKind::Layout),
        (cap("acme/web-browser", "threshold: 1.5\n"), "threshold", ErrorKind::Schema),
        (
            cap("acme/web-browser", "").replace("on-demand", "later"),
            "deliver",
            ErrorKind::Schema,
        ),
        (cap("acme/web-browser", "grants: [Bash]\n"), "grants", ErrorKind::Schema),
        (
            cap("acme/web-browser", "").replace(
                "relevant:\n  - The task may involve something a person sees or does in a web page.\n",
                "relevant: []\n",
            ),
            "relevant",
            ErrorKind::Schema,
        ),
        (
            cap("acme/web-browser", "").replace(
                "description: A persistent web browser, to act as a user would.",
                "description: \"two\\nlines\"",
            ),
            "description",
            ErrorKind::Schema,
        ),
        (
            cap("acme/web-browser", "when: {facts: {python.pm: maven}}\n"),
            "python.pm",
            ErrorKind::Schema,
        ),
        (
            cap("acme/web-browser", "needs: {tools: [ruff, ruff]}\n"),
            "needs",
            ErrorKind::Schema,
        ),
        (
            cap("acme/web-browser", "when: {packs: [nowhere]}\n"),
            "unknown pack nowhere",
            ErrorKind::Layout,
        ),
    ] {
        let r = repo(&[(".forge/packs/acme/pack.yaml", PACK), (file, &extra_or_whole)]);
        let err = load(r.path()).expect_err(&extra_or_whole);
        assert_eq!(err.kind, kind, "{err}");
        assert!(err.to_string().contains(want), "{want}: {err}");
        assert!(err.to_string().contains("acme"), "{err}");
    }
}

// Local capabilities: a standalone file in .forge/capabilities/ with a pack capability's id
// replaces it; one of no known pack is kept; one for a known pack that is not active is
// ignored with a warning. They apply at project level only.
#[test]
fn local_capabilities_replace_and_add() {
    let r = repo(&[
        (".forge/packs/acme/pack.yaml", PACK),
        (
            ".forge/packs/acme/capabilities/web-browser.yaml",
            &cap("acme/web-browser", ""),
        ),
        (
            ".forge/capabilities/browser.yaml",
            &cap("acme/web-browser", "").replace("on-demand", "inject"),
        ),
        (".forge/capabilities/own.yaml", &cap("team/runbook", "")),
        (
            ".forge/capabilities/inactive.yaml",
            &cap("docker/compose-up", ""),
        ),
        (".forge/capabilities/README.md", "not a capability"),
    ]);
    let set = load(r.path()).unwrap();
    assert_eq!(set.local_capabilities.len(), 3);
    let (caps, warnings) = set.select_capabilities(&["acme"], Level::Project);
    let ids: Vec<&str> = caps.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["acme/web-browser", "team/runbook"]);
    assert_eq!(caps[0].origin, Origin::Local);
    assert_eq!(caps[0].deliver, Deliver::Inject);
    assert_eq!(caps[0].file, ".forge/capabilities/browser.yaml");
    assert_eq!(
        warnings,
        [
            ".forge/capabilities/inactive.yaml: pack \"docker\" is not active here; capability docker/compose-up is ignored"
        ]
    );
    // a pack that is not active gives none
    let (caps, _) = set.select_capabilities(&[], Level::Project);
    assert_eq!(
        caps.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["team/runbook"]
    );
    assert!(set.select_capabilities(&["acme"], Level::User).0.is_empty());
    assert_eq!(
        set.capability("acme/web-browser").unwrap().origin,
        Origin::Local
    );
}

// The lints: the agent-text lints on the description and the text, an injected text over 1,500
// characters, a relevant statement that is negated or spans lines, a threshold without examples
// on both sides. None stops loading.
#[test]
fn capability_lints() {
    let long = format!("text: |\n  {}\n", "word ".repeat(320));
    let r = repo(&[
        (".forge/packs/acme/pack.yaml", PACK),
        (
            ".forge/packs/acme/capabilities/noisy.yaml",
            &format!(
                "id: acme/noisy\ndescription: Run claude-forge to see it (FORGE_DEBUG).\nrelevant:\n  - The task does not involve a web page.\n  - |\n    The task changes\n    two lines.\n  - The task changes a page.\nthreshold: 0.6\nexamples:\n  selected: [\"Fix the page\"]\ndeliver: inject\n{long}"
            ),
        ),
        (
            ".forge/packs/acme/capabilities/short.yaml",
            &cap("acme/short", "").replace("on-demand", "inject"),
        ),
    ]);
    let set = load(r.path()).unwrap();
    let kinds = |id: &str| -> Vec<LintKind> {
        let mut k: Vec<LintKind> = set
            .findings
            .iter()
            .filter(|f| f.subject == id)
            .map(|f| f.lint)
            .collect();
        k.sort();
        k
    };
    assert_eq!(
        kinds("acme/noisy"),
        [
            LintKind::ForgeInternals,
            LintKind::ForgeInternals,
            LintKind::InjectSize,
            LintKind::RelevantStatement,
            LintKind::RelevantStatement,
            LintKind::ThresholdExamples,
        ],
        "{:#?}",
        set.findings
    );
    assert!(kinds("acme/short").is_empty());
    let negated = set
        .findings
        .iter()
        .find(|f| f.lint == LintKind::RelevantStatement)
        .unwrap();
    assert!(
        negated.message.contains("relevant/0 is negated"),
        "{negated}"
    );
    assert_eq!(negated.line, Some(4));
}

// The agent-text lints read a capability's description and text as they read context: a text
// cut by a YAML comment, and a text claiming a tool enforces something.
#[test]
fn context_lints_apply_to_capability_text() {
    let r = repo(&[
        (".forge/packs/acme/pack.yaml", PACK),
        (
            ".forge/packs/acme/capabilities/cut.yaml",
            "id: acme/cut\ndescription: A browser for pages # and more\nrelevant:\n  - The task involves a page.\ndeliver: on-demand\ntext: Pages are linted by the browser.\n",
        ),
    ]);
    let set = load(r.path()).unwrap();
    let mut kinds: Vec<LintKind> = set.findings.iter().map(|f| f.lint).collect();
    kinds.sort();
    assert_eq!(
        kinds,
        [LintKind::CutByComment, LintKind::EnforcementClaim],
        "{:#?}",
        set.findings
    );
    // `claude-forge use` is the one phrase of Forge's a capability may name
    let r = repo(&[
        (".forge/packs/acme/pack.yaml", PACK),
        (
            ".forge/packs/acme/capabilities/ok.yaml",
            "id: acme/ok\ndescription: A browser.\nrelevant:\n  - The task involves a page.\ndeliver: inject\ntext: Load the browser's guide with claude-forge use web-browser when needed.\n",
        ),
    ]);
    assert!(load(r.path()).unwrap().findings.is_empty());
}
