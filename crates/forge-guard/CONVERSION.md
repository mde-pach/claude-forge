# Converting the guard rules

The 58 built-in guard rules (`grep -rl "target: guard" packs`) are written as a POSIX ERE run on
the whole JSON-escaped command. forge-guard matches them per simple command instead
(`src/ere.rs`), and proposes a structural form for each (`src/converted.yaml`, read by
`converted::rule`; the field reference is `CommandMatch` in `src/rule.rs`). The packs are not
edited yet; the loader can carry the structural form once the schema accepts it.

## How a pack ERE is matched per simple command

- The command line is parsed into simple commands. Each one is shown as one line: its
  assignments, words (quoting removed), redirections (`> target`) and input (`<< 'body'`),
  separated by single spaces. A word holding whitespace is shown between single quotes.
- The ERE is split into its top-level alternatives:
  - An alternative opening with a boundary group `(^|…)` must match at a **command position**:
    the start of the command, of an assignment, or of a command name. Command names include
    what a wrapper runs (`sudo`, `env`, `nice -n 5`…) and what a runner runs (`npx`, `bunx`,
    `bun x|run`, `npm exec|run`, `pnpm exec|dlx|<bin>`, `uv run`, `uvx`, `poetry run`,
    `python <script>`, `python -m <module>`…). A directory may come before the name.
  - Any other alternative may match anywhere, except starting inside a quoted word that holds
    whitespace.
- An ERE with an escaped pipe `\|` is matched against the whole pipeline: its stages joined
  by ` | `.

All 58 rules pass their own pack examples this way (`tests/examples.rs`,
`pack_patterns_per_simple_command_on_their_examples`). They also pass the §14.3 corpus
(`tests/corpus.rs`).

Matching per simple command fixes the reported false positives:

- `git commit -m "remove chmod 777"`
- `gh pr create --body "…git push --force…"`
- `which pnpm`

It does not fix `pip install --dry-run`. Some patterns also still match too much, because a
regex cannot tell where an argument ends:

- `[^;&|]*` or `.*` after the command name runs into a quoted argument or a heredoc body of
  the same command. For example, `cat > notes.md <<'EOF' … cat .env …` is denied.
- A flag pattern tied to no command matches any command, so `rg -- --env-mode=loose` is
  denied.
- A flag cannot be seen as absent, as in `pip install --dry-run`.

## Summary

| | rules |
|---|---|
| Exact as written, per simple command: the pattern only reads fixed words right after the command name | 10 |
| Pass their examples, but need the structural form to stop matching prose or other commands | 44 |
| Need a built-in check: `python-pm`, `package-add`, `ephemeral-exec` (its pattern was only a pre-filter) | 3 |
| Dropped: `safety/forge-state` (see its row) | 1 |

The structural forms pass every pack example (`converted_rules_pass_their_pack_examples`) and
the corpus with its false-positive cases (`corpus_with_the_converted_rules`).

Key for the "Per command" column:

- **exact**: the pattern works per simple command unchanged.
- **prose**: it passes its examples, but `[^;&|]*` or `.*` can reach a quoted word or heredoc of the same command.
- **any command**: an unanchored flag or variable pattern, matched on whatever command carries it.
- **check**: the rule needs logic, not a pattern.

