### github-actions

Detected by `.github/workflows/*.yml` or `*.yaml`.

| Rule | Targets |
|---|---|
| `github-actions/zizmor` | `check` lint: `forge_zizmor` (§13.2) on changed workflows, actions and `.github/dependabot.y*ml`, failing on the `unpinned-uses`, `template-injection`, `dangerous-triggers`, `excessive-permissions`, `artipacked`, `cache-poisoning`, `github-env`, `insecure-commands`, `overprovisioned-secrets`, `unredacted-secrets`, `secrets-inherit`, `bot-conditions`, `obfuscation`, `unsound-condition`, `unsound-contains`, `unsound-ternary`, `hardcoded-container-credentials`, `unpinned-images`, `ref-version-mismatch`, `archived-uses`, `typosquat-uses`, `use-trusted-publishing`, `dependabot-cooldown` and `dependabot-execution` audits |
| `github-actions/zizmor-advice` | `check` lint, advisory (§10.7): `forge_advise forge_zizmor adhoc-packages unpinned-tools superfluous-actions` |
| `github-actions/ci-ratchet` | `grep` no-increase on workflows: `continue-on-error: true`, a run line ending in `\|\| true` / `\|\| exit 0` / `\|\| :`, `if: false` (or `${{ false }}`), `paths-ignore:` |
| `github-actions/no-removed-jobs` | `check` lint: `forge_ci_jobs`: a job of a changed workflow that ran on `push` or `pull_request` in the base and is gone now (the whole file deleted included), `path:0 job <id> was removed …`, unless `forge.yaml` accepts that line |
| `github-actions/actionlint` | `check` lint: `forge_actionlint`: pinned actionlint (§13.5) on the changed workflows (every tracked one with `--all`), `-shellcheck= -pyflakes=` (results do not depend on what is installed), with a config of Forge's own holding only the `self-hosted-runner` and `config-variables` blocks of `.github/actionlint.y*ml` (its `paths: … ignore:` do not apply); `path:line:column: kind: message` |

context.md: actions pinned to a commit SHA, `permissions: {}` then per job, `persist-credentials: false`, no `${{ }}` inside `run:`, no `pull_request_target` / `workflow_run` on untrusted code.
