/**
 * @spec 14.2 · Generator samples
 *
 * Each sample repository in `tests/generator/` is applied and its output
 * compared with a stored snapshot (`FORGE_UPDATE_SNAPSHOTS=1` rewrites them).
 * The samples cover Python (FastAPI, SQLAlchemy, Alembic, pytest, uv),
 * TypeScript with Next.js + React + Prisma + Vitest + Playwright on Bun, Astro
 * with React islands, and a repository that uses ESLint instead of Biome.
 */
import { describe, expect, test } from "bun:test";
import { expectSnapshot, forge, sampleRepo, snapshotOf } from "../helpers";

const T = 120_000;

describe.each([["fastapi-app"], ["nextjs-prisma-app"], ["astro-app"], ["eslint-app"]])("%s", (name) => {
  test(
    "apply output equals the stored snapshot",
    () => {
      const repo = sampleRepo(name);
      const r = forge(["apply", "--yes"], repo);
      expect(r.stderr).toBe("");
      expect(r.status).toBe(0);
      const s = expectSnapshot(name, snapshotOf(repo));
      if (!s.ok) throw new Error(`${name}: output differs from ${s.path} (FORGE_UPDATE_SNAPSHOTS=1 to update)`);
    },
    T,
  );

  test(
    "a second apply changes nothing",
    () => {
      const repo = sampleRepo(name);
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      const before = snapshotOf(repo);
      expect(forge(["apply", "--yes"], repo).stdout).toContain("forge: up to date");
      expect(snapshotOf(repo)).toBe(before);
    },
    T,
  );
});
