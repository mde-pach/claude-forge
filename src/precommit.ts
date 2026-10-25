import { existsSync } from "node:fs";
import { isAbsolute, join, relative } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { parse as parseYaml } from "yaml";
import { run, sha256 } from "./util.js";

// ---------------------------------------------------------------- pre-commit through a hook manager (§10.3)

export const CHECK = "sh .forge/check.sh --changed";
export const BEGIN = "# forge: begin (forge remove deletes this block)";
export const END = "# forge: end";

export type ManagerKind = "pre-commit" | "husky" | "lefthook" | "hooks";

export interface HookManager {
  kind: ManagerKind;
  /** How messages name it. */
  name: string;
  /** The file Forge adds its block to (relative to the repository); undefined when it cannot (core.hooksPath outside the repository). */
  file?: string;
}

/** A hook manager in use, or undefined. */
export function hookManager(base: string): HookManager | undefined {
  if (existsSync(join(base, ".husky"))) return { kind: "husky", name: ".husky", file: ".husky/pre-commit" };
  for (const f of ["lefthook.yml", "lefthook.yaml", ".lefthook.yml"]) if (existsSync(join(base, f))) return { kind: "lefthook", name: f, file: f };
  if (existsSync(join(base, ".pre-commit-config.yaml"))) return { kind: "pre-commit", name: ".pre-commit-config.yaml", file: ".pre-commit-config.yaml" };
  const hp = run("git", ["config", "core.hooksPath"], { cwd: base }).stdout.trim();
  if (!hp) return undefined;
  const rel = relative(base, isAbsolute(hp) ? hp : join(base, hp));
  const inside = rel !== "" && !rel.startsWith("..") && !isAbsolute(rel);
  return { kind: "hooks", name: `core.hooksPath (${hp})`, file: inside ? `${rel}/pre-commit` : undefined };
}

/** Whether a hook file or manager config already runs Forge's check (Forge's block or the user's own line). */
export function runsForgeCheck(text: string): boolean {
  return text.includes(".forge/check.sh");
}

const isMarker = (line: string, marker: string) => line.trim() === marker;

export function hasBlock(text: string): boolean {
  return text.split("\n").some((l) => isMarker(l, BEGIN));
}

/** Hash of Forge's block (its lines without indentation), or undefined when there is none. */
export function blockHash(text: string): string | undefined {
  const lines = text.split("\n");
  const start = lines.findIndex((l) => isMarker(l, BEGIN));
  if (start < 0) return undefined;
  const end = lines.findIndex((l, i) => i > start && isMarker(l, END));
  return sha256(lines.slice(start, end < 0 ? undefined : end + 1).map((l) => l.trim()).join("\n"));
}

/** The text without Forge's blocks (from a begin line to the next end line). */
export function removeBlock(text: string): string {
  const out: string[] = [];
  let inside = false;
  for (const line of text.split("\n")) {
    if (!inside && isMarker(line, BEGIN)) inside = true;
    else if (inside) inside = !isMarker(line, END);
    else out.push(line);
  }
  return out.join("\n");
}

