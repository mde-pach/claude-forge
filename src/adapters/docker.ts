/**
 * docker, from PATH (§16), for its Compose plugin: what Forge asks is Compose's
 * (docker/compose/config-valid: `docker compose config --quiet --no-interpolate
 * --no-env-resolution`), so the version is the Compose plugin's (`docker compose version`), not
 * the docker CLI's. Compose v1 (`docker-compose`, Python) is another program and not covered.
 *
 * - 2.0.0 (the first `docker compose` plugin release) to 2.24.6: `config --no-interpolate`
 *   still reads each service's `env_file`, so a compose file naming an uncommitted `.env`
 *   fails (verified: 2.0.0 exit 14, 2.24.6 exit 14 `env file .../.env not found`). 2.24.7
 *   restored it (release notes v2.24.7: "restore support for `config --no-interpolate`"
 *   #11556, "Fix `--no-interpolate` flag on `config`"; 2.25.0 #11604 "Restore previous `config`
 *   behavior until `--no-interpolate` is set"). Before 2.24.7, `validate` cannot.
 * - 2.35.0 added `--no-env-resolution` (release notes v2.35.0: "Introduce config
 *   `--no-env-resolution`" #12665); before it the flag is unknown. From 2.24.7 to 2.34.x
 *   `--quiet --no-interpolate` alone already leaves env_file unread (verified on 2.24.7,
 *   2.25.0, 2.32.4, 2.33.0, 2.34.0: a missing or malformed env_file passes, as on 2.35.0 with
 *   the flag), so that range drops the flag.
 * - 2.33.0 removed the per-error-type exit codes (release notes v2.33.0: "remove exit code per
 *   error type used by legacy metrics system" #12502): before, an unknown flag exits 16, a
 *   schema error 15, a missing file 14; from 2.33.0 all exit 1. The usage signature below is
 *   2.33.0's; on 2.24.7-2.32.x a usage error (exit 16) would read as a finding (Forge's own
 *   arguments there are known good).
 * - Compose 5 (5.0.0, 5.6.1) runs the same command; only the schema messages changed
 *   (`additional properties 'notakey' not allowed`, 5.6.1 `services.web.notakey false schema`).
 * - In every release run, the project's `.env` is still read (a malformed `.env` fails with `failed
 *   to read .env: line 1: ...`, verified on 2.24.6, 2.24.7, 2.25.0, 2.35.0, 5.6.1): `--no-interpolate`
 *   keeps its values out of the output, it does not skip the file.
 */
import type { ToolAdapter } from "./types.js";

export const DOCKER: ToolAdapter = {
  tool: "docker",
  runner: "path",
  versionArgs: "compose version",
  verified: "5.6.1",
  usage: { exit: [1, 16], pattern: "^unknown (shorthand )?flag: |^invalid argument .+ flag|^flag needs an argument" },
  ranges: [
    {
      from: "2.0.0",
      to: "2.24.7",
      source: "github.com/docker/compose/releases/tag/v2.24.7: restore support for config --no-interpolate (#11556)",
      ops: { validate: null },
    },
    {
      from: "2.24.7",
      to: "2.35.0",
      source: "github.com/docker/compose/releases/tag/v2.24.7 (#11556); v2.35.0 (#12665: --no-env-resolution)",
      ops: { validate: "compose config --quiet --no-interpolate" },
    },
    {
      from: "2.35.0",
      source: "github.com/docker/compose/releases/tag/v2.35.0: Introduce config --no-env-resolution (#12665)",
      ops: { validate: "compose config --quiet --no-interpolate --no-env-resolution" },
    },
  ],
};
