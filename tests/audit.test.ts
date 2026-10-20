// §11: audits against the real tools, with correct and incorrect configs.
// Installs Biome, TypeScript (bun) and ruff, basedpyright (uv) once into a
// cache directory; needs the network on first run.
import { beforeAll, describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { forge, git, sampleRepo, sh } from "./helpers";

const CACHE = join(tmpdir(), "forge-audit-tools-v1");
const T = 300_000;
const hasBun = sh("sh", ["-c", "command -v bun"]).status === 0;
const hasUv = sh("sh", ["-c", "command -v uv"]).status === 0;

beforeAll(() => {
  mkdirSync(CACHE, { recursive: true });
  if (hasBun && !existsSync(join(CACHE, "node_modules/.bin/biome"))) {
    writeFileSync(join(CACHE, "package.json"), '{"name":"tools","private":true}');
    const r = sh("bun", ["add", "-d", "@biomejs/biome@2.2.4", "typescript@5.9.3"], { cwd: CACHE });
    if (r.status !== 0) throw new Error(`bun add failed: ${r.stderr}`);
  }
  if (hasUv && !existsSync(join(CACHE, ".venv/bin/basedpyright"))) {
    let r = sh("uv", ["venv", "-q", join(CACHE, ".venv")]);
    if (r.status === 0) r = sh("uv", ["pip", "install", "-q", "--python", join(CACHE, ".venv/bin/python"), "ruff==0.14.1", "basedpyright==1.31.7"]);
    if (r.status !== 0) throw new Error(`uv install failed: ${r.stderr}`);
  }
}, T);

/** A sample repo with the cached tools linked in and the given config files. */
function repo(name: string, files: Record<string, string>): string {
  const dir = sampleRepo(name);
  for (const [rel, text] of Object.entries(files)) writeFileSync(join(dir, rel), text);
  if (name === "fastapi-app") symlinkSync(join(CACHE, ".venv"), join(dir, ".venv"));
  else symlinkSync(join(CACHE, "node_modules"), join(dir, "node_modules"));
  git(dir, "add", "-A");
  git(dir, "commit", "-qm", "config");
  return dir;
}

function audit(dir: string): string {
  const r = forge(["status"], dir);
  expect(r.status).toBe(0);
  // auditing leaves no file behind
  expect(git(dir, "status", "--porcelain").stdout).toBe("");
  return r.stdout;
}

const BIOME = (rules: object) =>
  JSON.stringify({ vcs: { enabled: true, clientKind: "git", useIgnoreFile: true }, linter: { enabled: true, rules: { recommended: false, ...rules } } });
const TSCONFIG = (strict: boolean) => JSON.stringify({ compilerOptions: { strict, noEmit: true, skipLibCheck: true }, include: ["src"] });

describe.if(hasBun)("TypeScript tools", () => {
  test(
    "correct: Biome rules on, strict tsconfig -> satisfied",
    () => {
      const out = audit(repo("nextjs-prisma-app", { "biome.json": BIOME({ suspicious: { noExplicitAny: "error", noTsIgnore: "error" } }), "tsconfig.json": TSCONFIG(true) }));
      expect(out).toContain("forge:typescript/no-explicit-any biome lint/suspicious/noExplicitAny: satisfied");
      expect(out).toContain("forge:typescript/no-ts-ignore biome lint/suspicious/noTsIgnore: satisfied");
      expect(out).toContain("forge:typescript/strict tsc TS7006: satisfied");
    },
    T,
  );
  test(
    "incorrect: Biome rules off, strict off -> unsatisfied with the snippet",
    () => {
      const out = audit(repo("nextjs-prisma-app", { "biome.json": BIOME({ suspicious: { noExplicitAny: "off", noTsIgnore: "off" } }), "tsconfig.json": TSCONFIG(false) }));
      expect(out).toContain("forge:typescript/no-explicit-any biome lint/suspicious/noExplicitAny: unsatisfied");
      expect(out).toContain('add to the biome config: "linter": {"rules": {"suspicious": {"noExplicitAny": "error"}}}');
      expect(out).toContain("forge:typescript/no-ts-ignore biome lint/suspicious/noTsIgnore: unsatisfied");
      expect(out).toContain("forge:typescript/strict tsc TS7006: unsatisfied");
      expect(out).toContain('add to the tsc config: "compilerOptions": {"strict": true}');
    },
    T,
  );
  test(
    "configured but not installed -> skipped, never satisfied",
    () => {
      const dir = sampleRepo("nextjs-prisma-app");
      const out = forge(["status"], dir).stdout;
      expect(out).toContain("biome lint/suspicious/noExplicitAny: skipped (biome is configured but not installed");
    },
    T,
  );
  test(
    "apply prints unsatisfied audits",
    () => {
      const dir = repo("nextjs-prisma-app", { "biome.json": BIOME({}), "tsconfig.json": TSCONFIG(true) });
      const r = forge(["apply", "--yes"], dir);
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("noExplicitAny: unsatisfied");
    },
    T,
  );
});

describe.if(hasUv)("Python tools", () => {
  const PYPROJECT = (select: string[], any: string) =>
    `[project]\nname = "fastapi-app"\nversion = "0.1.0"\ndependencies = ["fastapi>=0.115", "sqlalchemy>=2.0.36", "alembic>=1.14", "pydantic>=2.9"]\n\n[dependency-groups]\ndev = ["pytest>=8.3", "ruff>=0.8", "basedpyright>=1.22"]\n\n[tool.ruff.lint]\nselect = ${JSON.stringify(select)}\n\n[tool.basedpyright]\nreportExplicitAny = "${any}"\n`;
  test(
    "correct: E722, ANN401, reportExplicitAny on -> satisfied",
    () => {
      const out = audit(repo("fastapi-app", { "pyproject.toml": PYPROJECT(["E", "ANN401"], "error") }));
      expect(out).toContain("forge:python/no-bare-except ruff E722: satisfied");
      expect(out).toContain("forge:python/no-explicit-any ruff ANN401: satisfied");
      expect(out).toContain("forge:python/no-explicit-any basedpyright reportExplicitAny: satisfied");
    },
    T,
  );
  test(
    "incorrect: rules not selected -> unsatisfied with the snippet",
    () => {
      const out = audit(repo("fastapi-app", { "pyproject.toml": PYPROJECT(["F"], "none") }));
      expect(out).toContain("forge:python/no-bare-except ruff E722: unsatisfied");
      expect(out).toContain('add to the ruff config: [tool.ruff.lint] extend-select = ["E722"]');
      expect(out).toContain("forge:python/no-explicit-any ruff ANN401: unsatisfied");
      expect(out).toContain("forge:python/no-explicit-any basedpyright reportExplicitAny: unsatisfied");
    },
    T,
  );
  test(
    "stricter config satisfies automatically (select ALL)",
    () => {
      const out = audit(repo("fastapi-app", { "pyproject.toml": PYPROJECT(["ALL"], "error") }));
      expect(out).toContain("forge:python/no-bare-except ruff E722: satisfied");
      expect(out).toContain("forge:python/no-explicit-any ruff ANN401: satisfied");
    },
    T,
  );
});
