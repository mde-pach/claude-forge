<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `php`

### php

PHP projects.

Detected by: a file matching `composer.json`; or a source file of the project's own matching `**/*.php`.

No rules of its own.

### php/laravel

Laravel applications, run through artisan.

Detected by: a line matching `"laravel/framework"\s*:` in a file matching `composer.json`.
Child of `php`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `php/laravel/laravel-db-schema` | Reading the Laravel app's real database tables, columns and indexes with artisan before writing a migration or a query. | on-demand | The task adds or changes a Laravel migration.<br>The task writes or debugs a database query, or needs to know a table's columns or indexes. | wherever its pack is |
| `php/laravel/laravel-logs-errors` | Finding the error behind a failing Laravel request in the application's logs. | on-demand | The task debugs a Laravel request, job or command that fails or returns an error. | wherever its pack is |
| `php/laravel/laravel-tinker` | Running PHP code inside the Laravel application with tinker to check how its models, queries and configuration behave. | on-demand | The task needs to find out how the app behaves at run time, such as what its models, relations or queries return or which configuration values it uses. | wherever its pack is |

### php/magento

Magento 2 and Adobe Commerce stores.

Detected by: a file matching `bin/magento`, `app/etc/config.php`; or a line matching `"magento/(product-community-edition\|product-enterprise-edition\|magento2-base\|framework)"\s*:` in a file matching `composer.json`.
Child of `php`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `php/magento/magento-index-status` | Checking Magento's indexers and caches with bin/magento, and rebuilding only what is stale. | on-demand | The task debugs a Magento store that shows stale or wrong data, such as prices, stock, categories or search results.<br>The task investigates why Magento pages are slow. | wherever its pack is |

### php/mcp

Model Context Protocol servers written with the PHP SDK.

Detected by: a line matching `"(mcp/sdk\|php-mcp/server\|logiscape/mcp-sdk-php)"\s*:` in a file matching `composer.json`.
Child of `php`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `php/mcp/mcp-inspector-probe` | Calling the PHP MCP server's tools, resources and prompts through the MCP Inspector's command line and reading its answers. | on-demand | The task adds or changes a tool, resource or prompt of the project's MCP server.<br>The task needs to check what the MCP server answers to a client. | wherever its pack is |

### php/wordpress

WordPress sites, themes and plugins, WooCommerce among them.

Detected by: a file matching `wp-config.php`, `wp-config-sample.php`, `wp-cli.yml`; or a line matching `"(johnpbloch/wordpress\|roots/wordpress\|wpackagist-(plugin\|theme)/[^"]+)"\s*:` in a file matching `composer.json`; or a line matching `^\s*\*?\s*Plugin Name:` in a file matching `*.php`, `*/*.php`; or a line matching `^\s*\*?\s*Theme Name:` in a file matching `style.css`, `*/style.css`.
Child of `php`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `php/wordpress/wp-cli-diagnose` | Finding why a WordPress or WooCommerce site is slow with WP-CLI and Query Monitor, before changing it. | on-demand | The task investigates or improves the speed of a WordPress or WooCommerce site. | wherever its pack is |
