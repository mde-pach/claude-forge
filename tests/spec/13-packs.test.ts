/**
 * @spec 13 · Packs and rules (bun run test:packs)
 *
 * Every pack and rule validates against its schema, and every rule's examples
 * behave as declared: ast-grep rule tests pass (`match` is invalid code,
 * `no_match` valid), guard examples get the rule's decision on every shell,
 * permission examples match the rule's permission strings, `grep` patterns
 * match every `match` example and no `no_match` one, and `check`
 * examples fail (`match`) or pass (`no_match`) the generated `check.sh` in a
 * temporary repository. There is one test per rule below; the pack pages of
 * this site show the same results.
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadPackBundle, loadPacks } from "../../src/packs";
import { checkExamplesOptional, hadolintIntegrity, permissionMatches, runCheckExample, runCheckExamples, selfTest, type TestReport } from "../../src/selftest";
import { ANGULAR_TOOLS, ROOT, SYSTEM_TOOLS_BIN, ensureNodeTools, fullEngine, hasBun, hasUv, packsSelfTest, sh, shScript, STUB_VERSION } from "../helpers";
import type { CheckTarget } from "../../src/types";
import { commandsOf, SYSTEM_TOOLS } from "../../src/core/tools";
import { renderCheckScript } from "../../src/render";
import { RELEASE_BINARIES } from "../../src/util";
import { loadModels, modelRuleFiles, renderRuleFile } from "../../scripts/models";

const T = 300_000;
const packs = loadPacks(join(ROOT, "packs"));
const rules = [...packs.values()].flatMap((p) => p.rules);
let report: TestReport;

/** The self-test of every pack, run once per test run (tests/helpers.ts packsSelfTest). */
beforeAll(() => {
  report = packsSelfTest();
}, T);

/** Failures of `forge test` that concern one rule. */
const failuresOf = (id: string) => report.failures.filter((f) => f.startsWith(`${id}:`) || f.startsWith(`${id} [`) || (f.startsWith("ast-grep") && f.includes(` ${id} `)));

describe("every rule", () => {
  test.each(rules.map((r) => [r.id]))("%s: examples behave as declared", (id) => {
    expect(failuresOf(id)).toEqual([]);
  });
});

/**
 * A project gets only the engine modules its checks and hook scripts use (§10.1): a check run
 * on its own must still find every engine function its commands call, and its check.sh must
 * parse.
 */
