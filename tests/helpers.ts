import { spawnSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

export const ROOT = join(import.meta.dir, "..");
export const FORGE = join(ROOT, "dist", "forge.js");

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
export const TOOL_VERSIONS = { biome: "2.5.15", typescript: "7.0.2", ruff: "0.17.1", basedpyright: "1.40.2" };
export const TOOLS_CACHE = join(tmpdir(), "forge-tools-v2");
export const hasBun = sh("sh", ["-c", "command -v bun"]).status === 0;
export const hasUv = sh("sh", ["-c", "command -v uv"]).status === 0;

/** Installs Biome and TypeScript (bun) and ruff and basedpyright (uv) once into TOOLS_CACHE. */
export function ensureTools(): void {
  mkdirSync(TOOLS_CACHE, { recursive: true });
  if (hasBun && !existsSync(join(TOOLS_CACHE, "node_modules/.bin/tsc"))) {
    writeFileSync(join(TOOLS_CACHE, "package.json"), '{"name":"tools","private":true}');
    const r = sh("bun", ["add", "-d", `@biomejs/biome@${TOOL_VERSIONS.biome}`, `typescript@${TOOL_VERSIONS.typescript}`], { cwd: TOOLS_CACHE });
    if (r.status !== 0) throw new Error(`bun add failed: ${r.stderr}`);
  }
  if (hasUv && !existsSync(join(TOOLS_CACHE, ".venv/bin/basedpyright"))) {
    let r = sh("uv", ["venv", "-q", join(TOOLS_CACHE, ".venv")]);
    if (r.status === 0)
      r = sh("uv", ["pip", "install", "-q", "--python", join(TOOLS_CACHE, ".venv/bin/python"), `ruff==${TOOL_VERSIONS.ruff}`, `basedpyright==${TOOL_VERSIONS.basedpyright}`]);
    if (r.status !== 0) throw new Error(`uv install failed: ${r.stderr}`);
  }
}

/** Links the cached tools into a sample repository (node_modules or .venv). */
export function linkTools(repo: string, kind: "node" | "python"): void {
  if (kind === "python") symlinkSync(join(TOOLS_CACHE, ".venv"), join(repo, ".venv"));
  else symlinkSync(join(TOOLS_CACHE, "node_modules"), join(repo, "node_modules"));
}