| Rule | Per command | Proposed structural form |
|---|---|---|
| python/django/migrate-guard | prose (`.*`) | `{name: [manage.py, django-admin], sub: [migrate], flags_any: [--fake]}` · `args_any: [zero]` · `sub: ['flush\|reset_db']` · `sub: [sqlflush], pipe_to: ['*']` |
| python/sqlalchemy/alembic/downgrade | prose | `{name: alembic, sub: ['downgrade\|stamp']}` |
| python/sqlalchemy/alembic/no-downgrade-base | prose | `{name: alembic, sub: [downgrade, base]}` |
| python/uv/no-pip | check: pip with `--dry-run`, or into another environment, is still read as an install | `deny` + `check: python-pm`; ask for poetry/pdm `add\|install`, `pipenv install`, conda/mamba/micromamba `install`, `{name: [venv, virtualenv]}` (also `python -m venv`). The description is reworded, since pip is now denied |
| safety/cdk-destructive | prose | `{name: cdk, sub: [deploy], flags_any: ['--require-approval=never']}` · `sub: [destroy]` |
| safety/chmod-777 | prose | `{name: chmod, args_any: ['777', '0777', 'a+rwx', 'ugo+rwx', '=rwx']}` · `{name: chown, flags_any: [-R, --recursive], args_any: ['/', '~', '$HOME', …]}` |
| safety/cloud-destructive | prose | aws `sub: ['*', 'delete-*\|terminate-instances\|deregister-*\|rb']`, `sub: [s3, rm], flags_any: [--recursive]`; gcloud/az/doctl `args_any: [delete]`; gsutil, fly, railway, vercel, heroku, netlify, wrangler by `sub` |
| safety/datastore-reset | prose (`.*`) | `{name: redis-cli, stdin: true, arg_regex: '(?im)^\s*flush(all\|db)\b'}` · `{name: supabase, sub: [db, reset]}` · `{name: manage.py, sub: [flush]}` |
| safety/docker-privileged | prose | `{name: [docker, podman], sub: ['run\|create\|exec'], flags_any: [--privileged, '--pid=host', '--cap-add=SYS_ADMIN', '--security-opt=seccomp=unconfined', …]}` · `option_values: {'-v\|--volume': '^(/\|/etc\|/root\|~\|\$HOME)/?(:\|$)', '--mount': …}` · `arg_regex: 'docker\.sock'` |
| safety/docker-push | prose | `sub: [push]` · `sub: [image, push]` · `sub: [buildx], flags_any: [--push]` · `sub: [compose], args_any: [push]` · `{name: docker-compose, sub: [push]}` |
| safety/docker-volumes | prose | `{name: docker, sub: [compose], args_any: [down], flags_any: [-v, --volumes]}` · `sub: [volume, 'rm\|prune']` · `sub: [system, prune], flags_any: [--volumes]` |
| safety/ephemeral-exec | check (its pattern only pre-filters) | `check: ephemeral-exec` alone: the check reads every command position |
| safety/forge-state | any command | **dropped.** Forge's state moves to the per-user cache. The PreToolUse refusal of internal paths covers it with a neutral reason. The rule's description names those internals, which the agent must not be pointed at |
| safety/git-branch-force-delete | prose | `{name: git, sub: [branch], flags_any: [-D]}` · `flags_all: ['-d\|--delete', '-f\|--force']` |
| safety/git-discard | prose: walks words without excluding quotes | `{name: git, sub: [checkout], args_any: ['.']}` · `arg_regex: '^--$'` · `sub: [restore], flags_none: [--staged, -S]` · `sub: [restore], flags_any: [--worktree, -W]` |
| safety/git-force-push | prose | `{name: git, sub: [push], flags_any: [--force, -f]}` · `args_any: ['+*']` |
| safety/git-force-with-lease | prose | `{name: git, sub: [push], flags_any: [--force-with-lease]}` |
| safety/git-no-verify | prose (the rm/mv target pattern) | `{name: git, sub: [commit], flags_any: [--no-verify, -n], value_flags: [-m, -F, …]}` · `{env_any: [SKIP, HUSKY, …]}` · `option_values: {'-c': '^core\.hooks[Pp]ath='}` · config set/unset forms · `sub: [uninstall]` · `{name: [rm, mv, chmod, truncate], arg_regex: '(^\|/)(\.git/hooks\|\.husky)(/\|$)'}`. The description is reworded: it said "Forge's check" |
| safety/git-stash-drop | exact | `{name: git, sub: [stash, drop]}` |
| safety/kubernetes-change | prose | `{name: kubectl, sub: ['delete\|drain\|edit\|patch\|replace']}` · `sub: [scale], flags_any: ['--replicas=0']` · `{name: helm, sub: ['uninstall\|delete\|del\|un\|rollback']}` |
| safety/kubernetes-destructive | prose | `{name: kubectl, sub: [delete, '/^(ns\|namespaces?\|pv\|pvc\|…\|crds?\|…)([/.,]\|$)/']}` · `sub: [delete], flags_any: [--all, -A, --all-namespaces]` |
| safety/lockfile-bash-write | prose / any command (the redirect alternative) | sed/perl `flags_any: [-i, --in-place]` + lock `arg_regex` · `{redirect_to: '(^\|/)<lock>$'}` · tee/truncate `args_any` · cp/mv/install/rsync `last_arg` · dd `of=` · python/node/bun/deno/ruby `stdin: true, arg_regex: <lock>.*<write>\|<write>.*<lock>` |
| safety/no-break-system-packages | any command | `{name: [pip, pip3, 'pip3.*', uv, pipx], flags_any: [--break-system-packages]}` · `{env_any: [PIP_BREAK_SYSTEM_PACKAGES]}` · `{name: [pip, …], sub: [install], wrapped_by: [sudo, doas]}` · `{name: [npm, pnpm, yarn, bun], sub: ['i\|install\|add\|global'], wrapped_by: [sudo, doas]}` |
| safety/package-add | check: `--dry-run` was read as an option, then a package | `check: package-add` |
| safety/prisma-ai-consent | any command (the name, even in prose) | `{env_any: [PRISMA_USER_CONSENT_FOR_DANGEROUS_AI_ACTION]}` |
| safety/pulumi-destructive | prose | `{name: pulumi, sub: ['destroy\|cancel']}` · `sub: [stack, rm]` · `sub: [state, 'delete\|unprotect']` · `sub: ['up\|refresh'], flags_any: [--yes, -y, --skip-preview]` |
| safety/remote-script | prose (`[^\|]*`) | `{name: [curl, wget], pipe_to: [sh, bash, zsh, dash, ksh, 'python*', bun, node]}`: the next stage's command, after `sudo` · `{name: base64, flags_any: [-d, -D, --decode], pipe_to: […]}` · new: `{name: [sh, bash, …], arg_regex: '^(<\(\|\$\()\s*(curl\|wget)\s'}` |
| safety/rm-root | prose | `{name: rm, flags_any: [-r, -R, --recursive], args_any: ['/^(/\|~\|\$HOME\|\$\{HOME\}\|\.\.)/?\*?$/']}` · bare variable `args_any: ['/^\$\{?[A-Za-z_][A-Za-z0-9_]*\}?/\*?$/']` |
| safety/secret-read-bash | prose | `{name: [cat, less, more, head, tail, cp, source, '.', grep, base64], arg_regex: '(^\|[/=])\.env(\.(local\|…))?$'}` · `arg_regex: '^(~\|\$HOME\|\$\{HOME\})/(\.ssh/\|\.pgpass\|\.netrc)'` |
| safety/sql-destructive | prose (`.*` by design reads the SQL) | `{name: [psql, mysql, mariadb, sqlite3], stdin: true, arg_regex: '(?i)(\bdrop\s+(table\|database\|schema)…\|\btruncate…\|\bdelete\s+from\s+\S+\s*(;\|$))'}` · mongosh/mongo `.drop(` · `{name: heroku, sub: ['pg:reset']}` · `{name: supabase, sub: [db, reset], flags_any: [--linked, --db-url]}` |
| safety/terraform-destroy | prose | `{name: [terraform, tofu], sub: [destroy]}` · `sub: [apply], flags_any: [-auto-approve, -replace, -target, '-lock=false']` · `sub: [state, 'rm\|mv\|push\|replace-provider']` · `sub: ['force-unlock\|import']` · `sub: [workspace, delete]` |
| supply-chain/insecure-download | prose | `{name: curl, flags_any: [-k, --insecure], value_flags: [-o, -H, -d, …]}` · `{name: wget, flags_any: [--no-check-certificate]}` |
| supply-chain/no-dependency-confusion-command | prose / any command | `{name: [pip, pip3, 'pip3.*', uv], flags_any: [--extra-index-url]}` · `{name: [uv, uvx], option_values: {'--index-strategy': '^unsafe-'}}` |
| supply-chain/no-tls-disable-command | exact / any command | npm/pnpm/yarn `sub: [config, set, strict-ssl, 'false']` (and `strict-ssl=false`) · `{flags_any: ['--strict-ssl=false', --allow-insecure-host, --trusted-host]}` · `{env_any: [GIT_SSL_NO_VERIFY, PIP_TRUSTED_HOST, UV_INSECURE_HOST, 'NODE_TLS_REJECT_UNAUTHORIZED=0', 'PYTHONHTTPSVERIFY=0']}` · git `-c http.sslVerify=false` and `config [set] http.sslVerify false` |
| typescript/bun/no-other-pm | exact | `{name: npm, sub: ['install\|i\|ci\|add']}` · `{name: [pnpm, yarn]}` |
| typescript/bun/no-snapshot-update | prose | `{name: bun, sub: [test], flags_any: [-u, --update-snapshots]}` |
| typescript/drizzle/no-push-force | prose | `{name: drizzle-kit, sub: [push], flags_any: [--force]}` |
| typescript/drizzle/no-push | exact | `{name: drizzle-kit, sub: [push]}` |
| typescript/jest/no-test-path-pattern | prose | `{name: jest, flags_any: [--testPathPattern]}` |
| typescript/mikro-orm/schema-guard | prose | `{name: [mikro-orm, mikro-orm-esm], sub: ['schema:drop\|schema:fresh'], flags_any: [--run, -r]}` |
| typescript/mikro-orm/schema-update | prose | `sub: ['schema:update'], flags_any: [--run, -r]` · `sub: ['migration:fresh']` |
| typescript/node/no-ts-node | exact | `{name: [ts-node, ts-node-esm, ts-node-script]}` |
| typescript/npm/allow-scripts-command | exact / any command | `{name: npm, sub: [approve-scripts, '*']}` · `flags_any: [--all, --no-allow-scripts-pin]` · `sub: [install-scripts, approve]` · `flags_any: [--dangerously-allow-all-scripts]` · `option_values: {'--allow-scripts': '^[^-]'}` |
| typescript/npm/no-audit-fix-force | prose | `{name: npm, sub: [audit, fix], flags_any: [--force]}` |
| typescript/npm/no-exotic-sources | any command | `{flags_any: ['--allow-git=all', '--allow-remote=all'], value_flags: [--allow-git, --allow-remote]}` |
| typescript/npm/no-legacy-peer-deps | prose | `{name: npm, sub: ['install\|i\|ci\|add'], flags_any: [--legacy-peer-deps, --force]}` |
| typescript/npm/no-other-pm | exact | `{name: [pnpm, yarn]}` · `{name: bun, sub: ['install\|i\|add']}` |
| typescript/npm/release-age-command | any command | `{option_values: {'--min-release-age': '^0*$'}}` |
| typescript/playwright/no-snapshot-update | prose | `{name: playwright, sub: [test], flags_any: [--update-snapshots, -u]}` |
| typescript/pnpm/no-other-pm | exact | `{name: npm, sub: ['install\|i\|ci\|add']}` · `{name: yarn}` · `{name: bun, sub: ['install\|i\|add']}` |
| typescript/prisma/db-push | exact | `{name: prisma, sub: [db, 'push\|execute']}` · `sub: [migrate, deploy]` |
| typescript/prisma/no-reset | exact / prose | `{name: prisma, sub: [migrate, reset]}` · `sub: [db, push], flags_any: [--force-reset, --accept-data-loss]` |
| typescript/turbo/no-env-loose-flag | any command | `{name: turbo, flags_any: ['--env-mode=loose'], value_flags: [--env-mode]}` (also as `pnpm turbo`) |
| typescript/typeorm/migration-revert | exact | `{name: [typeorm, typeorm-ts-node-esm, typeorm-ts-node-commonjs], sub: ['migration:revert']}` |
| typescript/typeorm/schema-sync-guard | exact | `… sub: ['schema:sync\|schema:drop']` |
| typescript/vitest/no-pass-with-no-tests | prose | `{name: vitest, flags_any: [--passWithNoTests]}` |
| typescript/vitest/no-snapshot-update | prose | `{name: vitest, flags_any: [-u, --update]}` (also `bun run vitest`) |
| typescript/yarn/no-other-pm | exact | `{name: npm, sub: ['install\|i\|ci\|add']}` · `{name: pnpm}` · `{name: bun, sub: ['install\|i\|add']}` |

## Notes for the pack pass

- Three rules (jest/no-test-path-pattern, node/no-ts-node and npm/no-exotic-sources) share
  their string examples between a `grep` target and the guard. A JSON object or a line of
  `.npmrc` is file content, not a command. The tests skip those strings for the guard; the
  pack examples should move commands to `{command: …}`.
- `python-pm` replaces the old ask on `pip install` in a uv project with a deny. The reason
  is: "This project uses uv: `uv add <pkg>` (`--dev` for tooling)." The check adds the exact
  command, for example "Here: `uv add requests`." or "Here: `uv sync` installs the project
  itself." A pip install aimed at another environment is not denied:
  - `--target`, `--prefix`, `--root` or `--system`;
  - `--python <path>` outside the repository;
  - a pip or python run by a path outside the repository;
  - `VIRTUAL_ENV=` set outside the repository;
  - a command run from outside the repository.

  `safety/package-add` still asks about these. A plain-scripts project (`python.pm` = `scripts`)
  gets the `uv init --bare` wording.
