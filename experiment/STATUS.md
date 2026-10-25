# Status: experiment paused (2026-10-25)

The user paused the experiment, to update Forge and the experiment harness first. Nothing is running.

## What exists
| Run | Directory | State |
|---|---|---|
| Pilots 1–4 | `results/raw/pilot1..4` | complete (harness checks only) |
| Run A | `results/raw/main` | about 170 of 360 trials, stopped. Invalid as the primary result: Forge's sandbox could not start (D17) |
| Run B (primary) | `results/raw/main_b` | 7 trials, stopped |
| Forge-fixed arm | `results/raw/main_b_fixed` | not started (D18) |

The frozen dataset `dataset/manifest.v1.json` (60 tasks) remains valid. A rerun with a changed harness or Forge should keep using it, or freeze a v2 if the task rules change.

## Known problems to address before a rerun
**Forge** (FINDINGS.md):
- **F1:** coloured diagnostics broke the attribution of type errors to changed files. Fixed on this branch in `d3008b6`, not merged to main.
- **F2:** the sandbox probe misses `/proc` mount failures.
- **F3:** the Stop test run blocks on tests that need services the environment lacks.
- **Tests that use stand-ins** instead of real binaries: alembic, pytest, bwrap (see the conversation's audit).

**Harness** (CHANGELOG.md P1–P13 and M1–M3; DECISIONS.md D16–D18):
- intermittent parity mismatches in `node_modules` trees and better-auth `dist/` (install non-determinism);
- better-auth needs about 15–25 minutes of setup per trial, and its database-backed tests cannot run;
- the container needs `systempaths=unconfined` for the sandbox (now set);
- the orchestrator's scheduling, fixed in M1.
