# Findings about Forge, from the experiment's transcripts

These are behaviours of Forge at the pinned commit (091c0d3). They are reported, not fixed: the experiment measures Forge as shipped.

## F1. The type check blocks on errors in unchanged files when tsc prints colour (zod)
- **Documented behaviour.** "A whole-project type check blocks only for errors in files that are part of the change. Errors already present elsewhere are reported as remaining and do not block" (lifecycle: When Claude stops).
- **Observed.** In zod, `typescript/typecheck` blocked the Stop 3 times, then handed over, in 10 Forge trials across 8 tasks of Run A. The errors it showed were in files the agent never touched (`packages/bench/*.ts`, `packages/integration/fixtures/...`). zod's parent commit already has 31 tsc errors.
- **Cause.** Forge runs `tsc --noEmit -p tsconfig.json`, then attributes diagnostics with `forge_diagnostics` (`templates/scripts/engine.sh`), which expects lines that start with `path(l,c)` or `path:l:c`.
  - zod's `.configs/tsconfig.base.json` sets `"pretty": true`, so tsc 5.5 colours its output even when it goes to a file: `\e[96mpackages/bench/key-iteration.ts\e[0m:\e[93m26\e[0m:...`.
  - No line matches, so the function returns `none`, and `forge_run_check` falls through to `failed`, printing the whole output.
  - The same path also fires after each edit (PostToolUse type-check feedback).
- **Effect.** Every zod Forge trial gets up to 3 Stop blocks that the agent cannot clear without fixing unrelated code. This costs turns, time and money, and risks unrelated edits.
- **Possible fix (for Forge, not applied).** Strip ANSI escape sequences before attribution, or run tsc with `--pretty false`.

## F2. Forge's sandbox probe does not detect that Claude Code's sandbox cannot start in Docker
- `apply --user` / `status --user` probe `bwrap --ro-bind / / --dev /dev --unshare-all --die-with-parent true`. That probe succeeds in a container that blocks mounting `/proc`, but Claude Code's sandbox mounts `/proc` and fails there with `bwrap: Can't mount proc on /newroot/proc: Operation not permitted`.
- **Observed** in all 81 Forge trials of Run A (DECISIONS D17). Every Bash command failed in the sandbox first and was retried with `dangerouslyDisableSandbox`.
- **Possible fix.** Add `--proc /proc` to the probe, and mention `--security-opt systempaths=unconfined` in the message.

## F3. The Stop test run includes tests that need services the environment lacks (better-auth)
See DECISIONS D16. This is expected behaviour (tests must pass), but in an environment without the project's databases Forge blocks for failures unrelated to the change.
