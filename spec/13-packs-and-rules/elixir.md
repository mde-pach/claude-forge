<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `elixir`

### elixir

Elixir projects built with Mix.

Detected by: a file matching `**/mix.exs`.

No rules of its own.

### elixir/phoenix

Phoenix web applications (controllers, LiveView, Ecto).

Detected by: a line matching `\{:phoenix(_live_view)?,` in a file matching `**/mix.exs`.
Child of `elixir`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `elixir/phoenix/ecto-query-count` | Counting the Ecto queries a Phoenix page, LiveView or controller action really runs, to find N+1 loading. | on-demand | The task is to find why a page or endpoint is slow, or which database queries it runs.<br>The task is to find or fix N+1 database queries. | wherever its pack is |
| `elixir/phoenix/runtime-eval` | Evaluating code, reading logs and querying the database inside the running Phoenix app, through Tidewave or Mix. | on-demand | The task is to find out how a running Phoenix application behaves, what state it holds or what its database contains.<br>The task is to find the cause of a runtime error in a Phoenix application. | wherever its pack is |
