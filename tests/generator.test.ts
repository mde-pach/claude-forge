// §14.2: generator snapshots, idempotency, drift, remove, ESLint repository.
import { describe, expect, test } from "bun:test";
import { appendFileSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { expectSnapshot, fakeClaude, forge, git, sampleRepo, sh, snapshotOf } from "./helpers";

const SAMPLES = ["fastapi-app", "nextjs-prisma-app", "eslint-app"];
const T = 120_000;

describe.each(SAMPLES)("%s", (name) => {
  test(
    "apply matches the snapshot; a second apply changes nothing",
    () => {
      const repo = sampleRepo(name);
      const r1 = forge(["apply", "--yes"], repo);
      expect(r1.stderr).toBe("");
      expect(r1.status).toBe(0);
      const snap = snapshotOf(repo);
      const s = expectSnapshot(name, snap);
      if (!s.ok) throw new Error(`${name}: output differs from ${s.path} (FORGE_UPDATE_SNAPSHOTS=1 to update)`);

      const r2 = forge(["apply", "--yes"], repo);
      expect(r2.status).toBe(0);
      expect(r2.stdout).toContain("forge: up to date");
      expect(snapshotOf(repo)).toBe(snap);
      expect(forge(["diff"], repo).status).toBe(0);
    },
    T,
  );

  test(
    "an edited owned file stops apply and fails check.sh on integrity",
    () => {
      const repo = sampleRepo(name);
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      appendFileSync(join(repo, ".forge", "check.sh"), "# edited\n");
      const r = forge(["apply", "--yes"], repo);
      expect(r.status).toBe(1);
      expect(r.stderr).toContain(".forge/check.sh was modified since the last apply");
      expect(forge(["diff"], repo).status).toBe(1);
      const c = sh("sh", [".forge/check.sh", "--changed"], { cwd: repo });
      expect(c.status).toBe(1);
      expect(c.stdout).toContain("forge:forge/integrity .forge/check.sh was modified");
      expect(forge(["apply", "--yes", "--force"], repo).status).toBe(0);
    },
    T,
  );

  test(
    "remove restores the repository (git status clean)",
    () => {
      const repo = sampleRepo(name);
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      sh("sh", ["-c", "mkdir -p .forge/state && echo x > .forge/state/last-pass"], { cwd: repo });
      const r = forge(["remove", "--yes"], repo);
      expect(r.status).toBe(0);
      expect(git(repo, "status", "--porcelain", "--untracked-files=all").stdout).toBe("");
      expect(existsSync(join(repo, ".git", "hooks", "pre-commit"))).toBe(false);
    },
    T,
  );
});

test(
  "ESLint repository: no Biome target, ast-grep fallback, rest reported, no project file modified",
  () => {
    const repo = sampleRepo("eslint-app");
    expect(forge(["apply", "--yes"], repo).status).toBe(0);
    const check = readFileSync(join(repo, ".forge", "check.sh"), "utf8");
    expect(check).not.toContain("biome");
    expect(readFileSync(join(repo, ".claude/skills/forge-project/scripts/post-edit.sh"), "utf8")).not.toContain("biome");
    expect(existsSync(join(repo, ".forge/ast-grep/rules/typescript/no-explicit-any.yml"))).toBe(true);
    expect(existsSync(join(repo, ".forge/ast-grep/rules/typescript/no-ts-ignore.yml"))).toBe(true);
    const status = forge(["status"], repo).stdout;
    expect(status).toContain("not enforced here: biome not configured");
    // only forge's own paths changed
    const changed = git(repo, "status", "--porcelain").stdout.trim().split("\n");
    expect(changed.sort()).toEqual(["?? .claude/", "?? .forge/"]);
    expect(git(repo, "diff", "--stat").stdout).toBe("");
  },
  T,
);

test(
  "apply refuses Claude Code older than 2.1.295",
  () => {
    const repo = sampleRepo("eslint-app");
    const r = forge(["apply", "--yes"], repo, fakeClaude("2.1.200"));
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("too old");
    expect(existsSync(join(repo, ".forge"))).toBe(false);
  },
  T,
);

test(
  "--dry-run writes nothing; --local writes permissions to settings.local.json",
  () => {
    const repo = sampleRepo("fastapi-app");
    const d = forge(["apply", "--dry-run"], repo);
    expect(d.status).toBe(0);
    expect(d.stdout).toContain("+++ .forge/check.sh (new)");
    expect(existsSync(join(repo, ".forge"))).toBe(false);
    expect(forge(["apply", "--local"], repo).status).toBe(0);
    expect(existsSync(join(repo, ".claude/settings.json"))).toBe(false);
    expect(readFileSync(join(repo, ".claude/settings.local.json"), "utf8")).toContain("Edit(.forge/**)");
  },
  T,
);

test(
  "forge.yaml: add, remove with dependents, disable with reason",
  () => {
    const repo = sampleRepo("fastapi-app");
    sh("sh", ["-c", `printf 'forge: "0.5"\\npacks:\\n  add: [zod]\\n  remove: [typescript, sqlalchemy]\\ndisable:\\n  - id: pytest/run\\n    reason: slow suite, CI only\\n' > forge.yaml`], { cwd: repo });
    expect(forge(["apply", "--yes"], repo).status).toBe(0);
    const status = forge(["status"], repo).stdout;
    expect(status).toContain("zod: not active (requires removed pack typescript)");
    expect(status).toContain("sqlalchemy: not active (removed in forge.yaml)");
    expect(status).toContain("pytest/run: slow suite, CI only");
    expect(readFileSync(join(repo, ".forge/check.sh"), "utf8")).not.toContain("pytest -q");
  },
  T,
);
