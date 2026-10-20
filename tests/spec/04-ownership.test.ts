/**
 * @spec 4 · Ownership and the lock
 *
 * Forge regenerates the files it **owns** (`.forge/**`, `.claude/skills/forge-*`,
 * `.claude/rules/forge-*.md`), **appends** to settings files with consent and
 * records each entry it adds, **audits** tool configs, and leaves everything
 * else untouched. `.forge/forge.lock.json` holds the hashes of owned files and
 * rule files and the added settings entries; it is how drift is detected and
 * how `remove` knows exactly what to delete (§4, §12).
 */
import { describe, expect, test } from "bun:test";
import { appendFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addEntries, removeEntries } from "../../src/settings";
import { applied, commitAll, ensureTools, forge, git, hasBun, linkTools, put, sampleRepo, sh } from "../helpers";

const T = 120_000;

describe("owned files", () => {
  test(
    "an owned file edited since the last apply stops apply with its diff; --force overwrites it",
    () => {
      const repo = applied("fastapi-app");
      appendFileSync(join(repo, ".forge/check.sh"), "# edited\n");
      const r = forge(["apply", "--yes"], repo);
      expect(r.status).toBe(1);
      expect(r.stderr).toContain(".forge/check.sh was modified since the last apply");
      expect(r.stdout).toContain("-# edited");
      expect(forge(["apply", "--yes", "--force"], repo).status).toBe(0);
      expect(readFileSync(join(repo, ".forge/check.sh"), "utf8")).not.toContain("# edited");
    },
    T,
  );

  test(
    "a file Forge does not own at an owned path stops apply; --force replaces it",
    () => {
      const repo = sampleRepo("eslint-app");
      put(repo, ".forge/check.sh", "#!/bin/sh\necho mine\n");
      const r = forge(["apply", "--yes"], repo);
      expect(r.status).toBe(1);
      expect(r.stderr).toContain(".forge/check.sh exists and is not a forge file");
      expect(readFileSync(join(repo, ".forge/check.sh"), "utf8")).toBe("#!/bin/sh\necho mine\n");
      expect(forge(["apply", "--yes", "--force"], repo).status).toBe(0);
    },
    T,
  );

  test(
    "a deleted owned file is recreated by apply without --force",
    () => {
      const repo = applied("eslint-app");
      rmSync(join(repo, ".claude/skills/forge-project/scripts/stop.sh"));
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      expect(existsSync(join(repo, ".claude/skills/forge-project/scripts/stop.sh"))).toBe(true);
    },
    T,
  );

  test(
    "files an older apply generated and the current one does not are deleted",
    () => {
      const repo = applied("fastapi-app");
      put(repo, "forge.yaml", 'forge: "0.5"\ndisable:\n  - {id: python/sqlalchemy/no-query-api, reason: legacy code base}\n');
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      expect(existsSync(join(repo, ".forge/ast-grep/rules/python/sqlalchemy/no-query-api.yml"))).toBe(false);
    },
    T,
  );
});

describe("the lock", () => {
  test(
    "check.sh reads owned-file hashes from .forge/manifest.sha256, in sha256sum format; the lock is for the CLI only",
    () => {
      const r = applied("eslint-app");
      const manifest = readFileSync(join(r, ".forge/manifest.sha256"), "utf8");
      expect(manifest).toMatch(/^[0-9a-f]{64} {2}\.claude\/rules\/forge-typescript\.md\n/);
      expect(manifest).not.toContain("forge.lock.json");
      expect(manifest).not.toContain("manifest.sha256");
      // non-JSON entries verify with the standard tool; JSON entries hash without whitespace
      const plain = manifest.split("\n").filter((l) => l && !l.endsWith(".json")).join("\n") + "\n";
      const c = sh("sh", ["-c", 'command -v sha256sum >/dev/null && sha256sum -c --quiet || shasum -a 256 -c --quiet'], { cwd: r, input: plain });
      expect(c.status).toBe(0);
      expect(readFileSync(join(r, ".forge/check.sh"), "utf8")).not.toMatch(/awk[^\n]*forge\.lock\.json/);
    },
    T,
  );

  test(
    "diff and status report a rule whose source changed since the last apply",
    () => {
      const repo = applied("eslint-app");
      const lockFile = join(repo, ".forge/forge.lock.json");
      const lock = JSON.parse(readFileSync(lockFile, "utf8"));
      lock.rules["typescript/strict"] = "0".repeat(64);
      writeFileSync(lockFile, JSON.stringify(lock, null, 2) + "\n");
      expect(forge(["diff"], repo).stdout).toContain("rule typescript/strict changed since the last apply");
      expect(forge(["status"], repo).stdout).toContain("rule typescript/strict changed since the last apply");
    },
    T,
  );
});

