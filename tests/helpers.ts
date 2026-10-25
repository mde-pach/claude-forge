import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

export const ROOT = join(import.meta.dir, "..");
export const FORGE = join(ROOT, "dist", "forge.js");

// The hooks keep their state outside the repository, under $FORGE_STATE_HOME
// (default ~/.claude/forge-state): the suite never writes the user's home.
process.env.FORGE_STATE_HOME ||= mkdtempSync(join(tmpdir(), "forge-state-home-"));

/** The hooks' state directory for <repo> (lib.sh forge_state_of): $FORGE_STATE_HOME/<sha256 of its physical path, 16 hex>. */
export function stateDir(repo: string): string {
  return join(process.env.FORGE_STATE_HOME as string, createHash("sha256").update(realpathSync(repo)).digest("hex").slice(0, 16));
}

export interface Result {
  status: number;
  stdout: string;
  stderr: string;
}

export function sh(cmd: string, args: string[], opts: { cwd?: string; input?: string; env?: Record<string, string> } = {}): Result {
  const r = spawnSync(cmd, args, {
    cwd: opts.cwd,
    input: opts.input ?? "",
    encoding: "utf8",
    env: { ...process.env, ...opts.env },
    maxBuffer: 64 * 1024 * 1024,
  });
  return { status: r.status ?? 1, stdout: r.stdout ?? "", stderr: r.stderr ?? "" };
}

/** A directory with a fake `claude` reporting <version>, prepended to PATH. */
export function fakeClaude(version = "2.1.300"): Record<string, string> {
  const bin = mkdtempSync(join(tmpdir(), "forge-bin-"));
  writeFileSync(join(bin, "claude"), `#!/bin/sh\necho "${version} (Claude Code)"\n`);
  chmodSync(join(bin, "claude"), 0o755);
  return { PATH: `${bin}:${process.env.PATH}` };
}

export function forge(args: string[], cwd: string, env: Record<string, string> = fakeClaude()): Result {
  return sh("node", [FORGE, ...args], { cwd, env });
}

const GIT_ENV = {
  GIT_AUTHOR_NAME: "t",
  GIT_AUTHOR_EMAIL: "t@example.com",
  GIT_COMMITTER_NAME: "t",
  GIT_COMMITTER_EMAIL: "t@example.com",
  GIT_CONFIG_NOSYSTEM: "1",
};

export function git(cwd: string, ...args: string[]): Result {
  return sh("git", args, { cwd, env: GIT_ENV });
}

/** Copies tests/generator/<name> into a fresh git repository with one commit. */
export function sampleRepo(name: string): string {
  const dir = mkdtempSync(join(tmpdir(), `forge-${name}-`));
  cpSync(join(ROOT, "tests", "generator", name), dir, { recursive: true });
  git(dir, "init", "-q", "-b", "main");
  git(dir, "add", "-A");
  git(dir, "commit", "-qm", "init");
  return dir;
}

function walk(dir: string, base: string, out: string[]): void {
  if (!existsSync(dir)) return;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) walk(p, base, out);
    else out.push(relative(base, p));
  }
}

/** Every file forge writes in a project (owned, settings, pre-commit), as one text. */
export function snapshotOf(repo: string): string {
  const files: string[] = [];
  walk(join(repo, ".forge"), repo, files);
  walk(join(repo, ".claude"), repo, files);
  if (existsSync(join(repo, ".git", "hooks", "pre-commit"))) files.push(".git/hooks/pre-commit");
  return files
    .filter((f) => !f.startsWith(".forge/state/"))
    .sort()
    .map((f) => `===== ${f}\n${readFileSync(join(repo, f), "utf8")}`)
    .join("\n");
}

export function expectSnapshot(name: string, text: string): { ok: boolean; path: string } {
  const dir = join(ROOT, "tests", "generator", "__snapshots__");
  const path = join(dir, `${name}.snap`);
  if (process.env.FORGE_UPDATE_SNAPSHOTS || !existsSync(path)) {
    mkdirSync(dir, { recursive: true });
    writeFileSync(path, text);
    return { ok: true, path };
  }
  return { ok: readFileSync(path, "utf8") === text, path };
}

