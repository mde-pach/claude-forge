# End-to-end harness (§14.4)

**Status: written, not run.** These scenarios need real `claude -p` sessions
(an authenticated Claude Code ≥ 2.1.295) in a clean container with PostgreSQL;
the development environment where Forge was built had neither. Expect to adjust
some regexes in `run.ts` on the first real run: they match the hook output Forge
produces, but the exact framing in Claude Code's stream and debug log was not
observed.

```sh
sh tests/e2e/prepare.sh                     # host: copy samples, forge apply, commit
docker build -t forge-e2e tests/e2e
docker run --rm -e ANTHROPIC_API_KEY forge-e2e            # all scenarios
docker run --rm -e ANTHROPIC_API_KEY forge-e2e sh -c \
  'service postgresql start && sh /e2e/setup-repos.sh && bun /e2e/run.ts nextjs'
```

- `prepare.sh` runs on the host with Forge built from this checkout. The container
  never has Forge: it gets the already-committed repositories.
- `fixtures/` completes the generator samples for real runs: an async Alembic
  `env.py`, a real first revision, and `.env` files pointing at the container's
  PostgreSQL (fake credentials).
- `setup-repos.sh` installs each sample's own dependencies (`uv sync`, `bun install`)
  and creates the databases.
- Each scenario resets its repository (`git reset --hard`, `git clean`), runs one
  or more prompts in one session (`--continue`), then checks regexes against the
  stream-json output and the `--debug-file` log. Logs stay in `/work/.logs/`.
