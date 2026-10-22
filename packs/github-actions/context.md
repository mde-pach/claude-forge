GitHub Actions. Actions are pinned to a full commit SHA with the version in a comment (`uses: actions/checkout@<sha> # v5`); zizmor's unpinned-uses check rejects tags.
Workflows set `permissions: {}` at the top and grant each job only what it needs (excessive-permissions).
`actions/checkout` sets `persist-credentials: false` unless a later step pushes (artipacked).
Attacker-controllable `${{ }}` values (issue and PR titles, bodies, branch names) inside `run:` are rejected (template-injection): pass them through `env:` and use the shell variable.
`pull_request_target` and `workflow_run` run with secrets on untrusted code; zizmor's dangerous-triggers check rejects them.
