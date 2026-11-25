<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `cloudformation`

### cloudformation

AWS CloudFormation and SAM templates.

Detected by: a file matching `**/samconfig.toml`; or a line matching `^AWSTemplateFormatVersion:` in a file matching `**/*.yaml`, `**/*.yml`, `**/*.template`; or a line matching `^\s*"AWSTemplateFormatVersion"\s*:` in a file matching `**/*.json`, `**/*.template`; or a line matching `^Transform:\s*["']?AWS::Serverless` in a file matching `**/*.yaml`, `**/*.yml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `cloudformation/pulumi-import` | Migrating a deployed CloudFormation stack to Pulumi by importing its resources and previewing until Pulumi sees no change. | on-demand | The task migrates infrastructure from CloudFormation or SAM to Pulumi. | wherever its pack is |
