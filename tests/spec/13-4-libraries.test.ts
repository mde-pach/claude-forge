/**
 * @spec 13.4 · Libraries and project-run helpers
 *
 * `detect.library_any` activates a pack when the project is a library published
 * to that ecosystem. A Python library's `pyproject.toml` names the project and
 * declares a `[build-system]` (an application from `uv init` has none), has no
 * `[project.scripts]` / `[project.gui-scripts]`, no `Private ::` classifier and
 * no `manage.py`. An npm library's `package.json` is not `"private": true`, has a
 * `name`, and declares `exports`, or `main` / `module` together with `types`,
 * `typings` or `files` (`main` alone is `npm init`'s default).
 */
import { describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { isNpmLibrary, isPyLibrary } from "../../src/packs";
import { commitAll, forge, git, put } from "../helpers";

/** @pack python/library */
describe("a Python library", () => {
  const lib = { project: { name: "acme" }, "build-system": { "build-backend": "hatchling.build" } };
  test("is a named project with a build backend", () => {
    expect(isPyLibrary(lib, false)).toBe(true);
  });
  test("is not an application: no build backend, console scripts, a Private classifier or a manage.py", () => {
    expect(isPyLibrary({ project: { name: "app" } }, false)).toBe(false);
    expect(isPyLibrary({ ...lib, project: { name: "acme", scripts: { acme: "acme.cli:main" } } }, false)).toBe(false);
    expect(isPyLibrary({ ...lib, project: { name: "acme", classifiers: ["Private :: Do Not Upload"] } }, false)).toBe(false);
    expect(isPyLibrary(lib, true)).toBe(false);
  });
});

/** @pack typescript/library */
describe("an npm library", () => {
  test("declares exports, or main or module with types, typings or files, and is not private", () => {
    expect(isNpmLibrary({ name: "acme", exports: { ".": "./dist/index.js" } })).toBe(true);
    expect(isNpmLibrary({ name: "acme", main: "dist/index.js", types: "dist/index.d.ts" })).toBe(true);
  });
  test("is not an application: private, unnamed, or only npm init's main", () => {
    expect(isNpmLibrary({ name: "web", private: true, exports: "./index.js" })).toBe(false);
    expect(isNpmLibrary({ exports: "./index.js" })).toBe(false);
    expect(isNpmLibrary({ name: "server", main: "index.js" })).toBe(false);
  });
  test("forge apply activates typescript/library for a library and not for a private app", () => {
    const repo = mkdtempSync(join(tmpdir(), "forge-lib-"));
    try {
      git(repo, "init", "-q", "-b", "main");
      put(repo, "package.json", '{"name":"acme","version":"1.0.0","exports":"./dist/index.js","devDependencies":{"typescript":"5.9.3"}}\n');
      put(repo, "src/index.ts", "export const one = 1;\n");
      commitAll(repo, "init");
      expect(forge(["status"], repo).stdout).toContain("typescript/library");
      put(repo, "package.json", '{"name":"acme","private":true,"exports":"./dist/index.js","devDependencies":{"typescript":"5.9.3"}}\n');
      expect(forge(["status"], repo).stdout).not.toContain("typescript/library");
    } finally {
      rmSync(repo, { recursive: true, force: true });
    }
  });
});
