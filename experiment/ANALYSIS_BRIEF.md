# Brief for an agentic analysis of the Forge experiment runs

You are analysing the raw results of a controlled experiment. It asks whether **Forge**, a generator of Claude Code guardrails (hooks, permissions, sandbox, `check.sh`), improves Claude Code's outcomes on real software tasks. Your job is to **find problems**: harness bugs, unfair differences between arms, scoring errors, Forge misbehaviour, and anything that threatens the validity of the results. Report each problem with evidence (paths, line excerpts, counts). Do not judge trial outcomes by opinion. Scoring is mechanical, so check whether the mechanics are right.

Read these first, in order:
1. `DESIGN.md`: the pre-registered design.
2. `DECISIONS.md`: every judgement call, D1–D17 and later.
3. `CHANGELOG.md`: every harness change, P1–P13 and M1–M2.
4. `FINDINGS.md`: Forge behaviours already found, F1–F3.

## Layout of this bundle
```
experiment/
  DESIGN.md DECISIONS.md CHANGELOG.md FINDINGS.md   design and history
  config/pins.json               pinned versions (Claude Code 2.1.310, model claude-sonnet-5-5, Forge 091c0d3)
  mining/                        repository survey and candidate selection (select.py)
  dataset/
    manifest.v1.json             FROZEN dataset: 60 tasks with sha256 per task directory
    tasks/<task_id>/             task.json, instruction.md (exactly what the agent saw),
                                 hidden/ (audited hidden test files), audit.json, reference.patch
    tasks_pilot/                 pilot-window tasks (not in the main dataset)
    work/<task_id>/              dataset-building stages: state.json (F2P/P2P, audit verdicts, rejection reason)
    prompts/                     fixed prompts for the instruction writer and the test auditor
    candidates/                  every candidate commit per repo/window, with drop reasons
  harness/                       run_trial.py (one trial), orchestrate.py, repo_env.py (containers),
                                 transcript.py (parser), classify.py (infra classifier), fingerprint.py (parity),
                                 repos/*.yaml (install/test/lint per repo), images/ (Dockerfiles)
  analysis/                      analyze.py (pre-registered statistics), status.py, make_synthetic.py
  results/raw/
    main/      RUN A: the Forge sandbox could NOT start (D17). Secondary; about 170 trials; stopped
    main_b/    RUN B: the PRIMARY run, sandbox fixed. IN PROGRESS when this bundle was made
    pilot1..4/ pilot runs on pilot-window tasks (harness checks only)
```

## One trial: `results/raw/<run>/<task_id>/<arm>/r<rep>/`
| file | content |
|---|---|
| `meta.json` | `status` (`scored` / `infra_error` / `parity_error`), `infra_reason`, `cost_usd` (agent + Forge reviewer), `num_turns`, `duration_s` (agent), `started` / `finished` (whole trial), `timed_out`, `sandbox_failures` and `sandbox_bypass_calls` (Run B only) |
| `outcome.json` | `resolved`, `f2p` / `p2p` (test id → passed/failed/error/missing), `p2p_regressions`, `lint_new`, `diff_lines`, `changed_files`, `all_results` (every test the scorer saw) |
| `transcript.jsonl` | the full Claude Code `stream-json` with `--include-hook-events`: assistant messages, tool calls and results, `system/hook_started` and `system/hook_response` (with hook name, exit code and output), `result` events (a session can have several; `total_cost_usd` is cumulative) |
| `final.diff` | the agent's change against the starting commit, excluding `.forge/` and `.claude/` |
| `edits.json` | edit-tool calls versus shell edit commands (heuristic regex in `harness/transcript.py`) |
| `forge/hook_summary.json` | hook counts, blocks, Stop blocks and feedback, plus samples (parsed from the transcript) |
| `forge/forge_apply.log` | output of `forge apply --user --yes`, `forge apply --yes` and `forge status` (Forge arm only) |
| `forge/forge_state/` | Forge's own state directory, copied after the run |
| `parity.json` | the pre-agent environment fingerprint digest and comparison. In the Forge arm, also what `forge apply` changed |
| `fingerprint_pre.json` | the full pre-agent fingerprint of `/work` and `/home/dev` |
| `claude_calls.jsonl` | every `claude` invocation in the container: the agent itself, and Forge's reviewer if it ran |
| `debug.log` | Claude Code's `--debug-file` |
| `install.log`, `score_apply.log`, `score_tests.log`, `collect_diff.log` | preparation and scoring logs |
| `_logs/` | present when an attempt failed: the logs of that attempt |

