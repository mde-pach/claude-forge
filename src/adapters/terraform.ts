/**
 * Terraform, from PATH (§16). The project may use OpenTofu instead: `forge_tf_bin` (tools.sh)
 * prints `tofu` for a repository with *.tofu files or a lock file naming registry.opentofu.org,
 * and its adapter (tofu.ts) then drives the check; rules name `terraform`. What changed between
 * Terraform's releases, for what Forge asks of it (terraform/fmt: `terraform fmt -check
 * -recursive`, `terraform fmt <files>`; terraform/validate, forge_tf_validate: `terraform
 * -chdir=<dir> validate -no-color`, which runs no `init`: a directory that needs one is reported
 * skipped from the output's `terraform init`):
 *
 * - 1.0.0 is the oldest covered release: the 1.x line, under Terraform's v1.0 compatibility
 *   promises (developer.hashicorp.com/terraform/language/v1-compatibility-promises); every op
 *   below exists there (`-chdir` is from 0.14) (verified on 1.0.2, the first release with a
 *   darwin_arm64 build). 0.x releases are not covered.
 * - 1.3.0 let `terraform fmt` take several targets (v1.3/CHANGELOG.md 1.3.0, ENHANCEMENTS:
 *   "`terraform fmt` now accepts multiple target paths, allowing formatting of several
 *   individual files at once" #28191). Before, `fmt a.tf b.tf` is rejected (`The fmt command
 *   expects at most one argument.`, exit 1): `format` (the caller's files) cannot.
 * - Since: `fmt` also formats .tftest.hcl (1.6), .tfmock.hcl (1.7 #34580), .tfquery.hcl (1.15
 *   #38398); validate checks more (1.1 #29862, 1.10 #35543). Same invocations, verified on 1.5.7,
 *   1.6.0, 1.17.0.
 * - Exit statuses: fmt -check 0 formatted, 3 files to format (their paths on stdout); validate
 *   0 valid, 1 errors (`Error: <summary>` / `  on main.tf line 2, ...` on stderr); usage errors 1
 *   with Go's flag message: `flag provided but not defined: -x` (fmt prints it after its usage
 *   text, validate under `Error: Failed to parse command-line flags`), `invalid boolean value
 *   "foo" for -check`.
 */
import type { ToolAdapter } from "./types.js";

const COMMON = {
  "format-check": "fmt -check -recursive",
  // run in the directory to validate (forge_tf_validate: `(cd "$d" && forge_do terraform validate)`;
  // today's `-chdir=<d>` cannot be an {args} word); no init
  validate: "validate -no-color",
};

export const TERRAFORM: ToolAdapter = {
  tool: "terraform",
  runner: "path",
  pick: "forge_tf_pick",
  verified: "1.17.0",
  usage: { exit: 1, pattern: "flag provided but not defined: |invalid ([a-z]+ )?value .+ for (flag )?-" },
  ranges: [
    {
      from: "1.0.0",
      to: "1.3.0",
      source: "developer.hashicorp.com/terraform/language/v1-compatibility-promises (1.x); hashicorp/terraform v1.3/CHANGELOG.md 1.3.0 (fmt accepts multiple target paths, #28191)",
      ops: { ...COMMON, format: null },
    },
    {
      from: "1.3.0",
      source: "hashicorp/terraform v1.3/CHANGELOG.md 1.3.0 ENHANCEMENTS: terraform fmt now accepts multiple target paths (#28191)",
      ops: { ...COMMON, format: "fmt" },
    },
  ],
};
