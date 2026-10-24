# Decisions

Every ambiguous choice gets the most conservative option, recorded here with the reason. A conservative option is one that least favours either arm, or least risks a biased or irreproducible result.

## D1. Pins
- **Claude Code 2.1.310**, the native binary installed in this environment (`/opt/claude-code/bin/claude`). It is bind-mounted read-only into every trial container, and its sha256 is checked in the parity fingerprint. 2.1.310 satisfies Forge's requirement of ≥ 2.1.295.
- **Model `claude-sonnet-5-5`.** This is the model Claude Code 2.1.310 selects by default in this account (checked with `claude -p` without `--model`). I pin it explicitly so a default change cannot leak in. Effort is left at the CLI default in both arms, and not passed.
- **Forge `091c0d3`**, the head of the clone given to me, version 0.5.0. It is built with `bun run build` from a pristine `git worktree` of that commit, and no file of Forge is changed. Forge's ask-reviewer uses the same default model, because `FORGE_REVIEW_MODEL` is unset, as the docs leave it.
- **Dataset-building model: `claude-opus-5-5`**, for the instruction writer and the test auditor. These agents never see trial results.

## D2. Permission mode: `bypassPermissions` in both arms
Headless runs need a permission mode in which the agent can edit files and run commands. Forge's own end-to-end spec uses `acceptEdits`. In `-p` that mode silently denies every Bash command that is not allow-listed. The baseline then could not run tests, while in the Forge arm Forge's reviewer would approve the same commands. The arms would differ in *whether the agent may run commands at all*, which measures permission plumbing rather than Forge's guardrails.

`bypassPermissions` is the mode used for unattended agents in disposable containers. It is identical in both arms, and Forge's `deny` rules and hooks still apply in it: deny rules are honoured in bypass, and PermissionRequest and Pre/PostToolUse hooks fire. It is a property of the invocation, not of Forge's setup, so it does not alter Forge. Caveat: in this mode Forge's `ask` entries are auto-accepted, so its ask-reviewer rarely fires. This is a threat to external validity, reported in REPORT.md.

## D3. Repositories
The procedure ran before any trial:
1. Survey 41 popular, active Python and TypeScript repositories (`mining/survey*.json`).
2. Keep a repository if it is eligible:
   - it is covered by a Forge pack;
   - it uses that pack's lint tool, ruff (`[tool.ruff]`) or Biome (`biome.json[c]`);
   - it has a lockfile (`uv.lock`, or pnpm/bun);
   - it has at least 15 candidate commits in the main window.
3. Rank the eligible repositories by the number of distinct non-bot authors of candidate commits (`mining/rank.py`, `mining/rank.json`). This is a proxy for a real, multi-contributor project that does not depend on star counts. I did not use GitHub search, because it is outside this session's GitHub scope.
4. Take the top 3 per language:
   - **Python:** pydantic/pydantic-ai (101 authors), jlowin/fastmcp (65), pydantic/logfire (35).
   - **TypeScript:** better-auth/better-auth (52), colinhacks/zod (20), inngest/inngest-js (9). These are the only three Biome users among the 26 TypeScript repositories surveyed.

Each repository contributes the first 10 accepted tasks, so the target is 60. If a repository's tests cannot install and run offline in a container, it is replaced by the next one in rank in its language: Python next is pydantic/pydantic (14), then encode/starlette (12). TypeScript has no eligible replacement, so a dropped TypeScript repository reduces the count. Six repositories at 10 tasks each, rather than four at 15, is the more conservative choice for generalisation across codebases.

## D4. Network: allow-list proxy, no internet for the agent
The agent container is on an `--internal` Docker network. Its only route out is a host-side CONNECT proxy that allows `api.anthropic.com` (plus the Anthropic telemetry and feature-flag hosts Claude Code needs; the exact list is in `harness/netproxy.py`). The proxy forwards to the environment's egress proxy. Package registries and GitHub are unreachable, so the agent cannot fetch the upstream fix or new dependencies, in either arm.

Dependencies are installed before the agent starts, offline, from caches baked into the repository image. Forge's pinned ast-grep, zizmor and hadolint are also pre-warmed into the shared image's bunx/uvx caches. Both arms run the same image, so this changes nothing between them, and Forge's hooks can run offline as they would on a developer machine that has used them once.

## D5. Developer-shell PATH
The project's virtualenv `bin/` (Python) or `node_modules/.bin` (TypeScript) is prepended to PATH in both arms, as in an activated dev shell (direnv or `source .venv/bin/activate`). Forge calls "the project's own binaries" by name (`pytest -q`, `ruff`, `biome`). Without this, its rules would be reported "not enforced" for a reason unrelated to Forge.

