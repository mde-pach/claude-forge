<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `github-actions`

### github-actions

GitHub Actions workflows, audited with zizmor.

Detected by: a file matching `.github/workflows/*.yml`, `.github/workflows/*.yaml`.

Context (in `.claude/rules/forge-*.md`, §7):

- GitHub Actions. Actions are pinned to a full commit SHA with the version in a comment (`uses: actions/checkout@<sha> # v5`), never a tag or branch, which can be moved.
- Workflows set `permissions: {}` at the top and grant each job only what it needs.
- `actions/checkout` sets `persist-credentials: false` unless a later step pushes.
- Attacker-controllable `${{ }}` values (issue and PR titles, bodies, branch names) are never expanded inside `run:`: pass them through `env:` and use the shell variable.
- `pull_request_target` and `workflow_run` run with secrets on untrusted code: do not add them.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `github-actions/actionlint` | Workflows pass actionlint: expression type errors, unknown with: inputs of known actions, deprecated set-output / save-state / set-env commands, invalid cron, needs cycles, unknown runner labels (self-hosted labels come from .github/actionlint.yaml). | any: `.github/workflows/*.yml`, `.github/workflows/*.yaml` | check lint: `forge_actionlint` (when `.github/workflows/**`, `.github/actionlint.yaml` or `.github/actionlint.yml` changes) |
| `github-actions/ci-ratchet` | Workflow changes do not switch checks off: a changed workflow may not have more continue-on-error: true, run lines ending in \|\| true / \|\| exit 0 / \|\| :, if: false or paths-ignore than it had. Fix the failing step instead; switching a check off is the user's decision: ask them. | any: `.github/workflows/*.yml`, `.github/workflows/*.yaml` | grep no-increase `continue-on-error:[[:space:]]*(true\|\$\{\{[[:space:]]*true[[:space:]]*\}\})\|\\|\\|[[:space:]]*(true\|exit[[:space:]]+0\|:)([[:space:];)"']\|$)\|^[[:space:]]*-?[[:space:]]*if:[[:space:]]*(false\|\$\{\{[[:space:]]*false[[:space:]]*\}\})[[:space:]]*$\|paths-ignore:` |
| `github-actions/no-removed-jobs` | A job removed from a workflow that runs on push or pull_request (the whole workflow deleted included) is reported: restore it rather than switching a check off; removing a check is the user's decision: ask them. | any: `.github/workflows/*.yml`, `.github/workflows/*.yaml` | check lint: `forge_ci_jobs` (when `.github/workflows/*.yml` or `.github/workflows/*.yaml` changes) |
| `github-actions/zizmor-advice` | zizmor's adhoc-packages (packages installed in run: steps outside a lock file), unpinned-tools and superfluous-actions (actions that duplicate what the runner image provides) audits. | any: `.github/workflows/*.yml`, `.github/workflows/*.yaml`, `.github/actions/**/action.yml`, `.github/actions/**/action.yaml`, `action.yml`, `action.yaml` | check lint: `forge_advise forge_zizmor adhoc-packages unpinned-tools superfluous-actions` (when `.github/workflows/**`, `.github/actions/**`, `action.yml` or `action.yaml` changes)<br>advice, never blocks |
| `github-actions/zizmor` | Workflows, actions and dependabot.yml pass zizmor's unpinned-uses (actions pinned to a commit SHA, cf. the tj-actions/changed-files compromise, CVE-2025-30066), template-injection, dangerous-triggers, excessive-permissions, artipacked, cache-poisoning, github-env, insecure-commands, overprovisioned-secrets, unredacted-secrets, secrets-inherit, bot-conditions, obfuscation, unsound-condition, unsound-contains, unsound-ternary, hardcoded-container-credentials, unpinned-images, ref-version-mismatch, archived-uses, typosquat-uses, use-trusted-publishing, dependabot-cooldown and dependabot-execution audits. | any: `.github/workflows/*.yml`, `.github/workflows/*.yaml`, `.github/actions/**/action.yml`, `.github/actions/**/action.yaml`, `action.yml`, `action.yaml`, `.github/dependabot.yml`, `.github/dependabot.yaml` | check lint: a shell script calling `forge_zizmor` (when `.github/workflows/**`, `.github/actions/**`, `action.yml`, `action.yaml`, `.github/dependabot.yml` or `.github/dependabot.yaml` changes) |
