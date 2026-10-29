//! Every tool Forge provisions, with its pinned version and, for downloads, each platform's
//! sha256: the one table `forge apply` writes into `forge.lock`'s tool pins ([`lock_pins`]).
//!
//! Versions follow Forge's pinning rule: the newest release older than 14 days when the pin
//! was taken. The sha256 of each download asset was computed from the asset and checked
//! against the release's own checksum file.
//!
//! Platform keys are `uname -s`-`uname -m` (`Darwin-arm64` on Apple silicon), with a
//! `-musl` suffix for the few tools that ship separate musl builds (Bun).

use std::collections::BTreeMap;

/// The Python the uv-installed tools run on (ty's engine included), installed by uv as a
/// managed Python so it never depends on the machine's own.
pub const TOOLS_PYTHON: &str = "3.12";

/// One platform's release asset and its sha256.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    pub platform: &'static str,
    /// The file name, appended to the release's base URL.
    pub name: &'static str,
    pub sha256: &'static str,
}

pub const fn asset(platform: &'static str, name: &'static str, sha256: &'static str) -> Asset {
    Asset {
        platform,
        name,
        sha256,
    }
}

/// Where a tool comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// uv itself: a release archive holding `uv` and `uvx`.
    Uv {
        base: &'static str,
        assets: &'static [Asset],
    },
    /// Bun, provisioned only to run the npm-only tools.
    Bun {
        base: &'static str,
        assets: &'static [Asset],
    },
    /// A GitHub release asset: a `.tar.gz` or `.zip` holding `binary`, or the binary itself.
    Release {
        base: &'static str,
        binary: &'static str,
        assets: &'static [Asset],
    },
    /// A PyPI package installed with Forge's uv (`uv tool install <package>==<version>`).
    UvTool {
        package: &'static str,
        /// The executable the tool is run as.
        binary: &'static str,
        /// When the wheel repackages an upstream binary: that binary's sha256 per platform,
        /// checked after the install.
        binary_sha256: &'static [(&'static str, &'static str)],
    },
    /// An npm package run with Forge's Bun (`bun install <package>@<version>`).
    Npm {
        package: &'static str,
        binary: &'static str,
    },
}

/// A tool and its pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    /// The name `resolve`/`ensure` take, and the cache directory's (`tools/<name>/<version>`).
    pub name: &'static str,
    pub version: &'static str,
    pub source: Source,
}

impl ToolSpec {
    /// The asset for the first of `keys` the table has.
    pub fn asset_for(&self, keys: &[String]) -> Option<(&'static str, Asset)> {
        let (base, assets) = match self.source {
            Source::Uv { base, assets }
            | Source::Bun { base, assets }
            | Source::Release { base, assets, .. } => (base, assets),
            Source::UvTool { .. } | Source::Npm { .. } => return None,
        };
        keys.iter().find_map(|k| {
            assets
                .iter()
                .find(|a| a.platform == k.as_str())
                .map(|a| (base, *a))
        })
    }
}

/// A set of tool pins. [`Catalog::builtin`] is Forge's; tests build their own.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub tools: Vec<ToolSpec>,
}

