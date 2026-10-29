//! Upstream drift, opt-in (the weekly workflow): `FORGE_TEST_UPSTREAM=1 cargo test -p forge-check
//! --test upstream`. For every adapter of a tool published to npm or PyPI, the newest release
//! must not be past what the adapter was proven on (`verified`): a newer minor means its
//! changelog needs reading and the adapter's ranges extending.

use forge_check::adapters::{Adapter, cmp_version};

/// The registry package each adapter's releases are published as.
const REGISTRY: &[(&str, &str, &str)] = &[
    ("alembic", "pypi", "alembic"),
    ("pytest", "pypi", "pytest"),
    ("sqlfluff", "pypi", "sqlfluff"),
    ("astro", "npm", "astro"),
    ("biome", "npm", "@biomejs/biome"),
    ("eslint", "npm", "eslint"),
    ("graphql-codegen", "npm", "@graphql-codegen/cli"),
    ("mikro-orm", "npm", "@mikro-orm/cli"),
    ("ngc", "npm", "@angular/compiler-cli"),
    ("npm", "npm", "npm"),
    ("oxlint", "npm", "oxlint"),
    ("playwright", "npm", "@playwright/test"),
    ("pnpm", "npm", "pnpm"),
    ("prettier", "npm", "prettier"),
    ("prisma", "npm", "prisma"),
    ("tsc", "npm", "typescript"),
    ("typeorm", "npm", "typeorm"),
    ("vitest", "npm", "vitest"),
    ("vue-tsc", "npm", "vue-tsc"),
    ("wrangler", "npm", "wrangler"),
];

fn latest(registry: &str, package: &str) -> Result<String, String> {
    let url = match registry {
        "npm" => format!("https://registry.npmjs.org/{package}/latest"),
        _ => format!("https://pypi.org/pypi/{package}/json"),
    };
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "30", &url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("{url}: {e}"))?;
    v.get("version")
        .or_else(|| v.get("info").and_then(|i| i.get("version")))
        .and_then(|x| x.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{url}: no version"))
}

fn minor(v: &str) -> String {
    v.split('.').take(2).collect::<Vec<_>>().join(".")
}

#[test]
fn newest_releases_are_ones_the_adapters_were_proven_on() {
    if std::env::var_os("FORGE_TEST_UPSTREAM").is_none_or(|v| v != "1") {
        eprintln!("skipped: set FORGE_TEST_UPSTREAM=1 to compare with the registries");
        return;
    }
    let mut failures = vec![];
    for (tool, registry, package) in REGISTRY {
        let adapter = Adapter::load(tool).unwrap_or_else(|| panic!("no adapter for {tool}"));
        match latest(registry, package) {
            // a prerelease under the latest tag (8.0.0-rc) is not a release
            Ok(v) if v.contains('-') => {}
            Ok(v) => {
                if cmp_version(&minor(&v), &minor(&adapter.verified)).is_gt() {
                    failures.push(format!(
                        "{tool}: {package} {v} is newer than the adapter's verified {}",
                        adapter.verified
                    ));
                }
            }
            Err(e) => failures.push(format!("{tool}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_registry_tool_has_an_adapter() {
    for (tool, _, _) in REGISTRY {
        assert!(Adapter::load(tool).is_some(), "{tool}");
    }
}
