# Forge — Build Specification v0.5

Forge is a command-line generator. It reads rules grouped in packs and writes a standalone Claude Code setup at user level (`~/.claude/`) or project level (a git repository). Once written, the setup runs with plain Claude Code; no generated file calls Forge.

Every requirement uses MUST / MUST NOT. Anything not described here is out of scope.

The specification is split by section; each section's tests are `tests/spec/<NN>-*.test.ts`, one requirement per test title. Section numbers (`§10.4`) are stable.

| § | Section |
|---|---|
| §1 | [Principles](spec/01-principles.md) |
| §2 | [Stack and platform](spec/02-stack-and-platform.md) |
| §3 | [Forge CLI](spec/03-forge-cli.md) |
| §4 | [Ownership](spec/04-ownership.md) |
| §5 | [Inputs](spec/05-inputs.md) |
| §6 | [Rules](spec/06-rules.md) |
| §7 | [Generated output — user level](spec/07-generated-output-user-level.md) |
| §8 | [Generated output — project level](spec/08-generated-output-project-level.md) |
| §9 | [Shared script conventions](spec/09-shared-script-conventions.md) |
| §10 | [`check`](spec/10-check.md) |
| §11 | [Audit of tool targets](spec/11-audit-of-tool-targets.md) |
| §12 | [Lock](spec/12-lock.md) |
| §13 | [Packs and rules](spec/13-packs-and-rules/README.md) |
| §14 | [Acceptance tests](spec/14-acceptance-tests.md) |
| §15 | [Engine capabilities](spec/15-engine-capabilities.md) |
