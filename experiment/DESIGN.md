# Does Forge improve Claude Code on real software-engineering tasks? Experiment design

Status: **pre-registered**. I wrote this before building the dataset or running any trial. Changes made after this point are listed, with dates and reasons, in `CHANGELOG.md`. A change to a rule, the task set or the scoring creates a new dataset version (`dataset/manifest.vN.json`); it never edits a frozen one.

## 1. Question

Does applying Forge (https://github.com/mde-pach/claude-forge, pinned commit) as its documentation describes change the share of real upstream tasks that Claude Code completes correctly? The comparison is against the same Claude Code with nothing added.

- H0: the per-task success rate is the same in both arms.
- H1, two-sided: the rates differ.

## 2. Arms

| Arm | What runs |
|---|---|
| `baseline` | Claude Code as shipped: pinned version and model, a clean `$HOME`, the workspace marked trusted, no user or project settings. |
| `forge` | The same container, then `forge apply --user --yes` (user level, once per "machine" = per trial `$HOME`) and `forge apply --yes` in the repository, from Forge at the pinned commit, as in *Getting started*. Nothing in Forge is modified. No `forge.yaml`. |

There are no other arms. Each extra arm would cost as much as the main comparison. The one candidate I considered (an `acceptEdits` permission-mode variant) is discussed in DECISIONS.md D6.

Everything else is identical and checked mechanically before each trial (§7): the image digest, the Claude Code binary hash, the CLI arguments, the model, the effort, the environment variables, the workspace content apart from Forge-owned paths, the `$HOME` content apart from Forge-owned paths, the network policy, and the CPU, memory and time limits.

Invocation (both arms):
`claude -p "<instruction>" --model <pinned> --permission-mode bypassPermissions --output-format stream-json --verbose --include-hook-events --debug-file <log>`.
The run is non-root inside the container, with a wall-clock limit of 45 min per trial and `--max-budget-usd 15`.

## 3. Repositories

These are chosen by a fixed rule before any trial:
- the stack is covered by a Forge pack (Python: `pyproject.toml`; TypeScript: `tsconfig.json` + `typescript`);
- the repository uses that pack's own lint tool (ruff for Python, Biome for TypeScript), so that Forge has something to enforce;
- the project is active, with at least 10 candidate commits (§4) in the task window;
- the test suite installs and runs offline in a container from a lockfile.

The survey output (`mining/survey.json`) and the final list with reasons are in DECISIONS.md D3.

## 4. Tasks

Each task comes from one upstream commit C on the default branch's first-parent history:
- **Starting state:** the tree of `C^`, as a fresh single-commit git repository. No history and no remote, so the agent cannot see the future.
- **Hidden tests:** the test files of `C` after the audit (§5). The agent never sees them.
- **Instruction:** a description of what the change must achieve. A fresh agent context writes it from the commit message, the linked PR or issue text (when the commit message references one) and the diff. It states the observable behaviour and names only the public interface the tests need. It never describes the implementation.

**Selection procedure** (deterministic, `mining/select.py`, fixed now):
1. Main window: committer date in [2026-07-01, 2026-10-15). Pilot window: [2026-05-01, 2026-07-01), disjoint from the main pool. The windows start after the pinned model's training data, as far as public information allows (see Threats).
2. Keep non-merge commits that change at least one test file and at least one non-test source file. Non-test source lines changed (added + deleted) must be between 3 and 300, in at most 8 source files. Drop commits that change the dependency manifest's dependencies or the lockfile, and reverts.
3. Order the commits oldest first.
4. Execution validation (in the repository's image, offline): with the commit's test files applied, run them on `C^` and on `C`, three times each. F2P = tests that fail or error on `C^` in 3/3 runs and pass on `C` in 3/3 runs. P2P = tests in the same files that pass in 3/3 runs on both. Drop the commit if F2P is empty, or if any run is non-deterministic for a test it relies on.
5. Instruction writing, then the test audit (§5), then re-validation.
6. Take the first **10 accepted tasks per repository**, in order. A repository that yields fewer contributes what it has.

## 5. Test audit (fixed rule)

For each task, a fresh, isolated agent context (`claude -p`, no tools other than reading the repository, no knowledge of the arms or any result) receives the instruction, the commit's test changes, the reference fix, the repository at `C` and this rule:
- keep each test at its level (a unit test stays a unit test);
- assert only what is observable at that level through the interface named in the instruction;
- allow test doubles only at the unit's boundary;
- drop any test that cannot be checked without inspecting the implementation (private names, internal state, exact wording the instruction does not specify, call counts on internals);
- when in doubt, drop.

The auditor may only **drop tests** or **delete or relax assertions**. It may never add assertions. It returns the audited test files plus a per-test verdict with a reason. Then, mechanically:
- drop the task if no F2P test survives;
- re-validate: the reference fix passes all kept tests (3 runs), and the starting state fails every kept F2P test (3 runs);
- otherwise drop the task, with the reason recorded.

## 6. Scoring (mechanical)

After the agent stops, the harness copies the audited hidden test files over the workspace and runs the task's kept F2P and P2P test IDs with the project's runner, writing JUnit output.
- **Primary outcome:** `resolved` = every kept F2P test passes and every kept P2P test passes.
- **Secondary, same for every arm:**
  - F2P pass fraction;
  - P2P regressions;
  - cost (USD, including Forge's reviewer `claude -p` calls);
  - turns;
  - wall time;
  - lint errors introduced: the project's own linter on the files the agent changed, on the final tree versus the start, outside Forge (ruff `check` with the repo config, or `biome lint`);
  - diff size;
  - edit-tool versus shell edits;
  - Forge hook activity (blocks, feedback, reviews).

No agent judges outcomes.

**Infrastructure failures** are not results. These are network, registry, DNS, disk, API or usage-limit errors, container failures, and parity-check failures. They are detected by a fixed classifier on the run's exit status, the stream's `result` event and the logs. Such a trial is marked `infra_error` and retried (up to 3 times), then reported separately. It is never scored as a failure and never used to drop a task.

## 7. Parity check (before every trial)

The harness computes a fingerprint inside the prepared container:
- image ID;
- sha256 of the Claude Code binary and `claude --version`;
- the argv template;
- sorted environment variables;
- sha256 of every workspace file (excluding `.git/objects`);
- sha256 of every file under `$HOME`.

It removes the paths Forge is documented to own: those listed in `.forge/forge.lock.json` and `.forge/manifest.sha256`, `.claude/`, `.forge/`, the git pre-commit hook, `~/.claude/settings.json`, `~/.claude/skills/forge-user/`, and Forge's state and log files. The remaining fingerprint must equal the task's reference fingerprint, which is computed by a baseline preparation. In the `forge` arm, the Forge-owned set must also be non-empty, and the `forge` lock must name the pinned Forge commit and version. In the baseline arm, it must be empty. Any mismatch fails the trial as `parity_error` before the agent starts.

## 8. Sample size and power

The target is about 60 tasks (6 repositories × 10) × 2 arms × **3 trials** = 360 trials. The unit of analysis is the task: the per-task success rate is the mean over its 3 trials, and the difference per task is forge − baseline.
- **Primary test:** a two-sided paired permutation test (sign-flip) on the per-task differences, with a 95% bootstrap CI (tasks resampled, 10,000 draws) for the mean difference.
- **Supplementary:** a mixed-effects logistic model (arm fixed, task random) and McNemar on trial pairs.

With 60 tasks and a per-task SD of the difference of about 0.35 (typical for stochastic agents with 3 trials and ceiling effects), the SE ≈ 0.045. The design detects an absolute difference of about **13 percentage points** with 80% power at α = 0.05. It cannot reliably detect differences under about 8–10 pp, and a non-significant result is not evidence of equivalence within that band. I also report a TOST equivalence check at ±10 pp.

**Run order and stopping:** trials run in complete replicates (replicate k = every task × both arms). The order within a replicate is randomised with a fixed seed, and the arms of a task are interleaved. If resources run out, analysis uses only completed replicates, so truncation cannot favour an arm.

## 9. Observability (per trial)

All of this goes in `results/raw/<run>/<task>/<arm>/<rep>/`:
- `outcome.json` (resolved, per-test status, junit);
- `meta.json` (cost, turns, duration, exit, infra classification, parity fingerprint);
- `final.diff`;
- `transcript.jsonl` (stream-json with hook events);
- `debug.log`;
- `edits.json` (edit tools vs shell edits);
- `forge/` (the Forge state directory: reviews log, Stop-check outputs, plus hook events extracted from the transcript);
- `claude_calls.jsonl`: every `claude` invocation in the container, with cost, which catches Forge's reviewer.

## 10. Analysis plan

- Per-arm resolved rate with a 95% CI (cluster bootstrap over tasks).
- Paired difference with a CI and a permutation p-value.
- A breakdown by repository and by task type. Type is assigned mechanically from the commit's conventional-commit prefix or keywords (`fix`, `feat`, `refactor`, other).
- Secondary metrics: medians and paired differences.
- Validity checks:
  - parity failures;
  - infra-failure rates per arm;
  - Forge actually active in its arm (hook events present);
  - tasks at ceiling or floor;
  - per-repository heterogeneity.
