import { createHash } from "node:crypto";
import { appendFileSync, chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { spawnCaptured } from "../src/util";
import { engineModules, renderChecks, renderEngineModule, renderLib, type TypesLanguage } from "../src/render";
import { selfTest, type TestReport } from "../src/selftest";

export const ROOT = join(import.meta.dir, "..");
export const FORGE = join(ROOT, "dist", "forge.js");

// The hooks keep their state outside the repository, under $FORGE_STATE_HOME
// (default ~/.claude/forge-state): the suite never writes the user's home.
process.env.FORGE_STATE_HOME ||= mkdtempSync(join(tmpdir(), "forge-state-home-"));

/**
 * Version-manager shims (pyenv's `pyenv exec "$program"` scripts) cost about 200 ms per
 * call, and the hooks and checks call uv, uvx, ruff and python many times; with the
 * shims' directory on PATH, `uvx --python 3.12` also runs the python3.12 shim in its
 * interpreter discovery (300 ms more per call). The suite resolves them once: a
 * directory of symlinks to the executables the shims would run (looked up from a
 * temporary directory, where the hooks run), ahead of PATH, and the shims' directory
 * off PATH. The tools are the same; only the shim's lookup is skipped. On macOS,
 * /usr/bin/git is an xcrun shim (12 ms per call, and the suite makes thousands): the git
 * it runs (xcrun -f git) is linked the same way. A parent that did it already (the
 * runner, scripts/test.ts) passes FORGE_TEST_BIN and the PATH down.
 *
 * Every spawn passes env (spawnCaptured does): under Bun, a spawn without it gets the
 * environment the process started with, not process.env as changed here.
 */
function resolveShims(): void {
  if (process.env.FORGE_TEST_BIN) return;
  const bin = mkdtempSync(join(tmpdir(), "forge-test-bin-"));
  process.env.FORGE_TEST_BIN = bin;
  const env = process.env;
  const shims = new Set<string>();
  const shimmed = ["uv", "uvx", "ruff", "python3", "python"].filter((n) => {
    const r = spawnCaptured("sh", ["-c", 'command -v "$1"', "sh", n], { env });
    const p = r.status === 0 ? r.stdout.trim() : "";
    const shim = p.startsWith("/") && /^#!.*\n[\s\S]*pyenv"? exec "\$program"/.test(readFileSync(p).subarray(0, 1024).toString("latin1"));
    if (shim) shims.add(dirname(p));
    return shim;
  });
  if (shimmed.length) {
    const r = spawnCaptured("sh", ["-c", 'for n; do pyenv which "$n" 2>/dev/null || echo; done', "sh", ...shimmed], { cwd: tmpdir(), env });
    const real = (r.stdout ?? "").split("\n");
    shimmed.forEach((n, i) => {
      if (real[i]?.startsWith("/")) symlinkSync(real[i]!, join(bin, n));
    });
  }
  const git = spawnCaptured("sh", ["-c", "command -v git"], { env }).stdout.trim();
  if (git === "/usr/bin/git") {
    const real = spawnCaptured("xcrun", ["-f", "git"], { env });
    if (real.status === 0 && real.stdout.trim().startsWith("/")) symlinkSync(real.stdout.trim(), join(bin, "git"));
  }
  process.env.PATH = [bin, ...(process.env.PATH ?? "").split(":").filter((d) => !shims.has(d.replace(/\/$/, "")))].join(":");
}
resolveShims();

/**
 * ty is Forge's own type checker, run through uvx (§15.2). Where a test is not about
 * what ty finds, it gives a stand-in: $FORGE_TEST_TY_STAND_IN (TY_FINDS_NOTHING), or
 * .venv/bin/ty-stand-in in the repository. This shim, first on PATH for the whole
 * suite, hands `uvx ... --from ty==<v> ty <args>` to it; everything else is the real uvx.
 */
const REAL_UVX = spawnCaptured("sh", ["-c", "command -v uvx"]).stdout.trim();
if (REAL_UVX && !process.env.FORGE_TEST_UVX_SHIM) {
  const shim = mkdtempSync(join(tmpdir(), "forge-uvx-shim-"));
  writeFileSync(
    join(shim, "uvx"),
    [
      "#!/bin/sh",
      "stand_in() {",
      '  while [ "$#" -gt 0 ]; do',
      "    a=$1; shift",
      '    case $a in ty==*) [ "${1:-}" = ty ] && { shift; exec "$stand" "$@"; } ;; esac',
      "  done",
      "}",
      "stand=${FORGE_TEST_TY_STAND_IN:-.venv/bin/ty-stand-in}",
      '[ -x "$stand" ] && stand_in "$@"',
      `exec "${REAL_UVX}" "$@"`,
      "",
    ].join("\n"),
  );
  chmodSync(join(shim, "uvx"), 0o755);
  writeFileSync(join(shim, "ty-finds-nothing"), "#!/bin/sh\nexit 0\n");
  chmodSync(join(shim, "ty-finds-nothing"), 0o755);
  process.env.FORGE_TEST_UVX_SHIM = shim;
  process.env.PATH = `${shim}:${process.env.PATH}`;
}
/**
 * The edit-time typecheck (§10.5) asks the Python types engine when it can; the tests give
 * ty a stand-in, which the engine (ty server) cannot use: they check ty check's path unless
 * they ask for the engine's (FORGE_EDIT_TYPECHECK_ENGINE=auto).
 */
process.env.FORGE_EDIT_TYPECHECK_ENGINE ??= "never";
// check.sh run by the specs is run by a person unless a spec says otherwise: the suite itself may run in
// Claude Code's Bash tool (CLAUDECODE=1), whose output check.sh leaves the tool notes out of (§10.1)
delete process.env.CLAUDECODE;

/** The environment of a run in which ty finds nothing (the sample's dependencies are not installed, and the test is not about ty). */
export const TY_FINDS_NOTHING: Record<string, string> = { FORGE_TEST_TY_STAND_IN: join(process.env.FORGE_TEST_UVX_SHIM ?? "", "ty-finds-nothing") };

/** The hooks' state directory for <repo> (lib.sh forge_state_of): $FORGE_STATE_HOME/<sha256 of its physical path, 16 hex>. */
export function stateDir(repo: string): string {
  return join(process.env.FORGE_STATE_HOME as string, createHash("sha256").update(realpathSync(repo)).digest("hex").slice(0, 16));
}

/**
 * For a test that measures time (a latency budget) and needs a quiet machine: true while
 * the parallel runner (tests/run.ts, FORGE_TEST_PHASE=parallel) runs the suite many
 * processes at a time; the test is then skipped (test.skipIf), and <file> recorded in
 * $FORGE_TEST_QUIET_LIST so that the runner runs that file's skipped tests again, alone.
 * That run (FORGE_TEST_PHASE=quiet) starts while the other processes still run: its
 * beforeAll prepares, then each of these tests waits (beforeEach, after creating
 * $FORGE_TEST_GO.ready) for the runner's go, $FORGE_TEST_GO, given once nothing else runs. False in any other run (bun test <file>).
 */
export function othersRunning(file: string): boolean {
  const go = process.env.FORGE_TEST_GO;
  if (process.env.FORGE_TEST_PHASE === "quiet" && go) {
    (require("bun:test") as typeof import("bun:test")).beforeEach(() => {
      writeFileSync(`${go}.ready`, "");
      while (!existsSync(go)) Bun.sleepSync(50);
    }, 3_600_000);
    return false;
  }
  if (process.env.FORGE_TEST_PHASE !== "parallel" || !process.env.FORGE_TEST_QUIET_LIST) return false;
  appendFileSync(process.env.FORGE_TEST_QUIET_LIST, `${file}\n`);
  return true;
}

export interface Result {
  status: number;
  stdout: string;
  stderr: string;
}

/** Runs a process to its end (its output through files: src/util.ts spawnCaptured). */
export function sh(cmd: string, args: string[], opts: { cwd?: string; input?: string; env?: Record<string, string> } = {}): Result {
  const r = spawnCaptured(cmd, args, { cwd: opts.cwd, input: opts.input ?? "", env: { ...process.env, ...opts.env } });
  return { status: r.status ?? 1, stdout: r.stdout, stderr: r.stderr };
}

/**
 * A shell script run as `sh <file>` from a temporary file, like sh(): a script holding
 * engine.sh is past Linux's 128 KiB limit on one argument, so `sh -c` cannot start it.
 */
export function shScript(script: string, opts: { cwd?: string; input?: string; env?: Record<string, string> } = {}): Result {
  const dir = mkdtempSync(join(tmpdir(), "forge-script-"));
  try {
    writeFileSync(join(dir, "script.sh"), script);
    return sh("sh", [join(dir, "script.sh")], opts);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/**
 * A directory with a fake `claude` reporting <version>, prepended to PATH: one per
 * version and process (the first run of a freshly written script costs macOS's
 * assessment, ≈50 ms).
 */
const fakeClaudes = new Map<string, string>();
export function fakeClaude(version = "2.1.300"): Record<string, string> {
  let bin = fakeClaudes.get(version);
  if (!bin) {
    bin = mkdtempSync(join(tmpdir(), "forge-bin-"));
    writeFileSync(join(bin, "claude"), `#!/bin/sh\necho "${version} (Claude Code)"\n`);
    chmodSync(join(bin, "claude"), 0o755);
    fakeClaudes.set(version, bin);
  }
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

/** git in a test repository; without the automatic maintenance a commit starts in the background (a process per commit). */
export function git(cwd: string, ...args: string[]): Result {
  return sh("git", ["-c", "maintenance.auto=false", ...args], { cwd, env: GIT_ENV });
}

/**
 * Prepared repositories, built once and copied for each use: the runner's
 * FORGE_TEST_FIXTURES is shared by its shards (one builds a fixture, the others wait for
 * it); a process without it (bun test <file>) has its own. A fixture's content depends
 * only on its key and the build (the same Forge, packs and tools in every shard), and
 * holds no absolute path (but links to the shared TOOLS_CACHE): a copy is the
 * repository the build would make there. Forge keeps its state outside repositories,
 * by path, so a copy starts with none. A build starts from an empty directory
 * (copyRepo puts another repository there, e.g. sampleRepo(name)).
 */
const FIXTURES = (process.env.FORGE_TEST_FIXTURES ||= mkdtempSync(join(tmpdir(), "forge-fixtures-")));
export function fixture(key: string, build: (dir: string) => void): string {
  const done = join(FIXTURES, key);
  const lock = `${done}.lock`;
  while (!existsSync(done)) {
    try {
      mkdirSync(lock);
    } catch {
      // a builder that died leaves its lock: taken over
      let pid = 0;
      try {
        pid = Number(readFileSync(join(lock, "pid"), "utf8").trim());
      } catch {}
      if (pid && sh("kill", ["-0", String(pid)]).status !== 0) rmSync(lock, { recursive: true, force: true });
      else Bun.sleepSync(50);
      continue;
    }
    try {
      writeFileSync(join(lock, "pid"), `${process.pid}\n`);
      if (!existsSync(done)) {
        const tmp = mkdtempSync(`${done}.build-`);
        build(tmp);
        renameSync(tmp, done);
      }
    } finally {
      rmSync(lock, { recursive: true, force: true });
    }
  }
  const dir = mkdtempSync(join(tmpdir(), `forge-${key}-`));
  cpSync(done, dir, { recursive: true, verbatimSymlinks: true });
  return dir;
}

/** Copies the repository <from> into <dir> (symbolic links as they are). */
export function copyRepo(from: string, dir: string): void {
  cpSync(from, dir, { recursive: true, verbatimSymlinks: true });
}

/** Copies tests/generator/<name> into a fresh git repository with one commit. */
export function sampleRepo(name: string): string {
  return fixture(name, (dir) => {
    cpSync(join(ROOT, "tests", "generator", name), dir, { recursive: true });
    git(dir, "init", "-q", "-b", "main");
    git(dir, "add", "-A");
    git(dir, "commit", "-qm", "init");
  });
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
/**
 * Text Forge copies from its templates as it is, whatever the plan: each engine module (its
 * pins filled in; per set of types engines), lib.sh, the hook scripts and helpers written
 * unchanged. A snapshot shows each as one marker line, so it holds what the plan decides
 * (generated checks, rules, settings, context) and a template edit does not rewrite it.
 */
function verbatimBlocks(): [string, string][] {
  const out: [string, string][] = [];
  // each engine module as one line: a snapshot shows which modules a project gets
  for (const m of engineModules())
    for (const types of [[], ["python"], ["typescript"], ["python", "typescript"], ["typescript", "python"]] as TypesLanguage[][]) {
      const text = renderEngineModule(m.name, types);
      if (types.length && text === renderEngineModule(m.name)) continue;
      out.push([text, `@@ templates/engine/${m.name}.sh${types.length ? ` (types: ${types.join(" ")})` : ""} @@`]);
    }
  out.push([renderLib().trimEnd(), "@@ templates/scripts/lib.sh @@"]);
  for (const dir of ["scripts", "helpers"])
    for (const f of readdirSync(join(ROOT, "templates", dir))) {
      const text = readFileSync(join(ROOT, "templates", dir, f), "utf8");
      if (!text.includes("@@")) out.push([text, `@@ templates/${dir}/${f} @@\n`]);
    }
  // the longest first: a block may contain a shorter one
  return out.sort((a, b) => b[0].length - a[0].length);
}

export function snapshotOf(repo: string): string {
  const files: string[] = [];
  walk(join(repo, ".forge"), repo, files);
  walk(join(repo, ".claude"), repo, files);
  if (existsSync(join(repo, ".git", "hooks", "pre-commit"))) files.push(".git/hooks/pre-commit");
  const blocks = verbatimBlocks();
  // the generated checks are rendered into check.sh and several hook scripts: shown once,
  // in check.sh, and each other copy that is the same says so
  const fragment = (text: string) => /# ---- generated checks\n[\s\S]*?# ---- end of generated checks/.exec(text)?.[0];
  const checkSh = join(repo, ".forge", "check.sh");
  const shown = existsSync(checkSh) ? fragment(readFileSync(checkSh, "utf8")) : undefined;
  return files
    .filter((f) => !f.startsWith(".forge/state/"))
    .sort()
    .map((f) => {
      let text = blocks.reduce((t, [block, marker]) => t.replaceAll(block, marker), readFileSync(join(repo, f), "utf8"));
      if (f !== ".forge/check.sh" && shown && fragment(text) === shown) text = text.replace(shown, "@@ the generated checks of .forge/check.sh @@");
      return `===== ${f}\n${text}`;
    })
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
  return fixture(`${name}-applied`, (dir) => {
    copyRepo(sampleRepo(name), dir);
    const r = forge(["apply", "--yes"], dir);
    if (r.status !== 0) throw new Error(`forge apply failed in ${name}:\n${r.stdout}\n${r.stderr}`);
  });
}

/** Commits everything in the repository. */
export function commitAll(repo: string, message = "commit"): void {
  git(repo, "add", "-A");
  git(repo, "commit", "-qm", message);
}

/** Pinned project tools used by specs that need the real thing. */
/**
 * The first lines of a shell stand-in for a project tool: it answers the ways adapters ask a
 * version (§16.3) with one newer than every adapter was proven on, so it is driven as the
 * newest range.
 */
export const STUB_VERSION = 'case "$1" in --version|-v|-V|version) echo "stub 99.0.0"; exit 0 ;; esac\n';

export const TOOL_VERSIONS = {
  biome: "2.5.15",
  typescript: "7.0.2",
  // tsc 5.x colours diagnostics even when piped, when the tsconfig sets "pretty": true
  // (tsc 7 does not); diagnostics attribution must read that format too.
  typescript5: "5.5.4",
  /** The TypeScript versions the types engine is proved on (§15.2): one per major. */
  typescriptEngines: ["5.9.3", "6.0.3", "7.0.2"],
  ruff: "0.17.1",
  /**
   * The ruff releases its adapter is proven on (§16.5): the first covered (0.1.4), the last
   * before `concise` (0.1.15), the first with it (0.2.0), the flake8-async renumbering
   * (0.5.0), the TCH -> TC rename (0.8.0), and the newest the adapter claims.
   */
  ruffAdapters: ["0.1.4", "0.1.15", "0.2.0", "0.5.0", "0.8.0", "0.17.3"],
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
  // sql/sqlfluff runs the project's sqlfluff
  sqlfluff: "4.4.0",
  // typescript/angular/typecheck: ngc, on the TypeScript its peer range allows (>=6.0 <6.1)
  angular: "22.2.2",
  typescriptForAngular: "6.0.3",
};
/** Shared by every process of a run: the runner (tests/run.ts) passes it down with each shard's own TMPDIR. */
export const TOOLS_CACHE = process.env.FORGE_TOOLS_CACHE || join(tmpdir(), "forge-tools-v2");
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

/**
 * Infrastructure CLIs that pack checks run from PATH (§13.5), for their examples: the newest
 * releases older than 14 days, checked against each release's own checksum file. Yarn 2 and
 * later (Berry) is one JavaScript file run by node, from its npm package (checked against the
 * sha256 of that tarball): a machine usually has Yarn 1 on PATH, which a .yarnrc.yml project
 * does not use.
 */
export const SYSTEM_TOOL_VERSIONS = { terraform: "1.16.5", helm: "4.3.0", yarn: "4.18.1" };
const YARN_SHA256 = "f6729593ad4c17784e3d4ade53b3ed684e3dba3af5adb9bebe21bb854fe0b153";
/** Where ensureTools puts terraform, helm and yarn (13-packs adds it to the example tools). */
export const SYSTEM_TOOLS_BIN = join(TOOLS_CACHE, "system", "bin");

/** Downloads terraform, helm and yarn (SYSTEM_TOOL_VERSIONS) for this platform into SYSTEM_TOOLS_BIN, once. */
function ensureSystemTools(): void {
  const os = process.platform === "darwin" ? "darwin" : "linux";
  const arch = process.arch === "arm64" ? "arm64" : "amd64";
  const t = SYSTEM_TOOL_VERSIONS.terraform;
  const h = SYSTEM_TOOL_VERSIONS.helm;
  const fetch = [
    [
      "terraform",
      `https://releases.hashicorp.com/terraform/${t}/terraform_${t}_${os}_${arch}.zip`,
      `https://releases.hashicorp.com/terraform/${t}/terraform_${t}_SHA256SUMS`,
      "unzip -o -q asset terraform",
    ],
    ["helm", `https://get.helm.sh/helm-v${h}-${os}-${arch}.tar.gz`, `https://get.helm.sh/helm-v${h}-${os}-${arch}.tar.gz.sha256sum`, `tar -xzf asset && mv ${os}-${arch}/helm helm`],
    [
      "yarn",
      `https://registry.npmjs.org/@yarnpkg/cli-dist/-/cli-dist-${SYSTEM_TOOL_VERSIONS.yarn}.tgz`,
      `data:,${YARN_SHA256}`,
      `tar -xzf asset && mv package/bin/yarn.js "$3/yarn-berry.js" && printf '#!/bin/sh\\nexec node "$(dirname "$0")/yarn-berry.js" "$@"\\n' > yarn`,
    ],
  ];
  mkdirSync(SYSTEM_TOOLS_BIN, { recursive: true });
  for (const [name, url, sums, unpack] of fetch) {
    if (existsSync(join(SYSTEM_TOOLS_BIN, name!))) continue;
    const dir = mkdtempSync(join(TOOLS_CACHE, `.${name}-`));
    const script = `set -e; curl -fsSL -o asset "$1"; want=$(case $2 in (data:,*) printf '%s\\n' "\${2#data:,}" ;; (*) curl -fsSL "$2" ;; esac | awk -v f="\${1##*/}" '$2 == f || NF == 1 { print $1; exit }'); have=$( (sha256sum asset 2>/dev/null || shasum -a 256 asset) | cut -d ' ' -f 1); [ -n "$want" ] && [ "$want" = "$have" ]; ${unpack}; chmod 755 ${name}; mv -f ${name} "$3/"`;
    const r = sh("sh", ["-c", script, "sh", url!, sums!, SYSTEM_TOOLS_BIN], { cwd: dir });
    rmSync(dir, { recursive: true, force: true });
    if (r.status !== 0) throw new Error(`could not install ${name}: ${r.stderr}`);
  }
}

/** Installs Biome, TypeScript and React's types (bun), ruff (uv), and Alembic, Django and sqlfluff (uv, own venvs) once into TOOLS_CACHE, and terraform and helm (ensureSystemTools). ty is Forge's own, through uvx. */
export function ensureTools(): void {
  mkdirSync(TOOLS_CACHE, { recursive: true });
  ensureSystemTools();
  if (hasBun && !existsSync(join(TOOLS_CACHE, "node_modules/@types/react/package.json"))) {
    writeFileSync(join(TOOLS_CACHE, "package.json"), '{"name":"tools","private":true}');
    const pkgs = ["@biomejs/biome", "typescript", "react", "@types/react"] as const;
    const r = sh("bun", ["add", "-d", ...pkgs.map((p) => `${p}@${TOOL_VERSIONS[p === "@biomejs/biome" ? "biome" : p]}`)], { cwd: TOOLS_CACHE });
    if (r.status !== 0) throw new Error(`bun add failed: ${r.stderr}`);
  }
  if (!hasUv) return;
  ensureVenv(join(TOOLS_CACHE, ".venv-alembic"), "bin/alembic", [`alembic==${TOOL_VERSIONS.alembic}`, `sqlalchemy[asyncio]==${TOOL_VERSIONS.sqlalchemy}`]);
  ensureVenv(join(TOOLS_CACHE, ".venv-django"), "bin/django-admin", [`django==${TOOL_VERSIONS.django}`]);
  ensureVenv(join(TOOLS_CACHE, ".venv"), "bin/ruff", [`ruff==${TOOL_VERSIONS.ruff}`]);
  ensureVenv(join(TOOLS_CACHE, ".venv-sqlfluff"), "bin/sqlfluff", [`sqlfluff==${TOOL_VERSIONS.sqlfluff}`]);
}

/** ruff <version> in a cache directory of its own (the adapter matrix, §16.5); returns its binary. */
export function ensureRuff(version: string): string {
  const dir = join(TOOLS_CACHE, `.venv-ruff-${version}`);
  ensureVenv(dir, "bin/ruff", [`ruff==${version}`]);
  return join(dir, "bin", "ruff");
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

/**
 * Node packages installed with bun into their own directory of the tools cache (once per
 * set of packages: a marker names them); returns that directory. Packs whose checks need
 * a framework's packages (ngc) or other linters (ESLint, Prettier, oxlint) get theirs here.
 */
export function ensureNodeTools(name: string, pkgs: string[]): string {
  const dir = join(TOOLS_CACHE, name);
  const marker = join(dir, `.installed-${createHash("sha256").update(pkgs.join(" ")).digest("hex").slice(0, 12)}`);
  if (hasBun && !existsSync(marker)) {
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "package.json"), `{"name":"tools-${name}","private":true}`);
    const r = sh("bun", ["add", "-d", ...pkgs], { cwd: dir });
    if (r.status !== 0) throw new Error(`bun add failed: ${r.stderr}`);
    writeFileSync(marker, "");
  }
  return dir;
}

/** The Angular compiler with the framework packages a component imports (typescript/angular/typecheck). */
export const ANGULAR_TOOLS = [
  `@angular/compiler-cli@${TOOL_VERSIONS.angular}`,
  `@angular/compiler@${TOOL_VERSIONS.angular}`,
  `@angular/core@${TOOL_VERSIONS.angular}`,
  `typescript@${TOOL_VERSIONS.typescriptForAngular}`,
  "rxjs@7.8.2",
];

/**
 * The project tools of pack check examples beyond Biome and tsc, each set in a directory of
 * its own (ensureNodeTools): code generators (GraphQL Code Generator; wrangler from the
 * release whose `types --check` the rule needs), Prettier, ESLint and oxlint.
 */
export const EXAMPLE_NODE_TOOLS: Record<string, string[]> = {
  angular: ANGULAR_TOOLS,
  codegen: ["@graphql-codegen/cli@7.4.6", "graphql@17.0.2", "wrangler@4.153.0"],
  // other formatters and linters a project may use instead of Biome; ESLint's type-aware rules on TypeScript 6 (typescript-eslint needs < 6.1)
  lint: ["prettier@3.9.10", "eslint@10.12.0", "typescript-eslint@8.71.2", "typescript@6.0.3", "oxlint@1.88.0", "oxlint-tsgolint@7.0.2004", "eslint-plugin-testing-library@7.16.2"],
};

/** Installs EXAMPLE_NODE_TOOLS (once); returns their bin directories. */
export function ensureExampleTools(): string[] {
  return Object.entries(EXAMPLE_NODE_TOOLS).map(([name, pkgs]) => join(ensureNodeTools(name, pkgs), "node_modules", ".bin"));
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

/**
 * Stops the types engines (§15.3) running for <repo>: the adapter and, with it, its
 * language server. Tests that run hooks starting them (SessionStart, post-edit) call it
 * before they drop the state directory, so no engine outlives its test.
 */
export function stopEngines(repo: string): void {
  const dir = join(stateDir(repo), "types");
  if (!existsSync(dir)) return;
  // a start in progress (SessionStart starts engines in the background) finishes first
  for (let i = 0; i < 300 && readdirSync(dir).some((f) => f.endsWith(".lock")); i++) Bun.sleepSync(100);
  const pids = readdirSync(dir)
    .filter((lang) => !lang.endsWith(".lock"))
    .map((lang) => join(dir, lang, "pid"))
    .filter((f) => existsSync(f))
    .map((f) => readFileSync(f, "utf8").trim())
    .filter((p) => /^\d+$/.test(p));
  for (const p of pids) sh("kill", [p]);
  for (let i = 0; i < 50 && pids.some((p) => sh("kill", ["-0", p]).status === 0); i++) Bun.sleepSync(100);
}

/** The processes whose command line names <text> (pgrep -f), e.g. a repository's engines. */
export function processesNaming(text: string): string[] {
  return sh("pgrep", ["-f", text]).stdout.split("\n").filter(Boolean);
}

/**
 * The self-test of every bundled pack (scripts/test-packs.ts's), run once per test run: a
 * fixture that tests/run.ts builds while the other shards run, read by the 13-packs shards
 * (a plain `bun test` of that file builds it in its beforeAll). The cached project tools
 * come first on the tools' path, then PATH.
 */
export function packsSelfTest(): TestReport {
  ensureTools();
  ensureVueTools();
  const toolBins = [
    join(TOOLS_CACHE, "node_modules/.bin"),
    join(TOOLS_CACHE, ".venv/bin"),
    join(TOOLS_CACHE, ".venv-sqlfluff/bin"),
    join(TOOLS_CACHE, "vue/node_modules/.bin"),
    ...ensureExampleTools(),
    SYSTEM_TOOLS_BIN,
    ...(process.env.PATH ?? "").split(":"),
  ];
  const dir = fixture("packs-selftest", (d) => {
    writeFileSync(join(d, "report.json"), JSON.stringify(selfTest(join(ROOT, "packs"), undefined, { toolBins })));
  });
  return JSON.parse(readFileSync(join(dir, "report.json"), "utf8")) as TestReport;
}

/** lib.sh, then the generated checks of no check with every engine module: for running any engine helper directly. */
export function fullEngine(): string {
  return [renderLib(), "IFS='\n'", renderChecks([], [], [], undefined, "all")].join("\n");
}

/**
 * PATH without the directories that hold one of <tools>: those tools not installed, whatever
 * the machine has. A Python tool the project's environment does not have is the one on PATH
 * (§9.1), so a spec whose lint or tests are not under test runs without them.
 */
export function pathWithout(...tools: string[]): string {
  return (process.env.PATH ?? "")
    .split(":")
    .filter((d) => d && !tools.some((t) => existsSync(join(d, t))))
    .join(":");
}
