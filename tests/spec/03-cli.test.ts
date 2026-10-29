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
import { applied, commitAll, ensureTools, fakeClaude, forge, git, hasUv, linkTools, put, ROOT, sampleRepo, sh, stopEngines } from "../helpers";

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
      expect(r.stdout).toContain("forge: created ty.toml (from python)");
      expect(r.stdout).toContain("forge: ruff.toml exists, kept");
      expect(readFileSync(join(repo, "ruff.toml"), "utf8")).toBe("line-length = 80\n");
      expect(existsSync(join(repo, ".forge/check.sh"))).toBe(true);
    },
    T,
  );

  /**
   * A pack can switch a tool diagnostic off where it is active (pack.yaml `tool_off`): NestJS
   * needs DTOs imported as values, so Biome's useImportType (whose fix turns them into
   * `import type`) is off in the biome.json init writes, no rule asks for it, and status
   * says which pack switched it off and why.
   * @rule typescript/nestjs/dto-not-type-import typescript/verbatim-imports
   */
  test(
    "a pack's tool_off: init writes Biome's diagnostic off, no rule's audit asks for it, and status lists it with the pack and the reason",
    () => {
      const repo = mkdtempSync(join(tmpdir(), "forge-init-"));
      git(repo, "init", "-q");
      put(repo, "package.json", JSON.stringify({ name: "api", private: true, dependencies: { "@nestjs/common": "11.1.6", "@nestjs/core": "11.1.6" }, devDependencies: { typescript: "6.0.3" } }));
      expect(forge(["init", "--yes"], repo).status).toBe(0);
      const biome = JSON.parse(readFileSync(join(repo, "biome.json"), "utf8")) as { linter: { rules: { style: Record<string, string> } } };
      expect(biome.linter.rules.style.useImportType).toBe("off");
      expect(biome.linter.rules.style.useExportType).toBe("error");
      const status = forge(["status"], repo).stdout;
      expect(status).toContain("not enforced here: biome lint/style/useImportType switched off by typescript/nestjs (a DTO imported with import type erases design:paramtypes");
      expect(status).toContain("Switched off by packs:\n  biome lint/style/useImportType (typescript/nestjs): a DTO imported with import type erases design:paramtypes");
      expect(status).toContain("not asked for by typescript/verbatim-imports");
      const json = JSON.parse(forge(["status", "--json"], repo).stdout) as { switched_off: { pack: string; expects: string; rules: string[] }[] };
      expect(json.switched_off).toEqual([expect.objectContaining({ pack: "typescript/nestjs", tool: "biome", expects: "lint/style/useImportType", rules: ["typescript/verbatim-imports"] })]);
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

  test.if(hasUv)(
    "status counts active rules apart from enforced ones, with the reasons the others are not enforced",
    () => {
      ensureTools();
      const r = sampleRepo("fastapi-app");
      linkTools(r, "python");
      commitAll(r, "tools");
      const out = forge(["status"], r).stdout;
      const m = /^Rules: (\d+) active, (\d+) enforced, (\d+) not enforced \((.*)\)$/m.exec(out);
      expect(m).not.toBeNull();
      expect(Number(m![2]) + Number(m![3])).toBe(Number(m![1]));
      // the sample's ruff config selects E, F, ANN401 and E722 only
      expect(m![4]).toMatch(/\d+ ruff codes not selected/);
    },
    T,
  );

  test(
    "status --json lists every active rule with whether it is enforced, by what, and why not",
    () => {
      const r = forge(["status", "--json"], applied("fastapi-app"));
      expect(r.status).toBe(0);
      const j = JSON.parse(r.stdout) as { counts: { active: number; enforced: number; not_enforced: number }; rules: { id: string; enforced: boolean; by: string[]; not_enforced: string[] }[] };
      expect(j.rules.length).toBe(j.counts.active);
      expect(j.rules.filter((x) => x.enforced).length).toBe(j.counts.enforced);
      expect(j.rules.find((x) => x.id === "python/sqlalchemy/no-query-api")).toMatchObject({ enforced: true, by: ["ast-grep forbid"] });
      const lint = j.rules.find((x) => x.id === "python/lint")!;
      if (!lint.enforced) expect(lint.not_enforced).toContain("ruff not installed");
    },
    T,
  );

  test(
    "status and apply report the same missing tools (one source: the scripts' forge_missing_tools); the SessionStart hook does not, they are the person's to fix",
    () => {
      const r = applied("fastapi-app");
      const fromStatus = (JSON.parse(forge(["status", "--json"], r).stdout) as { missing_tools: string[] }).missing_tools;
      const line = /forge: missing tools \(checks using them are skipped\): (.*)$/m.exec(forge(["apply", "--yes"], r).stdout);
      expect(line ? line[1]!.split(", ") : []).toEqual(fromStatus);
      const hook = sh("sh", [".claude/skills/forge-project/scripts/session-start.sh"], { cwd: r, input: JSON.stringify({ session_id: "s", cwd: r }) }).stdout;
      stopEngines(r); // SessionStart started the types engine (§15.3)
      expect(hook).not.toContain("missing tools");
      expect(hook).not.toContain("tool versions");
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