test("every check target, rendered on its own, has every engine function it calls, and its check.sh parses", () => {
  const dir = mkdtempSync(join(tmpdir(), "forge-engine-of-"));
  const problems: string[] = [];
  try {
    for (const rule of rules)
      for (const target of rule.enforce.filter((t): t is CheckTarget => t.target === "check")) {
        const script = renderCheckScript([{ rule, target }], { forbid: "", noIncrease: "", noDecrease: "" });
        for (const name of commandsOf(target)
          .join("\n")
          .match(/\bforge_[a-z0-9_]+\b/g) ?? [])
          if (!new RegExp(`^${name}\\(\\)`, "m").test(script)) problems.push(`${rule.id}: ${name} is not defined in its check.sh`);
        writeFileSync(join(dir, "check.sh"), script);
        const parsed = sh("sh", ["-n", join(dir, "check.sh")]);
        if (parsed.status !== 0) problems.push(`${rule.id}: sh -n: ${parsed.stderr.trim()}`);
      }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  expect(problems).toEqual([]);
});

test("every check target has examples, except test and migration runs, astro check, ngc and the lock-file checks", () => {
  const missing = rules.filter((r) => r.enforce.some((t) => t.target === "check" && !checkExamplesOptional(t as CheckTarget)) && !(r.examples?.match?.length && r.examples?.no_match?.length));
  expect(missing.map((r) => r.id)).toEqual([]);
});

/** @pack docker */
test(
  "the hadolint-bin wheel ships the upstream hadolint release binary (its sha256 matches the release's)",
  () => {
    expect(hadolintIntegrity().status).toBe("ok");
  },
  T,
);

/**
 * Each pinned release binary (§13.5) for this platform is fetched as check.sh fetches it
 * (into ~/.cache/forge/bin, once per machine) and matches the sha256 Forge embeds.
 */
test("every pinned release binary for this platform downloads and matches its embedded sha256", () => {
  const engine = fullEngine();
  const names = Object.keys(RELEASE_BINARIES);
  const r = shScript(`${engine}\nfor n in ${names.join(" ")}; do b=$(forge_release_bin "$n") && [ -x "$b" ] && echo "$n ok" || echo "$n: $b"; done`);
  expect(r.stdout).toBe(names.map((n) => `${n} ok\n`).join(""));
}, 600_000);

/**
 * Only a system tool (§13.5) this machine does not have (docker on CI's macOS runners, a
 * package manager the project does not use) may skip an example; the engine decides, as
 * check.sh does (forge_has_tool: Yarn 1 is not the yarn a .yarnrc.yml project needs).
 */
test("every check example ran: none was skipped, except for a system tool that is not installed here", () => {
  const engine = fullEngine();
  const absent = (tool: string) => shScript(`${engine}\nPATH=${SYSTEM_TOOLS_BIN}:$PATH forge_has_tool ${tool}`).status !== 0;
  const skipped = report.notes.filter((n) => n.includes(" example ") && n.includes("skipped"));
  const system = (n: string) => SYSTEM_TOOLS.find((t) => new RegExp(`skipped: ${t}( 2 or later)? (not available|not installed)`).test(n));
  expect(skipped.filter((n) => !(system(n) && absent(system(n)!)))).toEqual([]);
});

/**
 * The ai-models data file (packs/ai-models/models.tsv: each model id with its status,
 * retirement date, replacement and the request features it rejects) is the one list of
 * model ids: a rule line after a "# models.tsv: <kind>" comment holds its rendering
 * (`bun scripts/models.ts` writes it), so the rules that name models never disagree.
 */
test("every rule that renders the ai-models data file is current with it", () => {
  const rows = loadModels();
  const files = modelRuleFiles(join(ROOT, "packs"));
  expect(files.length).toBeGreaterThan(3);
  for (const f of files) {
    const text = readFileSync(f, "utf8");
    expect({ file: f, text: renderRuleFile(text, rows) }).toEqual({ file: f, text });
  }
});

describe("every pack", () => {
  test.each([...packs.values()].filter((p) => p.context !== undefined).map((p) => [p.id]))("%s: context.md is at most 10 lines", (id) => {
    expect(packs.get(id)!.context!.replace(/\n$/, "").split("\n").length).toBeLessThanOrEqual(10);
  });
});

test("forge test passes on the shipped packs and counts every rule file", () => {
  expect(report.failures).toEqual([]);
  expect(report.counts.rules).toBe(rules.length);
});

/** The CLI reads the shipped packs from the build's bundle; it holds what loadPacks reads from packs/, fields and their order included. */
test("dist/packs.json, which forge reads, is the packs directory as loaded and validated from its files", () => {
  const bundled = loadPackBundle(join(ROOT, "dist", "packs.json"), join(ROOT, "packs"));
  expect(bundled).toStrictEqual(packs);
  expect(JSON.stringify([...bundled])).toBe(JSON.stringify([...packs]));
});

/** Pack authors run the self-test with `bun run test:packs` (scripts/test-packs.ts); the published CLI has no such command. */
test(
  "bun run test:packs runs the self-test on a packs directory, summarises it and exits 0, or 1 on a failure",
  () => {
    const dir = mkdtempSync(join(tmpdir(), "forge-packs-"));
    const pack = (rule: string) => {
      mkdirSync(join(dir, "demo/rules"), { recursive: true });
      writeFileSync(join(dir, "demo/pack.yaml"), "id: demo\ndescription: Demo.\nlevel: project\n");
      writeFileSync(
        join(dir, "demo/rules/g.yaml"),
        `id: demo/g\ndescription: G.\nscope: {language: any}\nenforce: [{target: guard, decision: deny, pattern: "rm[[:space:]]+-rf"}]\nexamples: {match: [${JSON.stringify(rule)}], no_match: ["rm x"]}\n`,
      );
    };
    pack("rm -rf x");
    const ok = sh("bun", ["run", "test:packs", dir], { cwd: ROOT });
    expect(ok.status).toBe(0);
    expect(ok.stdout).toContain("test:packs: 1 packs, 1 rules; ast-grep 0, guard 1");
    expect(ok.stdout).toContain("all passed");
    pack("rm -r x");
    const bad = sh("bun", ["run", "test:packs", dir], { cwd: ROOT });
    expect(bad.status).toBe(1);
    expect(bad.stderr).toContain('forge:test demo/g [sh]: expected deny for "rm -r x", got nothing');
  },
  T,
);

/**
 * `prisma db push` projects (no `prisma/migrations`, a `db push` script) have no
 * migration history to compare the schema with. Here a stand-in `prisma` logs its
 * arguments and reports a difference as `migrate diff --exit-code` does (exit 2; exit 1 is
 * a diff that could not run, reported skipped): the db push workflow must not call it, the
 * migrate workflow must.
 */
/** @rule typescript/prisma/migration-present */
test("prisma migration-present passes in a db push workflow and still runs migrate diff in a migrate workflow", () => {
  const rule = rules.find((r) => r.id === "typescript/prisma/migration-present")!;
  const bin = mkdtempSync(join(tmpdir(), "forge-prisma-stub-"));
  try {
    const log = join(bin, "calls");
    writeFileSync(join(bin, "prisma"), `#!/bin/sh\n${STUB_VERSION}printf '%s\\n' "$*" >> "${log}"\necho '[+] Added column \`phone\`'\nexit 2\n`);
    const calls = () => (existsSync(log) ? readFileSync(log, "utf8") : "");
    chmodSync(join(bin, "prisma"), 0o755);
    const target = rule.enforce[0] as CheckTarget;
    const schema = { "prisma/schema.prisma": "model User {\n  id Int @id\n}\n" };
    const changed = { files: { "prisma/schema.prisma": "model User {\n  id    Int    @id\n  phone String?\n}\n" } };
    const env = { ".env": "DATABASE_URL=postgresql://app@localhost:5432/app\n" };
    const push = { head: { ...schema, ...env, "package.json": '{"scripts": {"db:push": "prisma db push"}}\n' }, ...changed };
    const migrate = {
      head: {
        ...schema,
        ...env,
        "package.json": '{"scripts": {"migrate": "prisma migrate dev"}}\n',
        "prisma.config.ts": "export default { datasource: { shadowDatabaseUrl: env('SHADOW_DATABASE_URL') } };\n",
        "prisma/migrations/20261020000000_init/migration.sql": 'CREATE TABLE "User" ("id" INTEGER NOT NULL);\n',
      },
      ...changed,
    };
    // both examples in one call: they run in parallel
    const [p, m] = runCheckExamples(
      [
        { rule, target, example: push, kind: "no_match" },
        { rule, target, example: migrate, kind: "match" },
      ],
      [bin],
    );
    expect(p).toEqual({ outcome: "pass", detail: "" });
    expect(m).toEqual({ outcome: "pass", detail: "" });
    // only the migrate workflow called prisma
    expect(calls()).toBe("migrate diff --from-migrations prisma/migrations --to-schema prisma/schema.prisma --exit-code\n");
  } finally {
    rmSync(bin, { recursive: true, force: true });
  }
});

describe("forge test catches broken packs", () => {
  function packsWith(files: Record<string, string>): string {
    const dir = mkdtempSync(join(tmpdir(), "forge-packs-"));
    for (const [rel, text] of Object.entries({ "demo/pack.yaml": "id: demo\ndescription: Demo.\nlevel: project\n", ...files })) {
      mkdirSync(join(dir, rel, ".."), { recursive: true });
      writeFileSync(join(dir, rel), text);
    }
    return dir;
  }
  const guardRule = (match: string, noMatch: string) =>
    `id: demo/g\ndescription: Demo guard.\nscope: {language: any}\nenforce: [{target: guard, decision: deny, pattern: "rm[[:space:]]+-rf"}]\nexamples: {match: [${JSON.stringify(match)}], no_match: [${JSON.stringify(noMatch)}]}\n`;

  /** Fields that only mean something with certain others are rejected elsewhere, rather than silently doing nothing. */
  test.each([
    ["rerun outside a test check", "enforce: [{target: check, stage: lint, run: 'ruff check .', rerun: 'pytest {files}'}]", "/enforce/0/stage must be equal to constant"],
    [
      "format_stdin outside a format or lint check",
      "enforce: [{target: check, stage: typecheck, run: 'tsc', format_stdin: 'biome format --stdin-file-path={file}'}]",
      "/enforce/0/stage must be equal to one of the allowed values",
    ],
    ["review without a guard ask or the test-edit check", "enforce: [{target: guard, decision: deny, pattern: x}]\nreview: Is this fine?", "/enforce must contain at least 1 valid item"],
    ["the ephemeral-exec guard check with decision deny", "enforce: [{target: guard, decision: deny, pattern: npx, check: ephemeral-exec}]", "/enforce/0/decision must be equal to constant"],
    ["a project tool with a '-' run directly, not through its adapter", "enforce: [{target: check, stage: typecheck, run: 'vue-tsc --noEmit'}]", 'run vue-tsc as "forge_do vue-tsc <operation>"'],
  ])("a rule with %s is rejected", (_what, body, message) => {
    const dir = packsWith({ "demo/rules/x.yaml": `id: demo/x\ndescription: X.\nscope: {language: any}\n${body}\n` });
    expect(() => loadPacks(dir)).toThrow(message);
  });

  /** A typed rule's examples are written inside its scope, so a rule about test files is checked like any other. */
  test.if(hasUv)(
    "a rule with types and a scope of test files passes on its examples",
    () => {
      const rule = [
        "id: demo/q",
        "description: No Session.query in tests.",
        'scope: {language: python, files: ["**/tests/**"]}',
        "enforce:",
        "  - target: ast-grep",
        "    mode: forbid",
        '    rule: {pattern: "$S.query($$$)"}',
        "    where:",
        "      S: {type: {package: sqlalchemy, name: Session}}",
        "examples:",
        '  deps: [{ecosystem: pypi, name: sqlalchemy, version: "2.0.44"}]',
        `  match: [${JSON.stringify("from sqlalchemy.orm import Session\n\n\ndef f(db: Session) -> None:\n    db.query(int)\n")}]`,
        `  no_match: [${JSON.stringify("class Session:\n    def query(self, q: int) -> None: ...\n\n\ndef f(db: Session) -> None:\n    db.query(1)\n")}]`,
        "",
      ].join("\n");
      expect(selfTest(packsWith({ "demo/rules/q.yaml": rule })).failures).toEqual([]);
    },
    T,
  );

  test("a correct pack passes", () => {
    expect(selfTest(packsWith({ "demo/rules/g.yaml": guardRule("rm -rf x", "rm x") })).failures).toEqual([]);
  });
  test("a guard match example that does not trigger fails, per shell", () => {
    const f = selfTest(packsWith({ "demo/rules/g.yaml": guardRule("rm -r x", "rm x") })).failures;
    expect(f).toContain('demo/g [sh]: expected deny for "rm -r x", got nothing');
  });
  test("a guard no_match example that triggers fails", () => {
    expect(selfTest(packsWith({ "demo/rules/g.yaml": guardRule("rm -rf x", "rm -rf y") })).failures.some((x) => x.includes('expected nothing for "rm -rf y"'))).toBe(true);
  });
  test("a permission example that no permission string matches fails", () => {
    const rule =
      'id: demo/p\ndescription: P.\nscope: {language: any}\nenforce: [{target: permission, decision: deny, rule: "Bash(git clean:*)"}]\nexamples: {match: ["Bash(git cleanup)"], no_match: ["Bash(ls)"]}\n';
    expect(selfTest(packsWith({ "demo/rules/p.yaml": rule })).failures).toContain("demo/p: no permission rule matches Bash(git cleanup)");
  });
  test(
    "an ast-grep rule whose examples fail fails",
    () => {
      const rule =
        'id: demo/a\ndescription: A.\nscope: {language: python}\nenforce: [{target: ast-grep, mode: forbid, rule: {pattern: "eval($$$)"}}]\nexamples: {match: ["exec(x)"], no_match: ["print(x)"]}\n';
      expect(selfTest(packsWith({ "demo/rules/a.yaml": rule })).failures.some((x) => x.startsWith("ast-grep test failed") && x.includes("demo/a"))).toBe(true);
    },
    T,
  );
  test("a context.md over 10 lines fails", () => {
    expect(selfTest(packsWith({ "demo/context.md": "line\n".repeat(11) })).failures).toContain("demo/context.md: 11 lines (max 10)");
  });
  test("a rule id that does not match its file name fails", () => {
    expect(selfTest(packsWith({ "demo/rules/other.yaml": guardRule("rm -rf x", "rm x") })).failures[0]).toContain('id must be "demo/other"');
  });
  test("a rule without examples fails", () => {
    const rule = 'id: demo/n\ndescription: N.\nscope: {language: any}\nenforce: [{target: guard, decision: ask, pattern: "x"}]\n';
    expect(selfTest(packsWith({ "demo/rules/n.yaml": rule })).failures).toContain("demo/n: needs match and no_match examples");
  });
  const checkRule = (match: string, noMatch: string) =>
    `id: demo/c\ndescription: No TODO file.\nscope: {language: any}\nenforce: [{target: check, stage: lint, run: "! git ls-files TODO | grep ."}]\nexamples:\n  match: [{files: {${match}: x}}]\n  no_match: [{files: {${noMatch}: x}}]\n`;
  test(
    "check examples run check.sh in a temporary repository: a correct check rule passes",
    () => {
      expect(selfTest(packsWith({ "demo/rules/c.yaml": checkRule("TODO", "DONE") })).failures).toEqual([]);
    },
    T,
  );
  test(
    "a check match example that does not fail the check fails, and so does a no_match example that does",
    () => {
      const f = selfTest(packsWith({ "demo/rules/c.yaml": checkRule("DONE", "TODO") })).failures;
      expect(f.some((x) => x.startsWith("demo/c: match example 1 did not fail the check"))).toBe(true);
      expect(f.some((x) => x.startsWith("demo/c: no_match example 1 failed the check: forge:demo/c failed"))).toBe(true);
    },
    T,
  );
  test(
    "a check example whose tool is not available is reported skipped, never passed",
    () => {
      const rule =
        'id: demo/r\ndescription: R.\nscope: {language: python}\nenforce: [{target: check, stage: lint, run: "forge_do ruff lint ."}]\nexamples: {match: ["import os\\n"], no_match: ["X = 1\\n"]}\n';
      const r = selfTest(packsWith({ "demo/rules/r.yaml": rule }), undefined, { toolBins: [] });
      expect(r.failures).toEqual([]);
      expect(r.notes).toContain("demo/r: match example 1 skipped: ruff not available");
    },
    T,
  );
  test("a grep pattern that misses a match example, or matches a no_match example, fails", () => {
    const rule =
      'id: demo/g2\ndescription: G.\nscope: {language: python}\nenforce: [{target: grep, mode: no-increase, pattern: "#[[:space:]]*noqa"}]\nexamples: {match: ["x  # type: ignore"], no_match: ["y  # noqa"]}\n';
    const f = selfTest(packsWith({ "demo/rules/g2.yaml": rule })).failures;
    expect(f).toContain('demo/g2: grep pattern does not match match example 1: "x  # type: ignore"');
    expect(f).toContain('demo/g2: grep pattern matches no_match example 1: "y  # noqa"');
  });
  test("fixture examples are for check rules only, or one file without head for a rule of tool targets", () => {
    const failure = "demo/f: fixture examples (files) are for check rules only, or one file without head for a rule of tool targets";
    const rule = 'id: demo/f\ndescription: F.\nscope: {language: any}\nenforce: [{target: guard, decision: ask, pattern: "x"}]\nexamples: {match: [{files: {a: x}}], no_match: ["y"]}\n';
    expect(selfTest(packsWith({ "demo/rules/f.yaml": rule })).failures).toContain(failure);
    const tool = (ex: string) =>
      `id: demo/f\ndescription: F.\nscope: {language: any}\nenforce: [{target: tool, tool: biome, expects: lint/a11y/useAltText, snippet: x}]\nexamples: {match: [${ex}], no_match: ["y"]}\n`;
    expect(selfTest(packsWith({ "demo/rules/f.yaml": tool("{files: {src/Card.vue: x}}") })).failures).toEqual([]);
    expect(selfTest(packsWith({ "demo/rules/f.yaml": tool("{files: {a.vue: x, b.vue: y}}") })).failures).toContain(failure);
    expect(selfTest(packsWith({ "demo/rules/f.yaml": tool("{head: {a.vue: x}, files: {b.vue: y}}") })).failures).toContain(failure);
  });
  test("a pack requiring an unknown pack fails", () => {
    expect(selfTest(packsWith({ "demo/pack.yaml": "id: demo\ndescription: D.\nlevel: project\nrequires: [ghost]\n" })).failures[0]).toContain("requires unknown pack ghost");
  });
  test("a user-level pack with detect fails", () => {
    expect(selfTest(packsWith({ "demo/pack.yaml": "id: demo\ndescription: D.\nlevel: user\ndetect: {files_any: [x]}\n" })).failures[0]).toContain("invalid pack");
  });
});

describe("permission matching used by forge test", () => {
  test.each([
    ["Bash(git clean:*)", "Bash(git clean -fd)", true],
    ["Bash(git clean:*)", "Bash(git clean)", true],
    ["Bash(git clean:*)", "Bash(git cleanup-branches)", false],
    ["Read(**/.env)", "Read(apps/api/.env)", true],
    ["Read(**/.env)", "Read(.env.example)", false],
    ["Read(**/.env)", "Edit(.env)", false],
  ])("%s matches %s: %p", (rule, call, want) => {
    expect(permissionMatches(rule, call)).toBe(want);
  });
});

/**
 * ngc needs the framework's packages, so typescript/angular/typecheck has no examples in
 * its file: here a component imports @angular/core from the tools cache (tsconfig paths).
 * @rule typescript/angular/typecheck
 */
test.if(hasBun)(
  "typescript/angular/typecheck: ngc reports a template error of a changed component, and passes a correct one",
  () => {
    const ng = ensureNodeTools("angular", ANGULAR_TOOLS);
    const rule = rules.find((r) => r.id === "typescript/angular/typecheck")!;
    const target = rule.enforce[0] as CheckTarget;
    const head = {
      "tsconfig.json": JSON.stringify({
        compilerOptions: { strict: true, target: "ES2022", module: "preserve", moduleResolution: "bundler", skipLibCheck: true, paths: { "@angular/*": [join(ng, "node_modules/@angular/*")] } },
        angularCompilerOptions: { strictTemplates: true },
      }),
      "tsconfig.app.json": '{"extends": "./tsconfig.json", "files": ["src/main.ts"]}\n',
    };
    const component = (field: string) =>
      `import { Component } from "@angular/core";\n\n@Component({\n  selector: "app-root",\n  template: "<p>{{ user.${field} }}</p>",\n})\nexport class App {\n  user = { name: "Ada" };\n}\n`;
    const bins = [join(ng, "node_modules/.bin")];
    expect(runCheckExample(rule, target, { head, files: { "src/main.ts": component("nme") } }, "match", bins)).toEqual({ outcome: "pass", detail: "" });
    expect(runCheckExample(rule, target, { head, files: { "src/main.ts": component("name") } }, "no_match", bins)).toEqual({ outcome: "pass", detail: "" });
    // no application tsconfig: reported, never passed
    expect(runCheckExample(rule, target, { files: { "src/main.ts": component("nme") } }, "match", bins).outcome).toBe("skip");
  },
  T,
);

/**
 * The auth CLI loads the project's auth config and its ORM schema, so the rule has no
 * examples in its file; here a stand-in `better-auth` (the auth package's CLI) answers as it does (exit 0, 1 on
 * SCHEMA_MISMATCH, 2 when it cannot check). An adapter that reads the live database runs
 * only against a local one.
 * @rule typescript/better-auth/schema-check
 */
test("typescript/better-auth/schema-check: a schema mismatch fails, a check the CLI cannot run is skipped, and the database adapter needs a local database", () => {
  const rule = rules.find((r) => r.id === "typescript/better-auth/schema-check")!;
  const target = rule.enforce[0] as CheckTarget;
  const bin = mkdtempSync(join(tmpdir(), "forge-auth-"));
  const auth = (code: number, say: string) => {
    writeFileSync(join(bin, "better-auth"), `#!/bin/sh\n${STUB_VERSION}[ "$1 $2 $3" = "check schema --config" ] || exit 9\nprintf '%s\\n' '${say}' >&2\nexit ${code}\n`);
    chmodSync(join(bin, "better-auth"), 0o755);
  };
  const prisma =
    'import { betterAuth } from "better-auth";\nimport { prismaAdapter } from "better-auth/adapters/prisma";\nimport { organization } from "better-auth/plugins";\nimport { prisma } from "./db";\n\nexport const auth = betterAuth({ database: prismaAdapter(prisma, { provider: "postgresql" }), plugins: [organization()] });\n';
  const pool = 'import { betterAuth } from "better-auth";\nimport { Pool } from "pg";\n\nexport const auth = betterAuth({ database: new Pool({ connectionString: process.env.DATABASE_URL }) });\n';
  const change = (config: string, env = "") => ({ head: { "src/lib/auth.ts": "export {};\n", ...(env ? { ".env": env } : {}) }, files: { "src/lib/auth.ts": config } });
  auth(1, 'Schema mismatch against the generated Prisma client model: missing table "organization"');
  expect(runCheckExample(rule, target, change(prisma), "match", [bin])).toEqual({ outcome: "pass", detail: "" });
  auth(0, "Schema check passed against the generated Prisma client model.");
  expect(runCheckExample(rule, target, change(prisma), "no_match", [bin])).toEqual({ outcome: "pass", detail: "" });
  auth(2, "Could not load the Better Auth configuration.");
  expect(runCheckExample(rule, target, change(prisma), "match", [bin])).toEqual({ outcome: "skip", detail: "the schema check could not run: Could not load the Better Auth configuration." });
  auth(1, "missing table");
  expect(runCheckExample(rule, target, change(pool), "match", [bin])).toEqual({ outcome: "skip", detail: "no local database (the Better Auth adapter of src/lib/auth.ts checks the live database)" });
  expect(runCheckExample(rule, target, change(pool, "DATABASE_URL=postgres://app@localhost:5432/app\n"), "match", [bin])).toEqual({ outcome: "pass", detail: "" });
});
