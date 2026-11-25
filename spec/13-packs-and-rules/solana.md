<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `solana`

### solana

Solana programs (Anchor, native, Pinocchio) and their clients.

Detected by: a dependency on one of `@solana/web3.js` (npm), `@solana/kit` (npm), `@coral-xyz/anchor` (npm); or a file matching `Anchor.toml`, `**/Anchor.toml`; or a line matching `^[ \t]*(anchor-lang\|solana-program\|pinocchio)[ \t]*=` in a file matching `Cargo.toml`, `**/Cargo.toml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `solana/solana-mainnet-fork` | Running the program against a local Surfpool network that forks mainnet, sending it transactions and reading their logs. | on-demand | The task reproduces or checks how a Solana program behaves against real mainnet accounts or programs.<br>The task debugs a Solana transaction that fails or behaves unexpectedly. | wherever its pack is |
