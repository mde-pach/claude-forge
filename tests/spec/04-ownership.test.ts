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
import { applied, commitAll, forge, git, put, sampleRepo, sh } from "../helpers";

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
    "the lock is pretty-printed with one path per line, as check.sh reads it with awk",
    () => {
      const text = readFileSync(join(applied("eslint-app"), ".forge/forge.lock.json"), "utf8");
      expect(text).toMatch(/^\{\n {2}"forge_version": "0\.5\.0",\n {2}"claude_code_min": "2\.1\.295",\n {2}"rules": \{/);
      expect(text).toMatch(/\n {2}"files": \{\n {4}"\.claude\/rules\/forge-typescript\.md": "[0-9a-f]{64}",\n/);
      expect(text).not.toContain('"forge.lock.json"');
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