export function hookJson(fields: Record<string, unknown>): string {
  return JSON.stringify({ session_id: "test", hook_event_name: "Test", ...fields });
}

export function put(repo: string, rel: string, text: string): void {
  mkdirSync(join(repo, rel, ".."), { recursive: true });
  writeFileSync(join(repo, rel), text);
}

/** A sample repository after a successful `forge apply --yes`. */
export function applied(name: string): string {
  const repo = sampleRepo(name);
  const r = forge(["apply", "--yes"], repo);
  if (r.status !== 0) throw new Error(`forge apply failed in ${name}:\n${r.stdout}\n${r.stderr}`);
  return repo;
}

/** Commits everything in the repository. */
export function commitAll(repo: string, message = "commit"): void {
  git(repo, "add", "-A");
  git(repo, "commit", "-qm", message);
}

/** Pinned project tools used by specs that need the real thing. */
export const TOOL_VERSIONS = {
  biome: "2.5.15",
  typescript: "7.0.2",
  // tsc 5.x colours diagnostics even when piped, when the tsconfig sets "pretty": true
  // (tsc 7 does not); diagnostics attribution must read that format too.
  typescript5: "5.5.4",
  ruff: "0.17.1",
  basedpyright: "1.40.2",
  // types that Biome reads to report deprecated imports
  react: "19.2.0",
  "@types/react": "19.2.2",
  // migration linting (Alembic renders SQL offline; the async template needs greenlet)
  alembic: "1.20.0",
  sqlalchemy: "2.1.5",
  // typescript/vue/typecheck (vue-tsc runs on the JavaScript TypeScript 5 API, not tsc 7)
  "vue-tsc": "3.3.13",
  typescriptForVue: "5.9.3",
  // python/django/makemigrations runs the project's manage.py (5.2 LTS: Python 3.10+)
  django: "5.2.18",
};
export const TOOLS_CACHE = join(tmpdir(), "forge-tools-v2");
export const hasBun = sh("sh", ["-c", "command -v bun"]).status === 0;
export const hasUv = sh("sh", ["-c", "command -v uv"]).status === 0;

/**
 * The Python of every cached tools venv. Pinned, so the suite does not depend on
 * whichever interpreter uv finds first: the python/config-ratchet helper reads
 * TOML with tomllib (3.11+), and a cache built with 3.10 failed those checks.
 */
export const TOOLS_PYTHON = "3.12";

/** Creates <dir> with Python TOOLS_PYTHON and installs <pkgs>, unless it has <probe> and that Python already; a venv on another Python is rebuilt. */
function ensureVenv(dir: string, probe: string, pkgs: string[]): void {
  const cfg = join(dir, "pyvenv.cfg");
  const pinned = new RegExp(`^version(_info)? = ${TOOLS_PYTHON.replace(".", "\\.")}\\.`, "m");
  if (existsSync(join(dir, probe)) && existsSync(cfg) && pinned.test(readFileSync(cfg, "utf8"))) return;
  rmSync(dir, { recursive: true, force: true });
  let r = sh("uv", ["venv", "-q", "--python", TOOLS_PYTHON, dir]);
  if (r.status === 0) r = sh("uv", ["pip", "install", "-q", "--python", join(dir, "bin/python"), ...pkgs]);
  if (r.status !== 0) throw new Error(`uv install failed: ${r.stderr}`);
}

/** Installs Biome, TypeScript and React's types (bun), ruff and basedpyright (uv), and Alembic and Django (uv, own venvs) once into TOOLS_CACHE. */
export function ensureTools(): void {
  mkdirSync(TOOLS_CACHE, { recursive: true });
  if (hasBun && !existsSync(join(TOOLS_CACHE, "node_modules/@types/react/package.json"))) {
    writeFileSync(join(TOOLS_CACHE, "package.json"), '{"name":"tools","private":true}');
    const pkgs = ["@biomejs/biome", "typescript", "react", "@types/react"] as const;
    const r = sh("bun", ["add", "-d", ...pkgs.map((p) => `${p}@${TOOL_VERSIONS[p === "@biomejs/biome" ? "biome" : p]}`)], { cwd: TOOLS_CACHE });
    if (r.status !== 0) throw new Error(`bun add failed: ${r.stderr}`);
  }
  if (!hasUv) return;
  ensureVenv(join(TOOLS_CACHE, ".venv-alembic"), "bin/alembic", [`alembic==${TOOL_VERSIONS.alembic}`, `sqlalchemy[asyncio]==${TOOL_VERSIONS.sqlalchemy}`]);
  ensureVenv(join(TOOLS_CACHE, ".venv-django"), "bin/django-admin", [`django==${TOOL_VERSIONS.django}`]);
  ensureVenv(join(TOOLS_CACHE, ".venv"), "bin/basedpyright", [`ruff==${TOOL_VERSIONS.ruff}`, `basedpyright==${TOOL_VERSIONS.basedpyright}`]);
}