describe("appended settings", () => {
  test(
    "a settings entry removed by hand is reported by diff and status, and re-added by apply",
    () => {
      const repo = applied("eslint-app");
      const f = join(repo, ".claude/settings.json");
      writeFileSync(f, readFileSync(f, "utf8").replace('"Edit(forge.yaml)",', ""));
      const d = forge(["diff"], repo);
      expect(d.status).toBe(1);
      expect(d.stdout).toContain('entry missing, apply re-adds it: permissions.ask "Edit(forge.yaml)"');
      expect(forge(["status"], repo).stdout).toContain('missing permissions.ask "Edit(forge.yaml)"');
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      expect(readFileSync(f, "utf8")).toContain("Edit(forge.yaml)");
    },
    T,
  );

  test(
    "entries already in the settings file are kept, never recorded, and survive remove",
    () => {
      const repo = sampleRepo("eslint-app");
      put(repo, ".claude/settings.json", '{\n  // team settings\n  "permissions": {"ask": ["Edit(.forge/**)"], "allow": ["Bash(ls:*)"]}\n}\n');
      commitAll(repo);
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      const lock = JSON.parse(readFileSync(join(repo, ".forge/forge.lock.json"), "utf8"));
      expect(lock.settings.map((e: { value: string }) => e.value)).not.toContain("Edit(.forge/**)");
      expect(forge(["remove", "--yes"], repo).status).toBe(0);
      const back = readFileSync(join(repo, ".claude/settings.json"), "utf8");
      expect(back).toContain("// team settings");
      expect(back).toContain("Edit(.forge/**)");
      expect(back).not.toContain("Edit(forge.yaml)");
    },
    T,
  );

  test("JSONC comments are preserved when entries are appended and removed", () => {
    const base = '{\n  // mine\n  "permissions": {\n    "deny": ["Read(x)"] // keep\n  }\n}\n';
    const { text, added } = addEntries(base, [
      { file: "s", path: "permissions.deny", value: "Read(x)", append: true },
      { file: "s", path: "permissions.ask", value: "Edit(.forge/**)", append: true },
      { file: "s", path: "sandbox.enabled", value: true, append: false },
    ]);
    expect(text).toContain("// mine");
    expect(text).toContain("// keep");
    expect(added.map((e) => e.path)).toEqual(["permissions.ask", "sandbox.enabled"]);
    const back = removeEntries(text, added)!;
    expect(back).toContain("// mine");
    expect(JSON.parse(back.replace(/\/\/.*$/gm, ""))).toEqual({ permissions: { deny: ["Read(x)"] } });
  });

  test("a scalar the user already set is never overwritten", () => {
    expect(addEntries('{"sandbox": {"enabled": false}}', [{ file: "s", path: "sandbox.enabled", value: true, append: false }]).added).toEqual([]);
  });

  test("a settings file emptied by remove is deleted", () => {
    const { text, added } = addEntries("", [{ file: "s", path: "permissions.ask", value: "Edit(a)", append: true }]);
    expect(removeEntries(text, added)).toBeUndefined();
  });

  test.each([
    ["not JSON", "{ not json", /\.claude\/settings\.json.*not valid JSON/],
    ["permissions.ask not an array", '{"permissions": {"ask": "Edit(x)"}}', /not an array/],
  ])(
    "a settings file that is %s stops apply and names the file",
    (_kind, text, message) => {
      const repo = sampleRepo("eslint-app");
      put(repo, ".claude/settings.json", text);
      const r = forge(["apply", "--yes"], repo);
      expect(r.status).toBe(1);
      expect(r.stderr).toMatch(message);
      expect(existsSync(join(repo, ".forge"))).toBe(false);
    },
    T,
  );
});

