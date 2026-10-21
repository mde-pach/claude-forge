GitHub Actions. Actions are pinned to a full commit SHA with the version in a comment (`uses: actions/checkout@<sha> # v5`).
Workflows set `permissions: {}` at the top and grant each job only what it needs.
`actions/checkout` sets `persist-credentials: false` unless a later step pushes.
`${{ }}` expressions never go inside `run:`; pass them through `env:` and use the shell variable.
`pull_request_target` and `workflow_run` run with secrets on untrusted code; avoid them.
