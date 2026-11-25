<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `shopify`

### shopify

Shopify apps, their extensions and Functions, and Shopify themes.

Detected by: a file matching `shopify.app.toml`, `shopify.app.*.toml`, `**/shopify.extension.toml`, `layout/theme.liquid`, `*/layout/theme.liquid`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `shopify/shopify-app-dev` | Running the Shopify app, its extensions and Functions against a development store with the Shopify CLI, and exercising them. | on-demand | The task changes an app's pages, its webhook handlers or its OAuth flow.<br>The task builds or changes a checkout or theme extension, or a Function such as a discount, shipping or payment customization. | a file matches `shopify.app.toml`, `shopify.app.*.toml` |

### shopify/theme

Shopify themes, their Liquid sections and blocks.

Detected by: a file matching `layout/theme.liquid`, `*/layout/theme.liquid`.
Child of `shopify`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `shopify/theme/shopify-theme-dev` | Previewing the Shopify theme locally with the Shopify CLI and checking its sections and blocks in the theme editor. | on-demand | The task adds or changes a Shopify theme's sections, blocks, snippets or templates.<br>The task debugs how a Shopify theme page renders. | wherever its pack is |