describe("remove", () => {
  test.each([["fastapi-app"], ["nextjs-prisma-app"], ["astro-app"], ["eslint-app"]])(
    "remove restores %s to its state before apply (git status clean)",
    (name) => {
      const repo = applied(name);
      mkdirSync(join(repo, ".forge/state"), { recursive: true });
      writeFileSync(join(repo, ".forge/state/last-pass"), "x");
      expect(forge(["remove", "--yes"], repo).status).toBe(0);
      expect(git(repo, "status", "--porcelain", "--untracked-files=all").stdout).toBe("");
      expect(existsSync(join(repo, ".git/hooks/pre-commit"))).toBe(false);
    },
    T,
  );

  test(
    "remove --dry-run lists what it would delete and deletes nothing",
    () => {
      const repo = applied("eslint-app");
      const r = forge(["remove", "--dry-run"], repo);
      expect(r.stdout).toContain("--- .forge/check.sh (deleted)");
      expect(existsSync(join(repo, ".forge/check.sh"))).toBe(true);
      expect(sh("sh", [".forge/check.sh", "--changed"], { cwd: repo }).status).toBe(0);
    },
    T,
  );
});

/**
 * Forge's JSON files are owned, but projects format JSON too. Owned JSON is
 * hashed with spaces, tabs and newlines removed (by the CLI and by check.sh),
 * so a formatter run is not tampering, while any other change still is; and on
 * apply Forge formats the JSON it generates with the project's own Biome.
 */
describe("formatters and Forge's JSON", () => {
  const forgeLines = (out: string) => out.split("\n").filter((l) => /(^|\s|\.\/)\.(forge|claude)\//.test(l));
  const DEFAULT_BIOME = '{"linter":{"enabled":true,"rules":{"recommended":true}}}\n';

  test.if(hasBun)(
    "with the project's Biome, a fresh apply formats Forge's JSON so that biome ci reports nothing on Forge's files",
    () => {
      ensureTools();
      const r = sampleRepo("nextjs-prisma-app");
      put(r, "biome.json", DEFAULT_BIOME);
      linkTools(r, "node");
      commitAll(r, "biome");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      const ci = sh("node_modules/.bin/biome", ["ci", "."], { cwd: r });
      expect(forgeLines(ci.stdout + ci.stderr)).toEqual([]);
      expect(readFileSync(join(r, ".claude/skills/forge-project/hooks/hooks.json"), "utf8")).toStartWith('{\n\t"description"');
    },
    T,
  );

  test.if(hasBun)(
    "reformatting Forge's JSON with the project's formatter keeps integrity, and apply then rewrites nothing",
    () => {
      ensureTools();
      const r = sampleRepo("nextjs-prisma-app");
      put(r, "biome.json", DEFAULT_BIOME);
      commitAll(r, "biome");
      expect(forge(["apply", "--yes"], r).status).toBe(0); // no Biome installed yet: two-space JSON
      commitAll(r, "forge");
      linkTools(r, "node");
      sh("node_modules/.bin/biome", ["check", "--write", "."], { cwd: r });
      expect(readFileSync(join(r, ".forge/forge.lock.json"), "utf8")).toStartWith('{\n\t"forge_version"');
      const c = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(c.stdout).not.toContain("forge/integrity");
      const again = forge(["apply", "--yes"], r);
      expect(again.stdout).toContain("forge: up to date");
      expect(forge(["diff"], r).status).toBe(0);
    },
    T,
  );

  test(
    "a real change to a Forge JSON file still fails integrity, naming the file",
    () => {
      const r = applied("eslint-app");
      const f = join(r, ".claude/skills/forge-project/hooks/hooks.json");
      writeFileSync(f, readFileSync(f, "utf8").replace('"timeout": 5,', '"timeout": 50,'));
      const c = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(c.status).toBe(1);
      expect(c.stdout).toContain("forge:forge/integrity .claude/skills/forge-project/hooks/hooks.json was modified (run forge diff)");
    },
    T,
  );

  test(
    "appending to an existing settings file keeps its indentation",
    () => {
      const r = sampleRepo("eslint-app");
      put(r, ".claude/settings.json", '{\n\t"permissions": {\n\t\t"allow": ["Bash(ls:*)"]\n\t}\n}\n');
      commitAll(r, "settings");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      const text = readFileSync(join(r, ".claude/settings.json"), "utf8");
      expect(text).toContain('\n\t\t"ask": [\n\t\t\t"Edit(.forge/**)"');
      expect(text).not.toMatch(/\n {2}"/);
    },
    T,
  );
});