/** Whether a file Forge created holds nothing else once its block is removed (a shebang at most). */
export function onlyForge(textWithoutBlock: string): boolean {
  return textWithoutBlock.replace(/^#!.*$/m, "").trim() === "";
}

const block = (lines: string[], pad: string) => [BEGIN, ...lines, END].map((l) => pad + l);

const OUR_REPO = { repo: "local", hooks: [{ id: "forge-check", name: "forge check", entry: CHECK, language: "system", pass_filenames: false }] };
const REPO_LINES = ["- repo: local", "  hooks:", "    - id: forge-check", "      name: forge check", `      entry: ${CHECK}`, "      language: system", "      pass_filenames: false"];

const content = (l: string) => l.trim() !== "" && !/^\s*#/.test(l);
const indentOf = (l: string) => /^ */.exec(l)![0];

/** Index of the last line of the top-level key's block starting at `at` (its indented lines; with items at column 0, its "- " lines too). */
function blockEnd(lines: string[], at: number, itemsAtZero: boolean): number {
  let last = at;
  for (let j = at + 1; j < lines.length; j++) {
    const l = lines[j]!;
    if (!content(l)) continue;
    if (/^\s/.test(l) || (itemsAtZero && /^-(\s|$)/.test(l))) last = j;
    else break;
  }
  return last;
}

/** pre-commit: a `repo: local` entry appended to `repos:` (or `repos:` with it, at the end). */
function preCommitInsert(lines: string[]): string[] | undefined {
  const at = lines.findIndex((l) => /^repos:\s*(#.*)?$/.test(l));
  if (at < 0) {
    if (lines.some((l) => /^repos:/.test(l))) return undefined; // inline value: leave it to the user
    return [...lines, ...block(["repos:", ...REPO_LINES], "")];
  }
  const first = lines.slice(at + 1).find(content);
  const pad = first && /^\s*- /.test(first) ? indentOf(first) : "  ";
  const end = blockEnd(lines, at, pad === "");
  return [...lines.slice(0, end + 1), ...block(REPO_LINES, pad), ...lines.slice(end + 1)];
}

/** lefthook: a `forge-check` command under `pre-commit: commands:` (each created when absent). */
function lefthookInsert(lines: string[]): string[] | undefined {
  const cmd = ["forge-check:", `  run: ${CHECK}`];
  const at = lines.findIndex((l) => /^pre-commit:\s*(#.*)?$/.test(l));
  if (at < 0) {
    if (lines.some((l) => /^pre-commit:/.test(l))) return undefined;
    return [...lines, ...block(["pre-commit:", "  commands:", ...cmd.map((l) => "    " + l)], "")];
  }
  const end = blockEnd(lines, at, false);
  const children = lines.slice(at + 1, end + 1).filter(content);
  const pad = children.length ? indentOf(children[0]!) : "  ";
  const c = lines.findIndex((l, i) => i > at && i <= end && new RegExp(`^${pad}commands:\\s*(#.*)?$`).test(l));
  if (c < 0) {
    // an inline value, or lefthook's jobs (not to be mixed with commands): left to the user
    if (children.some((l) => new RegExp(`^${pad}(commands|jobs):`).test(l))) return undefined;
    return [...lines.slice(0, at + 1), ...block(["commands:", ...cmd.map((l) => "  " + l)], pad), ...lines.slice(at + 1)];
  }
  const sub = lines.slice(c + 1, end + 1).find(content);
  const subPad = sub && indentOf(sub).length > pad.length ? indentOf(sub) : pad + "  ";
  return [...lines.slice(0, c + 1), ...block(cmd, subPad), ...lines.slice(c + 1)];
}

/**
 * A shell hook (husky, core.hooksPath): the check before the first top-level exec/exit,
 * else at the end; it exits on failure, also in a hook that does not run with `sh -e`.
 */
function shellInsert(lines: string[], created: boolean): string[] {
  const ours = block([`${CHECK} || exit 1`], "");
  if (created) return ["#!/bin/sh", ...ours];
  const stop = lines.findIndex((l) => /^(exec|exit)\b/.test(l));
  return stop < 0 ? [...lines, ...ours] : [...lines.slice(0, stop), ...ours, ...lines.slice(stop)];
}

/** For YAML configs: the result parses to the original plus Forge's entry, and nothing else changed. */
function verify(kind: ManagerKind, before: string, after: string): boolean {
  if (kind === "husky" || kind === "hooks") return true;
  try {
    const b = (parseYaml(before) ?? {}) as Record<string, any>;
    const a = (parseYaml(after) ?? {}) as Record<string, any>;
    if (kind === "pre-commit") {
      if (!Array.isArray(a.repos) || !isDeepStrictEqual(a.repos.at(-1), OUR_REPO)) return false;
      return isDeepStrictEqual({ ...a, repos: a.repos.slice(0, -1) }, { ...b, repos: b.repos ?? [] });
    }
    const pc = a["pre-commit"];
    if (pc?.commands?.["forge-check"]?.run !== CHECK) return false;
    const { "forge-check": _ours, ...commands } = pc.commands as Record<string, unknown>;
    const restored: Record<string, any> = { ...a, "pre-commit": { ...pc, commands } };
    if (!Object.keys(commands).length && b["pre-commit"]?.commands === undefined) delete restored["pre-commit"].commands;
    if (!Object.keys(restored["pre-commit"]).length && b["pre-commit"] === undefined) delete restored["pre-commit"];
    return isDeepStrictEqual(restored, b);
  } catch {
    return false;
  }
}

/**
 * The hook manager's file with Forge's block added (idempotent: unchanged when the
 * block is there), or undefined when Forge cannot add it safely. `text` undefined:
 * the file does not exist yet.
 */
export function insertBlock(kind: ManagerKind, text: string | undefined): string | undefined {
  const src = text ?? "";
  if (hasBlock(src)) return src;
  const lines = src === "" ? [] : src.replace(/\n$/, "").split("\n");
  const out = kind === "pre-commit" ? preCommitInsert(lines) : kind === "lefthook" ? lefthookInsert(lines) : shellInsert(lines, text === undefined);
  if (!out) return undefined;
  const after = out.join("\n") + "\n";
  return verify(kind, src, after) ? after : undefined;
}