Run-level files:
- `_attempts.json`: attempts per trial.
- `_failed_attempts/*.json`: the meta of every failed attempt.
- `_parity/<task>.json`: the reference fingerprint and which trial set it.
- `_log.jsonl`: one line per attempt.

## How the trial works (so you can check it)
1. Start a container on an internal network. Its only egress is an allow-list proxy to `api.anthropic.com`.
2. Write the parent commit's tree as a fresh single-commit git repository, then install offline from caches.
3. Mark the workspace trusted, in both arms.
4. In the Forge arm only: `forge apply --user --yes`, `forge apply --yes`, and commit `.forge` and `.claude`.
5. Parity: the pre-agent fingerprint must equal the task's reference. In the Forge arm, everything `forge apply` changed must be Forge-owned.
6. Run `claude -p --model claude-sonnet-5-5 --permission-mode bypassPermissions --output-format stream-json --verbose --include-hook-events`, with the instruction on stdin. The limits are 45 minutes and $15.
7. Collect the diff, then score in a **fresh** container: parent, install, apply the diff, copy the hidden tests over, and run the task's test targets. A trial is resolved when every kept F2P test and every kept P2P test passes.

## Known issues to verify or quantify (do not stop at these)
- **F1:** in zod, Forge's type check blocks on pre-existing errors because tsc output is coloured (`"pretty": true`). Count the affected trials and their cost and turns.
- **F2 / D17:** in Run A every Forge trial hit `bwrap: Can't mount proc`. Run B should show `sandbox_failures == 0`. Verify this.
- **F3 / D16:** in better-auth, Forge's Stop test run includes database tests that fail offline.
- **Parity errors:** a few attempts failed parity on `node_modules/**` (zod, inngest-js) or better-auth `dist/`. Every one was retried. Check whether retries ever exhausted, and whether failures cluster by arm.

## Questions worth answering
1. Is anything in a baseline transcript Forge-related? There must be none. Is anything in a Forge transcript a sign that Forge was not active?
2. Do any scored failures look like **scoring** failures rather than agent failures? Examples: `missing` test ids, collection errors caused by the environment, `patch_applied: false`, type checks failing at the parent.
3. Do the agents ever see or fetch the upstream fix (network attempts, git history)? Look at the netproxy-denied hosts in the transcripts, and at `git log` usage.
4. Did Forge's hooks ever block for reasons unrelated to the agent's change, beyond F1 and F3? Classify every Stop block (rule id, whether the named files were changed).
5. Did Forge's PreToolUse guard deny or ask on commands an agent legitimately needed (for example `safety/ephemeral-exec`)?
6. Cost, turns and time per arm and repository. Which Forge rules account for the extra turns?
7. Tasks at ceiling (both arms 100%) or floor (both 0%). Do the instructions over-specify the solution? The audit kept 674 of 676 F2P tests, so check whether the instructions leak the implementation.
8. Any trial where the agent edited tests, and Forge's `no-test-edits` rule interacted with it.

## Reproducing the statistics
`python3 analysis/analyze.py results/raw/main_b dataset --out /tmp/out`, standard library only. This writes `summary.json` and `tables.md`. The primary analysis uses only complete replicates.

## Notes
- Run B was in progress when this bundle was made, so its counts are partial.
- A pre-agent parity failure never reaches the agent. Infra and parity errors are retried up to 3 times and are never scored.
