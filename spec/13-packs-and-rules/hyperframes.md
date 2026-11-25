<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `hyperframes`

### hyperframes

HyperFrames video projects, compositions written in HTML and rendered to video.

Detected by: a dependency on one of `hyperframes` (npm), `@hyperframes/cli` (npm); or a file matching `hyperframes.json`, `*/hyperframes.json`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `hyperframes/hyperframes-preview-render` | Seeing what a HyperFrames composition shows, with the CLI's checks, frame snapshots, preview server and draft render. | on-demand | The task makes or edits a HyperFrames video composition, a scene or its animation.<br>The task needs to see what a HyperFrames video looks like at given times, or to render it. | wherever its pack is |
