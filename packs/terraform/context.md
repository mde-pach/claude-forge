Terraform / OpenTofu. Refactors use `moved {}` blocks, resources leave state through `removed {}` blocks and existing resources come in through `import {}` blocks, so the plan shows them; `terraform state mv|rm` and the CLI `import` ask first.
`prevent_destroy = true` and `deletion_protection = true` are never removed, and `force_destroy = true`, `skip_final_snapshot = true` and `deletion_protection = false` are rejected; an intended decommission is accepted in `forge.yaml`.
Inline scanner suppressions (`#trivy:ignore:`, `#tfsec:ignore:`, `#checkov:skip=`, `# tflint-ignore:`) may not increase.
State (`*.tfstate`, `*.tfstate.backup`), `.terraform/` and `crash.log` are never committed; `.terraform.lock.hcl` is.
