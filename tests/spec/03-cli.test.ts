/**
 * @spec 3 · Forge CLI
 *
 * `forge` is one bundled Node/Bun script. Writing commands (`init`, `apply`,
 * `remove`) accept `--dry-run`; writes to a shared settings file are shown as a
 * diff and need a `y` (or `--yes`). Exit codes: 0 success, 1 failure or
 * differences, 2 usage error.
 */
import { describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applied, fakeClaude, forge, git, put, ROOT, sampleRepo, sh } from "../helpers";

const T = 120_000;

describe("Claude Code version gate", () => {
  test(
    "apply refuses Claude Code older than 2.1.295 and writes nothing",
    () => {
      const repo = sampleRepo("eslint-app");
      const r = forge(["apply", "--yes"], repo, fakeClaude("2.1.200"));
      expect(r.status).toBe(1);
      expect(r.stderr).toContain("Claude Code 2.1.200 is too old; forge needs claude >= 2.1.295");
      expect(existsSync(join(repo, ".forge"))).toBe(false);
    },
    T,
  );

  test(
    "apply refuses when Claude Code is not installed",
    () => {
      const repo = sampleRepo("eslint-app");
      const bin = mkdtempSync(join(tmpdir(), "forge-noclaude-"));
      symlinkSync(sh("sh", ["-c", "command -v node"]).stdout.trim(), join(bin, "node"));
      const r = forge(["apply", "--yes"], repo, { PATH: `${bin}:/usr/bin:/bin` });
      expect(r.status).toBe(1);
      expect(r.stderr).toContain("Claude Code not found");
      expect(existsSync(join(repo, ".forge"))).toBe(false);
    },
    T,
  );
});

describe("apply", () => {
  test(
    "apply is idempotent: a second run changes nothing and diff exits 0",
    () => {
      const repo = applied("fastapi-app");
      const r = forge(["apply", "--yes"], repo);
      expect(r.stdout).toContain("forge: up to date");
      expect(forge(["diff"], repo).status).toBe(0);
    },
    T,
  );

  test(
    "--dry-run prints the diff and writes nothing",
    () => {
      const repo = sampleRepo("fastapi-app");
      const r = forge(["apply", "--dry-run"], repo);
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("+++ .forge/check.sh (new)");
      expect(r.stdout).toContain("+++ .claude/settings.json");
      expect(existsSync(join(repo, ".forge"))).toBe(false);
      expect(existsSync(join(repo, ".claude"))).toBe(false);
    },
    T,
  );

  test(
    "--local writes the permission entries to .claude/settings.local.json, without confirmation",
    () => {
      const repo = sampleRepo("fastapi-app");
      expect(forge(["apply", "--local"], repo).status).toBe(0);
      expect(existsSync(join(repo, ".claude/settings.json"))).toBe(false);
      expect(readFileSync(join(repo, ".claude/settings.local.json"), "utf8")).toContain("Edit(.forge/**)");
    },
    T,
  );

  test(
    "a change to a shared settings file needs --yes when there is no terminal",
    () => {
      const repo = sampleRepo("eslint-app");
      const r = forge(["apply"], repo);
      expect(r.status).toBe(1);
      expect(r.stderr).toContain("Re-run with --yes to confirm non-interactively");
      expect(existsSync(join(repo, ".claude/settings.json"))).toBe(false);
    },
    T,
  );

  test("apply outside a git repository fails", () => {
    const r = forge(["apply", "--yes"], mkdtempSync(join(tmpdir(), "forge-nogit-")));
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("not inside a git repository");
  });
});

describe("init", () => {
  test(
    "init creates the stack's tool configs that are missing, then applies",
    () => {
      const repo = mkdtempSync(join(tmpdir(), "forge-init-"));
      git(repo, "init", "-q");
      put(repo, "pyproject.toml", '[project]\nname = "x"\nversion = "0"\n');
      put(repo, "ruff.toml", "line-length = 80\n");
      const r = forge(["init", "--yes"], repo);
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("forge: created pyrightconfig.json (from python)");
      expect(r.stdout).toContain("forge: ruff.toml exists, kept");
      expect(readFileSync(join(repo, "ruff.toml"), "utf8")).toBe("line-length = 80\n");
      expect(existsSync(join(repo, ".forge/check.sh"))).toBe(true);
    },
    T,
  );

  test(
    "init refuses a project that already has forge",
    () => {
      const r = forge(["init", "--yes"], applied("eslint-app"));
      expect(r.status).toBe(1);
      expect(r.stderr).toContain("init is for new projects");
    },
    T,
  );
});

describe("status, explain, diff", () => {
  test(
    "status lists packs with the reason each is active, rules with where they are enforced, and the CI step",
    () => {
      const out = forge(["status"], applied("nextjs-prisma-app")).stdout;
      expect(out).toContain("  typescript/nextjs (detected)");
      expect(out).toContain("  typescript/nextjs/react (detected)");
      expect(out).toContain("  typescript/nextjs/react/client-hooks\n    ast-grep forbid (.forge/ast-grep/rules/typescript/nextjs/react/client-hooks.yml)");
      expect(out).toContain("CI step, pull requests: sh .forge/check.sh --since origin/main");
      expect(out).toContain("CI step, the default branch: sh .forge/check.sh --all");
    },
    T,
  );

  test(
    "explain prints the rule file and where it is enforced in this project",
    () => {
      const out = forge(["explain", "python/sqlalchemy/no-query-api"], applied("fastapi-app")).stdout;
      expect(out).toContain("packs/python/sqlalchemy/rules/no-query-api.yaml");
      expect(out).toContain('rule: {pattern: "$S.query($$$)"}');
      expect(out).toContain("Enforced here:\n  ast-grep forbid");
    },
    T,
  );

  test(
    "diff exits 1 when apply would change something",
    () => {
      const r = forge(["diff"], sampleRepo("eslint-app"));
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("+++ .forge/check.sh (new)");
    },
    T,
  );
});

describe("usage errors exit 2", () => {
  test.each([[["frobnicate"]], [["apply", "--nope"]], [["apply", "--user", "--local"]], [["explain"]]])("forge %p", (args) => {
    expect(forge(args, ROOT).status).toBe(2);
  });
});
