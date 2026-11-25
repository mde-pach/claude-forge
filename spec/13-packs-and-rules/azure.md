<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `azure`

### azure

Azure infrastructure and applications (Bicep and ARM templates, the Azure Developer CLI, Static Web Apps).

Detected by: a file matching `**/*.bicep`, `**/*.bicepparam`, `azure.yaml`, `**/staticwebapp.config.json`, `**/swa-cli.config.json`; or a line matching `"\$schema"\s*:\s*"https?://schema\.management\.azure\.com/schemas/[^"]*[dD]eploymentTemplate\.json` in a file matching `**/*.json`.

No rules of its own.

### azure/bicep

Azure infrastructure as Bicep or ARM templates, deployed with the Azure CLI or azd.

Detected by: a file matching `**/*.bicep`, `**/*.bicepparam`; or a line matching `"\$schema"\s*:\s*"https?://schema\.management\.azure\.com/schemas/[^"]*[dD]eploymentTemplate\.json` in a file matching `**/*.json`.
Child of `azure`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `azure/bicep/pulumi-import` | Migrating deployed Azure resources from ARM or Bicep to Pulumi by importing them and previewing until Pulumi sees no change. | on-demand | The task migrates infrastructure from ARM templates or Bicep to Pulumi. | wherever its pack is |
| `azure/bicep/what-if` | Previewing what a Bicep or ARM deployment would change in Azure with what-if, before deploying it. | on-demand | The task changes infrastructure templates that will be deployed to the cloud.<br>The task needs the changes a deployment would make previewed or validated before it runs.<br>The task asks which cloud resources an infrastructure change would create, modify, replace or delete. | wherever its pack is |

### azure/static-web-apps

Azure Static Web Apps, run locally with the SWA CLI.

Detected by: a dependency on `@azure/static-web-apps-cli` (npm); or a file matching `**/staticwebapp.config.json`, `**/swa-cli.config.json`.
Child of `azure`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `azure/static-web-apps/swa-local` | Running an Azure Static Web App locally with the SWA CLI emulator, frontend, API, routes and auth together. | on-demand | The task changes routing, navigation fallback, headers or authentication of a Static Web App.<br>The task changes the API functions of a Static Web App or how the frontend calls them. | wherever its pack is |
