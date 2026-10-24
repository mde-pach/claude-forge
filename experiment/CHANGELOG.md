# Changelog (changes after the design was written)

Every change to the harness, the rules or the dataset after DESIGN.md was written is listed here, with the reason. Pilot fixes are harness fixes made before any main-run trial existed.

## Before the pilot
- 2026-10-24: target changed from 4 repos × 15 to 6 repos × 10, before any dataset build (D3). This makes generalisation across codebases more conservative.

## Pilot fixes
- **P1 (pilot1):** The parity pre-check flagged every path under `/work/.claude/` as Forge-owned. fastmcp ships its own `.claude/` directory (settings, a SessionStart hook, skills) as part of the upstream repository, present in both arms. The pre-apply check now rejects only Forge-specific markers: `.forge/`, `.claude/skills/forge-project`, `.claude/rules/forge-*`, `~/.claude/skills/forge-user`, `~/.claude/settings.json`. The post-apply check is unchanged: everything `forge apply` changes must be Forge-owned or a listed side effect.
- **P2 (pilot1):** the agent environment overrode PATH, dropping `/opt/node/bin`, so `forge apply` could not find node. PATH is now inherited from the container: the image PATH with the repo's `path_prepend`, identical in both arms. The container's full environment is now part of the parity fingerprint. On failed attempts the runner keeps logs (`_logs/`) and full pre-agent fingerprints, for diagnosis.
- **P3 (pilot1):** Forge also writes `~/.claude/forge.lock.json` (user-level lock), which is now in the Forge-owned set (`~/.claude/forge*`). Stale same-named containers from killed runs are removed before a container starts.
- **P4 (pilot1):** the Forge setup is now committed after `forge apply`, as Getting started documents. Left uncommitted, Forge saw its own files as "59 changed files" at session start. DECISIONS D9 is updated.
- **P5 (pilot1):** two harness bugs that the pilot exposed and that would have invalidated scoring. (a) `final.diff` was empty in both arms: git ran as root on a repository owned by `dev` ("dubious ownership"), and the error was swallowed. Diff collection now runs as `dev`, and a git error is an infra error. (b) A dangling `latest` symlink from Claude Code's debug log crashed log copying. Added `harness/selftest_score.py`: on every task, the reference patch must be resolved and an empty change must not. The orchestrator caps concurrent better-auth trials at 1, for disk. All pilot1 results were discarded and the pilot re-run.