## D6. No third arm
An `acceptEdits` arm would measure the ask-reviewer (see D2), but it needs another 180 trials. A "Forge project level only" arm answers a narrower question that is not asked here. The budget goes to 3 trials per task instead, which tightens the primary comparison.

## D7. The Bash sandbox
`forge apply --user` enables Claude Code's Bash sandbox (bubblewrap). Docker's default seccomp and AppArmor profiles stop bubblewrap from creating namespaces, which is the failure Forge's docs describe. The docs' remedy is to give the container what bubblewrap needs. So **every** trial container, in both arms, runs with `--security-opt seccomp=unconfined --security-opt apparmor=unconfined --cap-add SYS_ADMIN`, and the shared image includes `bubblewrap` and `socat`. Then the Forge arm runs as Forge intends, and the baseline gets the same container privileges.

## D8. Workspace trust
Forge's docs say that headless Claude Code loads the project plugin only in a trusted workspace ("accept the trust dialog once, interactively"). The harness writes the equivalent `hasTrustDialogAccepted: true` entry for the workspace path into `~/.claude.json`, in **both** arms.

## D9. Starting repository without history
The agent's workspace is a fresh git repository with a single commit holding the tree of `C^`, the author fixed and the date fixed. Forge relies on git, and so does a normal developer. Upstream history is removed so the fix cannot be read from it. In the Forge arm, `forge apply` runs after that commit, and the result is committed as *Getting started* says (`git add .forge .claude && git commit -m "Add Forge"`). The baseline therefore has one commit and the Forge arm two; this is part of applying Forge as documented. The agent's diff is always taken against the root commit, excluding `.forge/` and `.claude/`.

## D10. Timeouts and budget
45 min wall clock and `--max-budget-usd 15` per trial, identical across arms. Hitting either limit is a scored outcome: the tree is scored as left. It is not an infrastructure error, because both arms face the same limit and the Forge hooks' own time is part of what is being measured.

## D11. Retries of infrastructure errors
At most 3 attempts. An attempt counts as infrastructure when:
- the agent never got a `result` event, because of an API error, a 5xx or overloaded response, a usage or rate limit, or a network error;
- container start or dependency install failed;
- the parity check failed;
- the scorer could not run the tests (collection error caused by the environment, not the code).

The classifier is a fixed regular-expression list in `harness/classify.py`. A scoring collection error caused by the agent's code, such as a syntax error, is a fail, not infra.

## D12. Offline package caches (from the image build; details in `harness/IMAGES.md`)
- **uv cache.** uv cannot use a read-only cache, even offline. Each container gets a throwaway overlay: the shared `/work/cache/uv` is the read-only lower layer and is never written. This is identical in every container.
- **`UV_OFFLINE=1` in the shared image (both arms).** Forge's hooks run ast-grep through `uvx`. On an offline machine uvx otherwise spends about 6 s per call retrying PyPI before Forge's own offline fallback. That delay is an artefact of our network isolation, not of Forge, so it is removed for both arms.
- **`UV_CACHE_DIR=/cache/uv` in the agent environment (both arms)**, so that `uv run pytest`, the way these repositories document running tests, works offline.
- **pydantic-ai: heavy local-model extras are not installed** (sentence-transformers, outlines-*, i.e. torch/CUDA or llama.cpp builds, about 5 GB per environment). Their tests skip at both the parent and the commit, so they can never be F2P. This applies equally to both arms.
- **Cache warm-up depth.** The first 60 main-window candidates plus 10 pilot candidates are warmed (logfire: 40, the pre-set floor, because of disk). Candidates are processed oldest first, and 10 accepted per repository are expected well within these.

## D13. Dependency install failures during dataset building
A candidate whose parent cannot be installed offline for a deterministic environmental reason is rejected as `install_failed`, and reported separately from the other reasons. The known example is a pinned package without a wheel for the image's Python, which would need a compiler. This is not a transient infrastructure failure (network, registry, DNS, disk, usage limit), so rule 5 does not apply. Re-running would give the same result. Transient failures, in contrast, leave the candidate unprocessed and are retried, by `build.py`'s exception path.

## D14. Hidden test files include non-code test assets
The hidden set is every file the commit changes under a test path, of any type: VCR cassettes, snapshots and fixtures, not only `.py`/`.ts` files. Test files the commit deletes are deleted at scoring. Only test *modules* are passed to the runner as targets.
