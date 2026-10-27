/**
 * @spec 13.4 · Library detection and project-run checks
 *
 * `detect.library_any` activates a pack when the project is a library published
 * to that ecosystem. A Python library's `pyproject.toml` names the project and
 * declares a `[build-system]` (an application from `uv init` has none), has no
 * `[project.scripts]` / `[project.gui-scripts]`, no `Private ::` classifier and
 * no `manage.py` in its package, and something shows it is published (trove
 * classifiers, or a CI workflow uploading to PyPI). An npm library's
 * `package.json` is not `"private": true`, has a `name`, declares `exports`, or
 * `main` / `module` together with `types`, `typings` or `files` (`main` alone is
 * `npm init`'s default), and is published (`publishConfig`, Changesets, or a CI
 * workflow running a publish command). Otherwise the pack is added in
 * `forge.yaml`. Some checks run the
 * project's own code: Django's `makemigrations --check`, and helpers that compare
 * the public API with the base.
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { isNpmLibrary, isPyLibrary, publishSignals, readProjectFacts } from "../../src/packs";
import { commitAll, ensureTools, forge, git, hasUv, linkTools, put, sh } from "../helpers";

const T = 180_000;

/** @pack python/library */
describe("a Python library", () => {
  const lib = { project: { name: "acme", classifiers: ["Programming Language :: Python :: 3"] }, "build-system": { "build-backend": "hatchling.build" } };
  const bare = { project: { name: "acme" }, "build-system": { "build-backend": "hatchling.build" } };
  test("is a named project with a build backend and evidence it is published: trove classifiers, or a CI workflow uploading to PyPI", () => {
    expect(isPyLibrary(lib, false)).toBe(true);
    expect(isPyLibrary(bare, false, true)).toBe(true);
  });
  test("a build backend alone is not evidence: a packaged service or a uv workspace member has one too", () => {
    const service = { project: { name: "svc", version: "0.1.0", dependencies: ["fastapi"] }, "build-system": { requires: ["hatchling"], "build-backend": "hatchling.build" } };
    expect(isPyLibrary(service, false)).toBe(false);
  });
  test("is not an application: no build backend, console scripts, a Private classifier or a manage.py", () => {
    expect(isPyLibrary({ project: { name: "app" } }, false, true)).toBe(false);
    expect(isPyLibrary({ ...lib, project: { ...lib.project, scripts: { acme: "acme.cli:main" } } }, false)).toBe(false);
    expect(isPyLibrary({ ...lib, project: { name: "acme", classifiers: ["Private :: Do Not Upload"] } }, false, true)).toBe(false);
    expect(isPyLibrary(lib, true)).toBe(false);
  });
  test("malformed metadata (classifiers that are not a list, a project that is not a table) is not a library, and does not crash detection", () => {
    expect(isPyLibrary({ ...bare, project: { name: "acme", classifiers: "Private :: Do Not Upload" } }, false)).toBe(false);
    expect(isPyLibrary({ ...bare, project: "acme" }, false, true)).toBe(false);
  });
  test("a manage.py makes only its own workspace package a Django site", () => {
    const repo = mkdtempSync(join(tmpdir(), "forge-pylib-"));
    put(repo, "pyproject.toml", '[project]\nname = "acme"\nclassifiers = ["Programming Language :: Python :: 3"]\n\n[build-system]\nrequires = ["hatchling"]\nbuild-backend = "hatchling.build"\n\n[tool.uv.workspace]\nmembers = ["sites/*"]\n');
    put(repo, "sites/web/pyproject.toml", '[project]\nname = "web"\nclassifiers = ["Framework :: Django"]\n\n[build-system]\nrequires = ["hatchling"]\nbuild-backend = "hatchling.build"\n');
    put(repo, "sites/web/manage.py", "");
    const facts = readProjectFacts(repo, () => ["pyproject.toml", "sites/web/pyproject.toml", "sites/web/manage.py"]);
    expect([...facts.root.library]).toEqual(["pypi"]);
    expect([...facts.workspaces[0]!.library]).toEqual([]);
  });
});

