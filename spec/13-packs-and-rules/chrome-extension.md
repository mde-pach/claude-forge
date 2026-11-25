<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `chrome-extension`

### chrome-extension

Chrome and Chromium browser extensions (Manifest V3), plain or built with CRXJS, WXT or Plasmo.

Detected by: a dependency on one of `@crxjs/vite-plugin` (npm), `wxt` (npm), `plasmo` (npm); or a line matching `^\s*"manifest_version"\s*:\s*3\b` in a file matching `manifest.json`, `src/manifest.json`, `public/manifest.json`, `extension/manifest.json`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `chrome-extension/load-unpacked` | Loading the built extension into Chromium with Playwright to exercise its popup, content scripts and service worker. | on-demand | The task changes what a browser extension's popup, options page, content script or service worker does.<br>The task is to reproduce or check a browser extension's behaviour in the browser. | wherever its pack is |
