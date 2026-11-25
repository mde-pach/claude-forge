<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `terraform`

### terraform

Terraform and OpenTofu modules, formatted, validated and linted.

Detected by: a file matching `**/*.tf`, `**/*.tofu`.

Context (in `.claude/rules/forge-*.md`, §7):

- Terraform / OpenTofu. Refactors use `moved {}` blocks, resources leave state through `removed {}` blocks and existing resources come in through `import {}` blocks, so the plan shows them; `terraform state mv|rm` and the CLI `import` ask first.
- `prevent_destroy = true` and `deletion_protection = true` are never removed, and `force_destroy = true`, `skip_final_snapshot = true` and `deletion_protection = false` are not set; decommissioning a protected resource is the user's decision: ask them.
- Inline scanner suppressions (`#trivy:ignore:`, `#tfsec:ignore:`, `#checkov:skip=`, `# tflint-ignore:`) may not increase.
- State (`*.tfstate`, `*.tfstate.backup`), `.terraform/` and `crash.log` are never committed; `.terraform.lock.hcl` is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `terraform/ephemeral-secret` | A secret read through data "aws_secretsmanager_secret_version" is stored in clear text in the state. Where the argument that consumes it has a write-only form (password_wo, secret_string_wo; Terraform 1.11+, OpenTofu 1.11+), read it with ephemeral "aws_secretsmanager_secret_version" instead, so it never reaches the state. | any: `**/*.tf`, `**/*.tofu` | grep advise `^[[:space:]]*data[[:space:]]+"aws_secretsmanager_secret_version"`<br>advice, never blocks |
| `terraform/fmt` | Terraform and OpenTofu files are formatted (terraform fmt; tofu fmt in an OpenTofu project). | any: `**/*.tf`, `**/*.tfvars`, `**/*.tofu` | check format: `forge_do terraform format-check` |
| `terraform/iam-no-wildcard-resource` | An IAM policy statement names the resources it applies to (ARNs, or ARN patterns scoped to the project) rather than Resource = "*", except for actions that support no resource-level permissions (such as ec2:Describe*) and KMS key policies, where "*" means the key itself. | any: `**/*.tf`, `**/*.tofu`, `**/*.tf.json`, `**/*policy*.json` | grep advise `["]?[Rr]esources?["]?[[:space:]]*[:=][[:space:]]*\[?[[:space:]]*"\*"[[:space:]]*\]?[[:space:]]*,?[[:space:]]*(#.*)?$`<br>advice, never blocks |
| `terraform/misconfig` | A change introduces no new HIGH or CRITICAL Terraform misconfiguration found by trivy (public buckets, ingress open to the world, unencrypted storage); a finding is fixed, not added to .trivyignore. | any: `**/*.tf`, `**/*.tofu` | check lint: `forge_trivy_config` |
| `terraform/no-cancel-in-progress-apply` | A workflow job that runs terraform (or tofu) apply or destroy does not set concurrency cancel-in-progress: true: a newer run cancels the apply midway, leaving the state locked and resources half changed. Keep the concurrency group and set cancel-in-progress: false, so runs queue. | yaml: `.github/workflows/*.yml`, `.github/workflows/*.yaml` | ast-grep forbid |
| `terraform/no-dynamodb-lock` | The s3 backend locks the state with use_lockfile = true (Terraform 1.11+, OpenTofu 1.10+), not a dynamodb_table, which Terraform deprecated and will remove; drop the table once every user of the state runs a version that has use_lockfile. | any: `**/*.tf`, `**/*.tofu`, `**/*.tfbackend`, `**/backend*.hcl` | grep advise `^[[:space:]]*dynamodb_table[[:space:]]*=`<br>advice, never blocks |
| `terraform/no-literal-state-passphrase` | An OpenTofu state encryption key provider never holds its passphrase as a literal: anyone who reads the code can then decrypt the state. Pass it in a variable (passphrase = var.state_passphrase, set through TF_VAR_state_passphrase or TF_ENCRYPTION) or use a KMS key provider. | any: `**/*.tf`, `**/*.tofu` | grep forbid `^[[:space:]]*passphrase[[:space:]]*=[[:space:]]*"([^"$]\|\$[^{])` |
| `terraform/no-new-suppressions` | No new inline IaC scanner suppressions (#trivy:ignore:, #tfsec:ignore:, #checkov:skip=, # tflint-ignore:); a changed file may not have more than it had. Fix the finding instead. | any: `**/*.tf`, `**/*.tofu` | grep no-increase `(#\|//)[[:space:]]*(trivy:ignore:\|tfsec:ignore:\|checkov:skip=\|tflint-ignore:)` |
| `terraform/no-unprotect` | No force_destroy = true, skip_final_snapshot = true or deletion_protection = false: they clear the way for destroying data. An intended decommission is the user's decision: ask them. | any: `**/*.tf`, `**/*.tofu` | grep forbid `(force_destroy\|skip_final_snapshot)[[:space:]]*=[[:space:]]*true\|deletion_protection[[:space:]]*=[[:space:]]*false` |
| `terraform/protection-ratchet` | Deletion protections are not removed: a changed file may not have fewer prevent_destroy = true or deletion_protection = true than it had. An intended decommission is the user's decision: ask them. | any: `**/*.tf`, `**/*.tofu` | grep no-decrease `(prevent_destroy\|deletion_protection)[[:space:]]*=[[:space:]]*true` |
| `terraform/state-not-committed` | No Terraform state is committed or added (*.tfstate, *.tfstate.backup, .terraform/, crash.log): state holds every secret Terraform manages; keep it in a remote backend. A .terraform.lock.hcl next to the configuration is committed, not git-ignored. | any | check lint: a shell script of 6 lines (when `**/*.tf`, `**/*.tofu`, `**/*.tfstate`, `**/*.tfstate.backup`, `**/.terraform/**`, `**/crash.log`, `**/.terraform.lock.hcl` or `**/.gitignore` changes) |
| `terraform/tflint` | Changed modules pass tflint's terraform ruleset (recommended preset: required_version, required_providers, pinned module sources and versions, typed variables, no unused declarations, no deprecated syntax), plus the aws, google or azurerm ruleset when the lock file lists that provider (invalid instance types, engines, regions); rules are not disabled in .tflint.hcl to silence a finding. | any: `**/*.tf`, `**/*.tofu` | check lint: `forge_tflint` |
| `terraform/validate` | Each changed Terraform or OpenTofu module passes validate (arguments and attributes checked against the provider schemas); a module validate cannot judge before init (providers or modules not installed) is reported skipped. | any: `**/*.tf`, `**/*.tofu` | check typecheck: `forge_tf_validate` |

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `terraform/pulumi-migration` | Moving a Terraform or OpenTofu project and its state to Pulumi, until the Pulumi preview shows no change. | on-demand | The task is to migrate Terraform or OpenTofu infrastructure to Pulumi. | wherever its pack is |
