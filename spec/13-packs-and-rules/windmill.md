<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `windmill`

### windmill

Windmill scripts, flows and apps kept in a repository synced with the wmill CLI.

Detected by: a file matching `wmill.yaml`, `*/wmill.yaml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `windmill/wmill-preview` | Running a Windmill script or flow from the local files with the wmill CLI, and seeing what a sync would deploy, without deploying. | on-demand | The task writes or changes a Windmill script or flow.<br>The task debugs a Windmill script or flow that fails or returns the wrong result. | wherever its pack is |