/** Installs vue-tsc (TOOL_VERSIONS["vue-tsc"]) with its TypeScript in its own cache directory; returns its bin directory. */
export function ensureVueTools(): string {
  const dir = join(TOOLS_CACHE, "vue");
  if (hasBun && !existsSync(join(dir, "node_modules/vue-tsc/package.json"))) {
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "package.json"), '{"name":"tools-vue","private":true}');
    const r = sh("bun", ["add", "-d", `vue-tsc@${TOOL_VERSIONS["vue-tsc"]}`, `typescript@${TOOL_VERSIONS.typescriptForVue}`], { cwd: dir });
    if (r.status !== 0) throw new Error(`bun add failed: ${r.stderr}`);
  }
  return join(dir, "node_modules", ".bin");
}

/** Installs the real tsc 5.x (TOOL_VERSIONS.typescript5) in its own cache directory. */
export function ensureTs5(): string {
  const dir = join(TOOLS_CACHE, "ts5");
  if (!existsSync(join(dir, "node_modules/typescript/package.json"))) {
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "package.json"), '{"name":"tools-ts5","private":true}');
    const r = sh("bun", ["add", "-d", `typescript@${TOOL_VERSIONS.typescript5}`], { cwd: dir });
    if (r.status !== 0) throw new Error(`bun add failed: ${r.stderr}`);
  }
  return dir;
}

/** After linkTools(repo, "node"): makes the repository's `tsc` the real tsc 5.x. */
export function linkTs5(repo: string): void {
  const ts5 = join(ensureTs5(), "node_modules");
  const nm = join(repo, "node_modules");
  rmSync(join(nm, "typescript"), { recursive: true, force: true });
  symlinkSync(join(ts5, "typescript"), join(nm, "typescript"));
  const sharedBin = join(TOOLS_CACHE, "node_modules", ".bin");
  rmSync(join(nm, ".bin"), { recursive: true, force: true });
  mkdirSync(join(nm, ".bin"));
  for (const entry of readdirSync(sharedBin)) {
    if (entry !== "tsc" && entry !== "tsserver") symlinkSync(join(sharedBin, entry), join(nm, ".bin", entry));
  }
  symlinkSync(join(ts5, "typescript", "bin", "tsc"), join(nm, ".bin", "tsc"));
}

/**
 * Puts the cached tools into a sample repository (.venv, or node_modules). node_modules is a
 * real directory: tools are linked, but React and its types are copied, because Biome does not
 * resolve types through symlinks (it would not see React's @deprecated tags).
 */
export function linkTools(repo: string, kind: "node" | "python" | "alembic" | "django"): void {
  if (kind === "python") symlinkSync(join(TOOLS_CACHE, ".venv"), join(repo, ".venv"));
  else if (kind === "django") symlinkSync(join(TOOLS_CACHE, ".venv-django"), join(repo, ".venv"));
  else if (kind === "alembic") symlinkSync(join(TOOLS_CACHE, ".venv-alembic"), join(repo, ".venv"));
  else {
    const from = join(TOOLS_CACHE, "node_modules");
    mkdirSync(join(repo, "node_modules"));
    for (const entry of readdirSync(from)) {
      if (entry === "react" || entry === "@types") cpSync(join(from, entry), join(repo, "node_modules", entry), { recursive: true, dereference: true });
      else symlinkSync(join(from, entry), join(repo, "node_modules", entry));
    }
  }
}
