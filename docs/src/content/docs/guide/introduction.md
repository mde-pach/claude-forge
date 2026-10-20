---
title: Introduction
description: What Forge is, the principles it follows, and how this documentation is built.
---

Forge is a command-line generator. It reads **rules**, grouped in a tree of **packs**, and writes a Claude Code setup for your user account (`~/.claude/`) or for one git repository. Once written, the setup runs with plain Claude Code: no generated file calls Forge.

## What you get

At **user level**, once per machine: `deny` permissions on secrets and history-destroying git commands, the Bash sandbox with network limited to package registries and GitHub, a tripwire on dangerous Bash commands, and terminal notifications when Claude waits for you.

At **project level**, for the stack Forge detects:

- after each edit, the file is formatted, linted and checked against your stack's ast-grep rules, and the findings go back to Claude;
- a type check runs in the background and wakes Claude on errors;
- when Claude stops, `.forge/check.sh` verifies the change — integrity of the setup, applied migrations, deleted tests, skips and asserts, then format, lint, typecheck, migrations and the related tests — and sends Claude back to work if it fails;
- the same `check.sh` runs in your pre-commit hook and in CI.

## Principles

1. **Standalone output.** Generated files never call Forge.
2. **Nothing to install.** Hooks are POSIX `sh`; project tools are the project's own binaries; ast-grep runs through a pinned `bunx` / `uvx`.
3. **Forge writes only what it owns.** Tool configs, manifests, git config and `CLAUDE.md` are audited, never written.
4. **Rules are data.** One YAML file per rule, with examples that are also its tests.
5. **Enforce in the closest native place.**
6. **Claude cannot change the setup unnoticed.**
7. **Success is silent.** Every message starts with `forge:<rule-id>`.

## How to read this site

- The [pack tree](../../packs/) is generated from `packs/` on every build: each rule with its targets, examples and source.
- The [specification](../../spec/) is the test suite in `tests/spec/`. Each test title is a requirement and its code is how it is checked; the status next to it is the latest CI result. When this site says Forge does something, a test says so too.
- The guide pages explain how to use Forge and link to the specification for the details.