impl Catalog {
    pub fn builtin() -> Catalog {
        Catalog {
            tools: TOOLS.to_vec(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// The first tool of the given source kind (`uv`, `bun`).
    pub fn find(&self, pred: impl Fn(&Source) -> bool) -> Option<&ToolSpec> {
        self.tools.iter().find(|t| pred(&t.source))
    }
}

/// `name → version` for every pinned tool, plus `python`: what `forge apply` writes into
/// `forge.lock`'s `[tools]` table.
pub fn lock_pins() -> BTreeMap<String, String> {
    let mut m: BTreeMap<String, String> = TOOLS
        .iter()
        .map(|t| (t.name.to_owned(), t.version.to_owned()))
        .collect();
    m.insert("python".to_owned(), TOOLS_PYTHON.to_owned());
    m
}

/// The pins in a lock that differ from this Forge's table: `(tool, locked, this Forge's)`.
/// A difference means the lock was written by another Forge version; `forge apply`
/// rewrites it.
pub fn pin_differences(locked: &BTreeMap<String, String>) -> Vec<(String, String, String)> {
    let ours = lock_pins();
    let mut out = Vec::new();
    for (name, v) in locked {
        match ours.get(name) {
            Some(o) if o == v => {}
            Some(o) => out.push((name.clone(), v.clone(), o.clone())),
            None => out.push((name.clone(), v.clone(), String::new())),
        }
    }
    for (name, o) in &ours {
        if !locked.contains_key(name) {
            out.push((name.clone(), String::new(), o.clone()));
        }
    }
    out
}

pub const UV_VERSION: &str = "0.13.1";
pub const BUN_VERSION: &str = "1.4.3";

/// Every tool Forge provisions.
pub static TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "uv",
        version: UV_VERSION,
        source: Source::Uv {
            base: "https://github.com/astral-sh/uv/releases/download/0.13.1/",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "uv-x86_64-unknown-linux-musl.tar.gz",
                    "6c86040a8f045ab9adc9e3ec4cdafa03fd00e9a988ed2bf28971c7fc2d0debd0",
                ),
                asset(
                    "Linux-aarch64",
                    "uv-aarch64-unknown-linux-musl.tar.gz",
                    "345a8308e4d1835cdfaadf692836fd3137a6d45864773f05ad7bf59442b1ba16",
                ),
                asset(
                    "Darwin-arm64",
                    "uv-aarch64-apple-darwin.tar.gz",
                    "cfe454573412ff7380d15fc7dae5997ab53ffce72fce0a78e91ddbc84001acd2",
                ),
                asset(
                    "Darwin-x86_64",
                    "uv-x86_64-apple-darwin.tar.gz",
                    "bb7b805bdcf3d0bc8c74ceb94569e6b3e8aa35bd492acdbb9d46bf46d9d4ff6b",
                ),
            ],
        },
    },
    ToolSpec {
        name: "bun",
        version: BUN_VERSION,
        source: Source::Bun {
            base: "https://github.com/oven-sh/bun/releases/download/bun-v1.4.3/",
            // The x86_64 builds are Bun's "baseline" ones, which need no AVX2.
            assets: &[
                asset(
                    "Linux-x86_64",
                    "bun-linux-x64-baseline.zip",
                    "1fc2edac843102909e3a1be1d8d9802cc6071cf074e67e88231f4ffe0f8b397b",
                ),
                asset(
                    "Linux-x86_64-musl",
                    "bun-linux-x64-musl-baseline.zip",
                    "4b5b0b5503f6ebf15161e8c56eb5a263134c05cd98a86dc11ee43ac5bda9862e",
                ),
                asset(
                    "Linux-aarch64",
                    "bun-linux-aarch64.zip",
                    "efa9813da5ed72423bf847f916e8d2c47c0d776add972354026a75e10da9aa21",
                ),
                asset(
                    "Linux-aarch64-musl",
                    "bun-linux-aarch64-musl.zip",
                    "8de8a0003cc2707bb555887e3d450e24e1edf3f605f491393e89ed61a8853420",
                ),
                asset(
                    "Darwin-arm64",
                    "bun-darwin-aarch64.zip",
                    "80afd4c069b46a6fa8f3ec520bebc750a7843e4a8d5b9583d29ad436c00a3403",
                ),
                asset(
                    "Darwin-x86_64",
                    "bun-darwin-x64-baseline.zip",
                    "0fde04e8b092156b97fe6432e899baf16d9ed6ce52cbb4218d2d9c07a2f827ef",
                ),
            ],
        },
    },
    // The types engine (ty's language server).
    ToolSpec {
        name: "ty",
        version: "0.0.86",
        source: Source::UvTool {
            package: "ty",
            binary: "ty",
            binary_sha256: &[],
        },
    },
    // GitHub Actions auditor.
    ToolSpec {
        name: "zizmor",
        version: "1.30.1",
        source: Source::UvTool {
            package: "zizmor",
            binary: "zizmor",
            binary_sha256: &[],
        },
    },
    // Dockerfile linter: the hadolint-bin wheel, whose binary must be the upstream release's
    // (https://github.com/hadolint/hadolint/releases/tag/v2.15.1).
    ToolSpec {
        name: "hadolint",
        version: "2.15.1",
        source: Source::UvTool {
            package: "hadolint-bin",
            binary: "hadolint",
            binary_sha256: &[
                (
                    "Linux-x86_64",
                    "c7187db94eeeeca956519a6af171adc31453941a1e777961f6e680f697c8c507",
                ),
                (
                    "Linux-aarch64",
                    "f6198ef8090f404dbb771abfee086eb8c48ac177f30da7fd3510aca35b344b5d",
                ),
                (
                    "Darwin-arm64",
                    "5c09f3213f8e40406abe048233d985eebef336d4a6a20021be47fadb6cf480a2",
                ),
                (
                    "Darwin-x86_64",
                    "ffe9bb18b23d5ed1eae50237aecdbb523d016e96da0bd4e7aa432040acfc3fde",
                ),
            ],
        },
    },
    // Postgres migration linter.
    ToolSpec {
        name: "squawk",
        version: "2.70.0",
        source: Source::UvTool {
            package: "squawk-cli",
            binary: "squawk",
            binary_sha256: &[],
        },
    },
    // ast-grep's CLI, for whoever needs the executable (the checker links ast-grep itself).
    ToolSpec {
        name: "ast-grep",
        version: "0.45.3",
        source: Source::UvTool {
            package: "ast-grep-cli",
            binary: "ast-grep",
            binary_sha256: &[],
        },
    },
    // OpenAPI linters, the project's config choosing.
    ToolSpec {
        name: "redocly",
        version: "2.62.1",
        source: Source::Npm {
            package: "@redocly/cli",
            binary: "redocly",
        },
    },
    ToolSpec {
        name: "spectral",
        version: "6.17.0",
        source: Source::Npm {
            package: "@stoplight/spectral-cli",
            binary: "spectral",
        },
    },
    // package-lock.json hosts, https and integrity.
    ToolSpec {
        name: "lockfile-lint",
        version: "5.1.0",
        source: Source::Npm {
            package: "lockfile-lint",
            binary: "lockfile-lint",
        },
    },
    // Tools that ship only as GitHub release binaries. A tflint ruleset is fetched like a
    // tool, never by `tflint --init` (GitHub's API).
    ToolSpec {
        name: "actionlint",
        version: "1.7.12",
        source: Source::Release {
            base: "https://github.com/rhysd/actionlint/releases/download/v1.7.12/",
            binary: "actionlint",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "actionlint_1.7.12_linux_amd64.tar.gz",
                    "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8",
                ),
                asset(
                    "Linux-aarch64",
                    "actionlint_1.7.12_linux_arm64.tar.gz",
                    "325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6",
                ),
                asset(
                    "Darwin-arm64",
                    "actionlint_1.7.12_darwin_arm64.tar.gz",
                    "aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f",
                ),
                asset(
                    "Darwin-x86_64",
                    "actionlint_1.7.12_darwin_amd64.tar.gz",
                    "5b44c3bc2255115c9b69e30efc0fecdf498fdb63c5d58e17084fd5f16324c644",
                ),
            ],
        },
    },
    ToolSpec {
        name: "gitleaks",
        version: "8.30.1",
        source: Source::Release {
            base: "https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/",
            binary: "gitleaks",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "gitleaks_8.30.1_linux_x64.tar.gz",
                    "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb",
                ),
                asset(
                    "Linux-aarch64",
                    "gitleaks_8.30.1_linux_arm64.tar.gz",
                    "e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080",
                ),
                asset(
                    "Darwin-arm64",
                    "gitleaks_8.30.1_darwin_arm64.tar.gz",
                    "b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5",
                ),
                asset(
                    "Darwin-x86_64",
                    "gitleaks_8.30.1_darwin_x64.tar.gz",
                    "dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709",
                ),
            ],
        },
    },
    ToolSpec {
        name: "kubeconform",
        version: "0.8.0",
        source: Source::Release {
            base: "https://github.com/yannh/kubeconform/releases/download/v0.8.0/",
            binary: "kubeconform",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "kubeconform-linux-amd64.tar.gz",
                    "9bc2bffbf71f261128533edaf912153948b7ff238f9a531ae6d34466ec287883",
                ),
                asset(
                    "Linux-aarch64",
                    "kubeconform-linux-arm64.tar.gz",
                    "1f53fc8e81258197a35e8603054162a5af1de8c5af13746c71ab680d9534ed87",
                ),
                asset(
                    "Darwin-arm64",
                    "kubeconform-darwin-arm64.tar.gz",
                    "f84f4dfbebf4a6b0b230385fa065a39ea35e02608c2b50d025dcf64775a69d67",
                ),
                asset(
                    "Darwin-x86_64",
                    "kubeconform-darwin-amd64.tar.gz",
                    "71dbc87ac9f24099a62b93570e65aa06312ba6ac8aea63b7f86e9d999edf5a92",
                ),
            ],
        },
    },
    ToolSpec {
        name: "kube-linter",
        version: "0.8.3",
        source: Source::Release {
            base: "https://github.com/stackrox/kube-linter/releases/download/v0.8.3/",
            binary: "kube-linter",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "kube-linter-linux.tar.gz",
                    "1a6d8419b11971372971fdbc22682b684ebfb7cf1c39591662d1b6ca736c41df",
                ),
                asset(
                    "Linux-aarch64",
                    "kube-linter-linux_arm64.tar.gz",
                    "802e1b09eabd08f6f0a060a6b8ab2bf7bc7e6bf4f673bb2692303704c84b3e22",
                ),
                asset(
                    "Darwin-arm64",
                    "kube-linter-darwin_arm64.tar.gz",
                    "6e3443a8ff8625a9fc31a38682c783988d7559018f7ff707a4f8c77c18c92f14",
                ),
                asset(
                    "Darwin-x86_64",
                    "kube-linter-darwin.tar.gz",
                    "c62e8af3c9df2557c7a3922119ea1b35597794737d1ccad493f63a0d66e7b8fc",
                ),
            ],
        },
    },
    ToolSpec {
        name: "tflint",
        version: "0.64.0",
        source: Source::Release {
            base: "https://github.com/terraform-linters/tflint/releases/download/v0.64.0/",
            binary: "tflint",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "tflint_linux_amd64.zip",
                    "cca9d13e2e1d7a2c627af60ff899a3c9b74212899416aeb96ec764d2ef954537",
                ),
                asset(
                    "Linux-aarch64",
                    "tflint_linux_arm64.zip",
                    "560da89aacf59389d4eb029730dd5b109b7288096c32f2726a0d9e783a5ea8eb",
                ),
                asset(
                    "Darwin-arm64",
                    "tflint_darwin_arm64.zip",
                    "2496e9cb3d24992d553b45e7c87a0fdc9449ca975233876247a9bfeda857e6c0",
                ),
                asset(
                    "Darwin-x86_64",
                    "tflint_darwin_amd64.zip",
                    "0f3a9fd17526014646a2dfc3f9122f7b4161abe3d6b0f0f03f9014483ddf4d19",
                ),
            ],
        },
    },
    ToolSpec {
        name: "tflint-ruleset-aws",
        version: "0.49.0",
        source: Source::Release {
            base: "https://github.com/terraform-linters/tflint-ruleset-aws/releases/download/v0.49.0/",
            binary: "tflint-ruleset-aws",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "tflint-ruleset-aws_linux_amd64.zip",
                    "98a530130d33efd49408f3d0ab9198072192ede413cc2a2ce38295d0922eb1c6",
                ),
                asset(
                    "Linux-aarch64",
                    "tflint-ruleset-aws_linux_arm64.zip",
                    "94f4f965a959d1699ae59687ffccc25031f8aaeae6f49756e5d8ca6e36d9f95b",
                ),
                asset(
                    "Darwin-arm64",
                    "tflint-ruleset-aws_darwin_arm64.zip",
                    "246818b24c65a60d2b95d7c88921480d72bdca89cfbb936f23bb583b8c631f87",
                ),
                asset(
                    "Darwin-x86_64",
                    "tflint-ruleset-aws_darwin_amd64.zip",
                    "f68a67291f0635911793934b9d02f9f3aa654264f11364eca98c06af3a4d60d5",
                ),
            ],
        },
    },
    ToolSpec {
        name: "tflint-ruleset-google",
        version: "0.40.0",
        source: Source::Release {
            base: "https://github.com/terraform-linters/tflint-ruleset-google/releases/download/v0.40.0/",
            binary: "tflint-ruleset-google",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "tflint-ruleset-google_linux_amd64.zip",
                    "a5c02b3de937456de2521155831eaf1cc876014a21445bf7dd1621654511fe28",
                ),
                asset(
                    "Linux-aarch64",
                    "tflint-ruleset-google_linux_arm64.zip",
                    "cdda9b8ef8647885ca81ba2d9c6b1a213a1f608860452d79a9c0ac7de9b2750a",
                ),
                asset(
                    "Darwin-arm64",
                    "tflint-ruleset-google_darwin_arm64.zip",
                    "93e1a6f2c196e9ba5fa6359cd8d2116d18c1770bddb09bf333e0617e216a5e22",
                ),
                asset(
                    "Darwin-x86_64",
                    "tflint-ruleset-google_darwin_amd64.zip",
                    "9b32d94e4b32ae35b67dcddef02932a970e09a38421448d33160109b0b91c1d0",
                ),
            ],
        },
    },
    ToolSpec {
        name: "tflint-ruleset-azurerm",
        version: "0.32.0",
        source: Source::Release {
            base: "https://github.com/terraform-linters/tflint-ruleset-azurerm/releases/download/v0.32.0/",
            binary: "tflint-ruleset-azurerm",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "tflint-ruleset-azurerm_linux_amd64.zip",
                    "f1150b2dea7aef001ad07002671c75b5495f4521ddf3538ededf00896f7b0aaa",
                ),
                asset(
                    "Linux-aarch64",
                    "tflint-ruleset-azurerm_linux_arm64.zip",
                    "70510c58a2d550c548645c42a5ff8acc93940a7dd9ce18a5de207c326f5f3cd3",
                ),
                asset(
                    "Darwin-arm64",
                    "tflint-ruleset-azurerm_darwin_arm64.zip",
                    "f0140f15332e15e710f149d53cf9cbc2d48da051fadd6eec84f52db6094dc43f",
                ),
                asset(
                    "Darwin-x86_64",
                    "tflint-ruleset-azurerm_darwin_amd64.zip",
                    "f956b4c80eee7d2ae1b40ffb9748d8ef2248cc91417500231e17c85ff0a7e849",
                ),
            ],
        },
    },
    ToolSpec {
        name: "trivy",
        version: "0.75.0",
        source: Source::Release {
            base: "https://github.com/aquasecurity/trivy/releases/download/v0.75.0/",
            binary: "trivy",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "trivy_0.75.0_Linux-64bit.tar.gz",
                    "c6e65abddb348e25f10549df887045629cf28cc72453cd1c63acb717316b3f3f",
                ),
                asset(
                    "Linux-aarch64",
                    "trivy_0.75.0_Linux-ARM64.tar.gz",
                    "a1ee9f6ffb7d112b64ff726a2a0717c21175c1114361391f4a132956751a13b3",
                ),
                asset(
                    "Darwin-arm64",
                    "trivy_0.75.0_macOS-ARM64.tar.gz",
                    "4a77108cccf8e55c8d6823e1e759939a622277e66cd0daa3c1fc621ed69e4568",
                ),
                asset(
                    "Darwin-x86_64",
                    "trivy_0.75.0_macOS-64bit.tar.gz",
                    "291edaa9778acbe4693d067b5ad60ee11570e5ac68296e85595417528ca641e4",
                ),
            ],
        },
    },
    ToolSpec {
        name: "osv-scanner",
        version: "2.7.0",
        source: Source::Release {
            base: "https://github.com/google/osv-scanner/releases/download/v2.7.0/",
            binary: "osv-scanner",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "osv-scanner_linux_amd64",
                    "160c6253e2922e9e0a6a9771c5c4c40a4b3c6090b34a3cdd5e80c96cc2e4218b",
                ),
                asset(
                    "Linux-aarch64",
                    "osv-scanner_linux_arm64",
                    "4061ef4c021ed3c47f203b818f337e351336cc5cd67c0274fc36bd1dfc728dd7",
                ),
                asset(
                    "Darwin-arm64",
                    "osv-scanner_darwin_arm64",
                    "d4926ef8da134332543be276dff157e4c170f99ad91fc48e89448a8816bdf70b",
                ),
                asset(
                    "Darwin-x86_64",
                    "osv-scanner_darwin_amd64",
                    "e82c5df7ab0a42dff29a1cd0660711447b56b4c0483ff66ad8634df10fd81a45",
                ),
            ],
        },
    },
    ToolSpec {
        name: "oasdiff",
        version: "1.33.0",
        source: Source::Release {
            base: "https://github.com/oasdiff/oasdiff/releases/download/v1.33.0/",
            binary: "oasdiff",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "oasdiff_1.33.0_linux_amd64.tar.gz",
                    "43a4e328e2d13ba1552d760aa68d2485c75c5621f309f6ff64ae895188345247",
                ),
                asset(
                    "Linux-aarch64",
                    "oasdiff_1.33.0_linux_arm64.tar.gz",
                    "4ae3c362d6074d919aada2dea82d0ee84366591600384455d0bdc658ddf8f7ae",
                ),
                asset(
                    "Darwin-arm64",
                    "oasdiff_1.33.0_darwin_all.tar.gz",
                    "2a479337c15afdcbf0b1e89c0b4d0cf0176472dbc11483358fed1f831d1e46c5",
                ),
                asset(
                    "Darwin-x86_64",
                    "oasdiff_1.33.0_darwin_all.tar.gz",
                    "2a479337c15afdcbf0b1e89c0b4d0cf0176472dbc11483358fed1f831d1e46c5",
                ),
            ],
        },
    },
    ToolSpec {
        name: "editorconfig-checker",
        version: "4.0.2",
        source: Source::Release {
            base: "https://github.com/editorconfig-checker/editorconfig-checker/releases/download/v4.0.2/",
            binary: "editorconfig-checker",
            assets: &[
                asset(
                    "Linux-x86_64",
                    "editorconfig-checker-linux-amd64.tar.gz",
                    "44efaef3ed14c6ab52eb87481d78e5e5c204796dccf26e2f4598a61ba17f8003",
                ),
                asset(
                    "Linux-aarch64",
                    "editorconfig-checker-linux-arm64.tar.gz",
                    "abad76da8c9950fdc2a314bcd66969172aa823e9c595fa7e3a389a91e34d025e",
                ),
                asset(
                    "Darwin-arm64",
                    "editorconfig-checker-darwin-all.tar.gz",
                    "ff59f57717d95fc62b8e372c8051dbbb766f7d660942409a85d027b0feb2422c",
                ),
                asset(
                    "Darwin-x86_64",
                    "editorconfig-checker-darwin-all.tar.gz",
                    "ff59f57717d95fc62b8e372c8051dbbb766f7d660942409a85d027b0feb2422c",
                ),
            ],
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_well_formed() {
        let mut names = std::collections::HashSet::new();
        for t in TOOLS {
            assert!(names.insert(t.name), "duplicate {}", t.name);
            forge_home::check_version(t.version).unwrap();
            let assets: &[Asset] = match t.source {
                Source::Uv { base, assets }
                | Source::Bun { base, assets }
                | Source::Release { base, assets, .. } => {
                    assert!(base.starts_with("https://github.com/") && base.ends_with('/'));
                    assert!(
                        base.contains(t.version),
                        "{}: base names another version",
                        t.name
                    );
                    assets
                }
                _ => &[],
            };
            for a in assets {
                assert_eq!(a.sha256.len(), 64, "{}", t.name);
                assert!(
                    a.sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                );
            }
            if !assets.is_empty() {
                for k in [
                    "Linux-x86_64",
                    "Linux-aarch64",
                    "Darwin-arm64",
                    "Darwin-x86_64",
                ] {
                    assert!(
                        assets.iter().any(|a| a.platform == k),
                        "{} has no {k}",
                        t.name
                    );
                }
            }
        }
        let pins = lock_pins();
        assert_eq!(pins["uv"], UV_VERSION);
        assert_eq!(pins["python"], TOOLS_PYTHON);
        assert!(pin_differences(&pins).is_empty());
    }

    #[test]
    fn musl_lookup_prefers_musl_assets() {
        let bun = Catalog::builtin().get("bun").copied().unwrap();
        let keys = vec!["Linux-x86_64-musl".to_owned(), "Linux-x86_64".to_owned()];
        assert_eq!(
            bun.asset_for(&keys).unwrap().1.name,
            "bun-linux-x64-musl-baseline.zip"
        );
        let uv = Catalog::builtin().get("uv").copied().unwrap();
        assert_eq!(
            uv.asset_for(&keys).unwrap().1.name,
            "uv-x86_64-unknown-linux-musl.tar.gz"
        );
    }
}
