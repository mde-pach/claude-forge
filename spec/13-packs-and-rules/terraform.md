### terraform

Detected by `**/*.tf` or `**/*.tofu`. The grep rules apply to those files; checks run the project's Terraform CLI, a system tool (§13.5): `tofu` in an OpenTofu project, else `terraform`.

| Rule | Targets |
|---|---|
| `terraform/protection-ratchet` | `grep` no-decrease: `prevent_destroy = true`, `deletion_protection = true` |
| `terraform/no-unprotect` | `grep` forbid: `force_destroy = true`, `skip_final_snapshot = true`, `deletion_protection = false` |
| `terraform/no-new-suppressions` | `grep` no-increase: `#trivy:ignore:`, `#tfsec:ignore:`, `#checkov:skip=`, `# tflint-ignore:` (also after `//`) |
| `terraform/state-not-committed` | `check` lint: `git ls-files -co --exclude-standard` lists no `*.tfstate`, `*.tfstate.backup`, `crash.log` or file under `.terraform/`; a `.terraform.lock.hcl` next to a tracked `*.tf` / `*.tofu` is not git-ignored |
| `terraform/fmt` | `check` format: `terraform fmt -check -recursive`; changed files: `terraform fmt {files}` (on `*.tf`, `*.tfvars`, `*.tofu`) |
| `terraform/validate` | `check` typecheck: `forge_tf_validate`: `validate -no-color` in each directory of a changed `.tf` / `.tofu` file; errors as `path:line: Error: summary`; a directory validate cannot judge before `init` (its output asks for `terraform init`: providers or modules not installed) is `skipped` |
| `terraform/tflint` | `check` lint: `forge_tflint`: pinned tflint (§13.5) `--chdir <dir> --format compact` per changed module, with a config of Forge's own (`plugin "terraform"` with `preset = "recommended"`, plus the pinned `aws` 0.49.0 / `google` 0.40.0 / `azurerm` 0.32.0 ruleset when a `.terraform.lock.hcl` names that `hashicorp` provider, itself a pinned release binary (§13.5, sha256-checked; not `tflint --init`, which calls GitHub's rate-limited API), linked into a plugin directory of the run's own; offline: `skipped`; tflint runs with `TMPDIR=/tmp`, its plugins' sockets needing a short path); every issue counts, whatever tflint's severity |
| `terraform/misconfig` | `check` lint: `forge_trivy_config`: pinned trivy (§13.5) `config --misconfig-scanners terraform --severity HIGH,CRITICAL --skip-check-update --ignorefile <empty>` on each changed module and on its base copy (the directory's files with the change undone); a (check, resource) pair new in the change is a finding at line 0. Not with `--all` |

context.md: `moved {}`, `removed {}` and `import {}` blocks instead of `terraform state mv|rm` and CLI import; protections are kept; suppressions and state as above.