/** @pack typescript/library */
describe("an npm library", () => {
  const pub = { access: "public" };
  test("declares exports, or main or module with types, typings or files, is not private, and is published: publishConfig or a release set-up", () => {
    expect(isNpmLibrary({ name: "acme", exports: { ".": "./dist/index.js" }, publishConfig: pub })).toBe(true);
    expect(isNpmLibrary({ name: "acme", main: "dist/index.js", types: "dist/index.d.ts" }, true)).toBe(true);
  });
  test("is not an application: private, unnamed, or only npm init's main", () => {
    expect(isNpmLibrary({ name: "web", private: true, exports: "./index.js", publishConfig: pub })).toBe(false);
    expect(isNpmLibrary({ exports: "./index.js", publishConfig: pub })).toBe(false);
    expect(isNpmLibrary({ name: "server", main: "index.js" }, true)).toBe(false);
  });
  test("an internal workspace package that exports, without anything publishing it, is not a library", () => {
    expect(isNpmLibrary({ name: "@acme/lib", exports: "./dist/index.js" })).toBe(false);
  });
  test("release set-ups that publish: Changesets, or a CI workflow running a publish command", () => {
    const repo = mkdtempSync(join(tmpdir(), "forge-pub-"));
    put(repo, ".github/workflows/release.yml", "jobs:\n  r:\n    steps:\n      - run: uv build && uv publish\n");
    expect(publishSignals(repo, [".github/workflows/release.yml"])).toEqual({ pypi: true, npm: false });
    put(repo, ".github/workflows/release.yml", "jobs:\n  r:\n    steps:\n      - run: pnpm publish -r\n");
    expect(publishSignals(repo, [".github/workflows/release.yml"])).toEqual({ pypi: false, npm: true });
    expect(publishSignals(repo, [".changeset/config.json"]).npm).toBe(true);
    put(repo, ".github/workflows/ci.yml", "jobs:\n  t:\n    steps:\n      - run: npm test\n");
    expect(publishSignals(repo, [".github/workflows/ci.yml"])).toEqual({ pypi: false, npm: false });
  });
  test("forge apply activates typescript/library for a published library and not for a private app", () => {
    const repo = mkdtempSync(join(tmpdir(), "forge-lib-"));
    try {
      git(repo, "init", "-q", "-b", "main");
      put(repo, "package.json", '{"name":"acme","version":"1.0.0","exports":"./dist/index.js","publishConfig":{"access":"public"},"devDependencies":{"typescript":"5.9.3"}}\n');
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

/** A Django project in src/ (like paperless-ngx), its settings pointing at a database that does not exist. */
function djangoRepo(): string {
  const repo = mkdtempSync(join(tmpdir(), "forge-django-"));
  git(repo, "init", "-q", "-b", "main");
  put(repo, "pyproject.toml", '[project]\nname = "shop"\nversion = "0.1.0"\nrequires-python = ">=3.12"\ndependencies = ["django>=5.2"]\n');
  put(repo, "src/manage.py", 'import os\nimport sys\n\nif __name__ == "__main__":\n    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "proj.settings")\n    from django.core.management import execute_from_command_line\n\n    execute_from_command_line(sys.argv)\n');
  put(repo, "src/proj/__init__.py", "");
  put(repo, "src/proj/settings.py", 'SECRET_KEY = "test"\nINSTALLED_APPS = ["shop"]\nDATABASES = {"default": {"ENGINE": "django.db.backends.sqlite3", "NAME": "/nonexistent/db.sqlite3"}}\nDEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"\n');
  put(repo, "src/shop/__init__.py", "");
  put(repo, "src/shop/migrations/__init__.py", "");
  put(repo, "src/shop/models.py", "from django.db import models\n");
  const r = forge(["apply", "--yes"], repo);
  if (r.status !== 0) throw new Error(`forge apply failed:\n${r.stdout}\n${r.stderr}`);
  linkTools(repo, "django");
  writeFileSync(join(repo, ".gitignore"), ".venv\n");
  commitAll(repo, "init");
  return repo;
}

/** @rule python/django/makemigrations */
describe("Django migrations", () => {
  let repo = "";
  beforeAll(() => {
    if (!hasUv) return;
    ensureTools();
    repo = djangoRepo();
  }, T);
  const check = () => sh("sh", [".forge/check.sh", "--changed"], { cwd: repo });

  test.if(hasUv)(
    "a model without its migration fails with makemigrations' plan, without a database; once generated it passes",
    () => {
      put(repo, "src/shop/models.py", "from django.db import models\n\n\nclass Item(models.Model):\n    name = models.CharField(max_length=10)\n");
      const missing = check();
      expect(missing.status).toBe(1);
      expect(missing.stdout).toContain("forge:python/django/makemigrations failed\nMigrations for 'shop':");
      expect(missing.stdout).toContain("Create model Item");
      expect(missing.stdout).not.toContain("RuntimeWarning");
      expect(sh(join(repo, ".venv/bin/python"), ["src/manage.py", "makemigrations", "-v0"], { cwd: repo }).status).toBe(0);
      const done = check();
      expect(done.stdout).not.toContain("forge:python/django/makemigrations");
      expect(done.status).toBe(0);
      git(repo, "reset", "-q", "--hard");
      git(repo, "clean", "-qfd", "-e", ".venv");
    },
    T,
  );

  test.if(hasUv)(
    "settings that cannot load make the check skip with the error, never block",
    () => {
      put(repo, "src/proj/settings.py", 'import os\n\nSECRET_KEY = os.environ["SECRET_KEY"]\n');
      put(repo, "src/shop/models.py", "from django.db import models\n\n\nclass Item(models.Model):\n    name = models.CharField(max_length=10)\n");
      const r = check();
      expect(r.stdout).toContain("forge:python/django/makemigrations skipped: python src/manage.py makemigrations could not run: KeyError: 'SECRET_KEY'");
      expect(r.stdout).not.toContain("makemigrations failed");
      git(repo, "reset", "-q", "--hard");
      git(repo, "clean", "-qfd", "-e", ".venv");
    },
    T,
  );
});
