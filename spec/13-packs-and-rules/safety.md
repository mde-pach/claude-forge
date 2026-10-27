### safety (user)

| Rule | Targets |
|---|---|
| `safety/secret-files` | `permission` deny: `Read(**/.env)`, `Read(**/.env.local)`, `Read(**/.env.development)`, `Read(**/.env.production)`, `Read(**/.env.test)`, same with `Edit(...)`, `Read(**/*.pem)`, `Read(**/*.key)`, `Read(~/.ssh/**)`, `Read(~/.netrc)`, `Read(~/.npmrc)`, `Read(~/.pypirc)`, `Read(~/.pgpass)`, `Read(~/.git-credentials)`, `Read(~/.config/gh/hosts.yml)`, `Read(~/.docker/config.json)` |
| `safety/sandbox` | `sandbox` `enabled: true`; `network.allowedDomains`: `registry.npmjs.org`, `pypi.org`, `files.pythonhosted.org`, `github.com` |
| `safety/git-reset-hard` | `permission` deny: `Bash(git reset --hard:*)` |
| `safety/git-clean` | `permission` deny: `Bash(git clean:*)` |
| `safety/git-discard` | `guard` deny: `git checkout -- …`, `git checkout .`, `git restore` without `--staged` |
| `safety/git-force-push` | `guard` deny: `git push` with `--force`, `-f` or `+<ref>`; not `--force-with-lease` |
| `safety/git-force-with-lease` | `guard` ask |
| `safety/git-no-verify` | `guard` deny: `git commit` with `--no-verify` or `-n`; a `SKIP=`, `PRE_COMMIT_ALLOW_NO_CONFIG=`, `HUSKY=`, `HUSKY_SKIP_HOOKS=`, `LEFTHOOK=` or `LEFTHOOK_EXCLUDE=` assignment at the start of a command; `git -c core.hooksPath=…`; `git config` setting or unsetting `core.hooksPath` (reading it is fine); `pre-commit` / `lefthook uninstall`; `rm`, `mv`, `chmod`, `truncate` on `.git/hooks/` or `.husky/` |
| `safety/git-history-rewrite` | `permission` deny: `Bash(git filter-branch:*)`, `Bash(git filter-repo:*)`, `Bash(git reflog expire:*)`, `Bash(git stash clear:*)` |
| `safety/rm-root` | `guard` deny: recursive `rm` on `/`, `~`, `$HOME`, `..`, or on a bare variable followed by `/` or `/*` (`$DIR/`, `${OUT}/*`: `/` when it is empty; `${VAR:?}` is fine) |
| `safety/remote-script` | `guard` deny: `curl` / `wget` piped to `sh`, `bash`, `zsh`, `python`, `bun`, `node`; `base64 -d` piped to a shell |
| `safety/publish` | `permission` deny: `Bash(npm publish:*)`, `Bash(bun publish:*)`, `Bash(uv publish:*)` |
| `safety/secret-read-bash` | `guard` deny: `cat`, `less`, `head`, `tail`, `cp`, `source`, `grep`, `base64` on `.env` files (not `.env.example`), `~/.ssh/`, `~/.pgpass`, `~/.netrc` |
| `safety/dropdb` | `permission` deny: `Bash(dropdb:*)` |
| `safety/sql-destructive` | `guard` deny: `psql`, `mysql`, `mariadb`, `sqlite3` with `DROP TABLE`, `DROP DATABASE`, `DROP SCHEMA`, `TRUNCATE` or a `DELETE FROM <table>` without `WHERE`; `mongosh` / `mongo` with `dropDatabase()` or `.drop()`; `heroku pg:reset`; `supabase db reset` with `--linked` or `--db-url` |
| `safety/datastore-reset` | `guard` ask (`review`: a local, disposable database the request calls for wiping): `redis-cli` `FLUSHALL` / `FLUSHDB`, `supabase db reset`, `manage.py flush` |
| `safety/docker-volumes` | `guard` deny: `docker compose down` / `docker-compose down` with `-v` or `--volumes`, `docker volume rm`, `docker volume prune`, `docker system prune --volumes` |
| `safety/package-add` | `guard` ask (`review`: each package well-known, needed and spelled right): a named package added at the start of a command (after `sudo`, `env` or assignments) by `npm` / `pnpm` / `yarn` / `bun` `install` / `i` / `add`, `yarn` / `pnpm global add`, `pip` / `pip3` / `uv pip install`, `python -m pip install`, `uv` / `poetry` / `pdm add`, `pipenv install`, `pipx install`, `uv tool install` (hallucinated package names, slopsquatting); installs from the lock file, `-r` / `-c` files, `-e` and `.` do not ask |
| `safety/no-vendored-edits` | `permission` deny: `Edit(**/node_modules/**)`, `Edit(**/.venv/**)`, `Edit(**/site-packages/**)` (patch through the package manager instead) |
| `safety/no-break-system-packages` | `guard` deny: `--break-system-packages`, `PIP_BREAK_SYSTEM_PACKAGES=`, `sudo` `pip install` / `python -m pip install` / `npm`, `pnpm`, `yarn`, `bun` `i`, `install`, `add`, `global` |
| `safety/lockfile-hand-edit` | `permission` ask: `Edit(**/pnpm-lock.yaml)`, `Edit(**/package-lock.json)`, `Edit(**/npm-shrinkwrap.json)`, `Edit(**/yarn.lock)`, `Edit(**/bun.lock)`, `Edit(**/uv.lock)`, `Edit(**/poetry.lock)`, `Edit(**/Cargo.lock)`, `Edit(**/deno.lock)`, `Edit(**/pdm.lock)`, `Edit(**/pixi.lock)`, `Edit(**/Pipfile.lock)`, `Edit(**/.terraform.lock.hcl)`, `Edit(**/flake.lock)`, `Edit(**/mise.lock)`, `Edit(**/go.sum)`, `Edit(**/Gemfile.lock)`, `Edit(**/composer.lock)`, `Edit(**/packages.lock.json)` (the package manager writes lock files) |
| `safety/lockfile-bash-write` | `guard` ask (`review`: is it a hand rewrite of a lock file the request asks for): the same lock files rewritten through Bash: `sed -i` / `perl -i` (`-0pi` included), a `>` / `>>` redirect or `tee` into one, `cp` / `mv` / `install` / `rsync` with one as the last argument, `dd of=`, `truncate`, a `python` / `node` / `bun` / `deno` / `ruby` command naming one and writing (`.write(`, `write_text`, `writeFile`, `Bun.write`, an `open(…, 'w')` mode) |
| `safety/prisma-ai-consent` | `guard` deny: any command containing `PRISMA_USER_CONSENT_FOR_DANGEROUS_AI_ACTION` (only the user gives that consent) |
| `safety/terraform-destroy` | `guard` ask: `terraform` / `tofu` `destroy`, `apply … -auto-approve`; `state rm` / `mv` / `push` / `replace-provider`, `force-unlock`, CLI `import`, `workspace delete`; `apply` with `-replace=`, `-target=` or `-lock=false` (`plan -target` is fine) |
| `safety/cdk-destructive` | `guard` ask: `cdk deploy … --require-approval never`, `cdk destroy` |
| `safety/pulumi-destructive` | `guard` ask: `pulumi up` / `pulumi refresh` with `--yes`, `-y` or `--skip-preview`; `pulumi destroy`, `pulumi stack rm`, `pulumi state delete` / `unprotect`, `pulumi cancel` |
| `safety/kubernetes-destructive` | `guard` deny: `kubectl delete` of namespaces, persistent volumes and claims, CRDs, or with `--all`, `-A`, `--all-namespaces` |
| `safety/kubernetes-change` | `guard` ask (`review`: a local or development cluster, and the change the user asked for): `kubectl delete`, `drain`, `edit`, `patch`, `replace`, `scale --replicas=0`; `helm uninstall` / `delete` / `rollback` |
| `safety/cloud-destructive` | `guard` ask (`review`: a disposable resource the user asked to delete): `aws … delete-*` / `terminate-instances` / `deregister-*` / `s3 rb` / `s3 rm --recursive`; `gcloud`, `az`, `doctl … delete`; `gsutil rm` / `rb`; `fly` apps / volumes / postgres / machines `destroy` / `delete`; `railway down` / `delete`; `vercel remove`; `heroku apps:destroy`; `netlify sites:delete`; `wrangler … delete` |
| `safety/chmod-777` | `guard` deny: `chmod` `777`, `0777`, `a+rwx`, `ugo+rwx`, `=rwx`; recursive `chown` of `/`, `~`, `$HOME` |
| `safety/git-stash-drop` | `guard` ask: `git stash drop` (`git stash clear` is denied by `git-history-rewrite`) |
| `safety/git-branch-force-delete` | `guard` ask: `git branch -D`, `--delete --force`, `-f -d` |
| `safety/ephemeral-exec` | `guard` ask, check `ephemeral-exec` (§9.2): packages fetched from a registry on the fly. `npx tsc`, `bunx prisma`, `uvx ruff` with the tool installed in the project are not asked; `npx some-typo` is |
| `safety/docker-privileged` | `guard` deny: `docker run` / `create` / `exec` with `--privileged`, `--pid=host`, `--ipc=host`, `--userns=host`, `--cap-add ALL` or `SYS_ADMIN`, `--security-opt seccomp=unconfined` or `apparmor=unconfined`, `docker.sock`; `run` / `create` mounting `/`, `/etc`, `/root`, `~` or `$HOME` (`-v`, `--volume`, `--mount source=`) |
| `safety/docker-push` | `guard` ask: `docker push`, `docker image push`, `docker compose push`, `docker buildx … --push` |
| `safety/forge-state` | `guard` deny: a command naming Forge's hook state (`forge-state`, `.forge/state`, `forge-review-cache`, `forge-reviews.log`, as whole names; §8) |
| `safety/forge-state-edit` | `permission` deny: `Edit(~/.claude/forge-state/**)` |
