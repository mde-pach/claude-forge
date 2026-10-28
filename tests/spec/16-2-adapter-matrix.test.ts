/**
 * @spec 16.5 · Tool adapters: the version matrix
 *
 * Each adapter is run against real binaries of the releases it claims: for every release of
 * its matrix (tests/adapters/<tool>.matrix.json: the oldest covered, each boundary and the newest),
 * every operation of the covering range runs through `forge_do` without a usage error and
 * exits and prints as its callers read it, an operation the range cannot do is reported as
 * not enforced, and a command line the release rejects is skipped, never a finding. A tool's
 * newest release (npm, PyPI) newer than the adapter's `verified` minor fails, so the matrix
 * is extended.
 */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { ADAPTERS, adapterOf, rangeOf } from "../../src/adapters";
import type { ToolAdapter } from "../../src/adapters";
import { compareVersions } from "../../src/core/versions";
import { renderEngine, renderLib } from "../../src/render";
import { git, hasBun, hasUv, ROOT, sh, TOOLS_CACHE } from "../helpers";

const T = 600_000;

/** One tool's matrix (tests/adapters/<tool>.matrix.json). */
interface Matrix {
  tool: string;
  /**
   * How a release is installed: an npm package with its peers (`{version}` in a peer is the
   * release; `withFor` replaces peers per release; `inProject`: the project is the install
   * directory; `onPath`: the package's bin directory goes on PATH, for a tool found there), a
   * PyPI package, a release binary (its URL per platform, `{version}` in it and in `member`,
   * the file of the archive, per platform where it differs; `dest`, where it goes in the install directory; `node`: a script
   * run with node), or Forge's own pin (the engine fetches it).
   */
  install:
    | { npm: string; with?: string[]; withFor?: Record<string, string[]>; inProject?: boolean; onPath?: boolean }
    | { pypi: string; with?: string[]; withFor?: Record<string, string[]> }
    | { urls: Record<string, string>; member?: string | Record<string, string>; dest?: string; node?: boolean }
    | { pinned: true };
  versions: string[];
  /** Releases of a range the matrix does not run, and why (verified by hand, in the adapter's notes). */
  untestedVersions?: Record<string, string>;
  /** The sample project (with `inProject`, a package.json is merged into the install's). */
  files: Record<string, string>;
  /** Files of the sample project that differ per release (null: absent). */
  filesFor?: Record<string, Record<string, string | null>>;
  /** Environment of every run; `{install}` is the release's install directory. */
  env?: Record<string, string>;
  /** Environment of a release's runs, over `env`; and why. */
  envFor?: Record<string, Record<string, string>>;
  envWhy?: string;
  /** Shell commands run in the project before the calls (after git init). */
  setup?: string;
  /**
   * Each `forge_do <tool> <call>`: the exit statuses it may end with, an ERE its output matches;
   * `before`: the tool's own arguments run first (an install making the lock file), `write`:
   * files written first, `cwd`: the project's subdirectory it runs in.
   */
  calls: { call: string; exit: number[]; out?: string; input?: string; from?: string; to?: string; before?: string; write?: Record<string, string>; cwd?: string }[];
  /** A call the release must reject as a usage error (skipped, never a finding). */
  usageProbe?: string;
  /** The first release that rejects the probe (earlier ones ignore an unknown option, in their notes). */
  usageFrom?: string;
  /** Operations no call runs, and why (they need a database, an account, a browser…). */
  untested?: Record<string, string>;
  /** Commands the matrix needs on this machine. */
  needs?: string[];
  /** Why the peers installed with the tool's package are needed by its operations. */
  withWhy?: string;
}

const DIR = join(ROOT, "tests", "adapters");
const MATRICES: Matrix[] = existsSync(DIR)
  ? readdirSync(DIR)
      .filter((f) => f.endsWith(".matrix.json"))
      .sort()
      .map((f) => JSON.parse(readFileSync(join(DIR, f), "utf8")) as Matrix)
  : [];

/**
 * The real-binary matrix is opt-in: FORGE_ADAPTER_MATRIX=1 installs the releases it lacks
 * (CI's adapter-matrix job), and removes them when done unless FORGE_ADAPTER_MATRIX_KEEP=1.
 * Otherwise only releases already installed run; the others are listed as skipped, saying so.
 * A tool's releases share one content-addressed store (bun's cache, hardlinked into each
 * release's node_modules; uv's cache, hardlinked into each venv), removed with them once the
 * tool's releases ran; the matrix's directory stays within FORGE_ADAPTER_MATRIX_BUDGET_MB
 * (1024 by default), measured after each install (du, each hardlinked file once).
 */
const MATRIX_DIR = join(TOOLS_CACHE, "adapters");
const OPT_IN = process.env.FORGE_ADAPTER_MATRIX === "1";
const KEEP = process.env.FORGE_ADAPTER_MATRIX_KEEP === "1";
const BUDGET_MB = Number(process.env.FORGE_ADAPTER_MATRIX_BUDGET_MB ?? 1024);
/** The content-addressed store a tool's releases share: bun's and uv's caches, hardlinked into each install. */
const storeEnv = (tool: string) => ({
  BUN_INSTALL_CACHE_DIR: join(MATRIX_DIR, `.store-${tool}`, "bun"),
  UV_CACHE_DIR: join(MATRIX_DIR, `.store-${tool}`, "uv"),
  UV_LINK_MODE: "hardlink",
});
/** The matrix directory's size in MB, each hardlinked file counted once (du). */
function matrixMB(): number {
  if (!existsSync(MATRIX_DIR)) return 0;
  return Math.round(Number(sh("du", ["-sk", MATRIX_DIR]).stdout.split(/\s/)[0] ?? 0) / 1024);
}
const sizes: { after: string; mb: number }[] = [];
/** Whether <version> of <m> can run here without installing anything. */
const isCached = (m: Matrix, version: string) => "pinned" in m.install || existsSync(join(MATRIX_DIR, `${m.tool}-${version}`, ".complete"));

const PLATFORM = `${process.platform === "darwin" ? "darwin" : "linux"}-${process.arch === "arm64" ? "arm64" : "amd64"}`;
// biome-ignore lint/suspicious/noControlCharactersInRegex: the escape that starts a colour code
const ANSI = /\x1b\[[0-9;]*m/g;
const has = (cmd: string) => sh("sh", ["-c", `command -v ${cmd}`]).status === 0;

/** The peers of <version>: the matrix's, those of `withFor` replacing the same packages. */
function peers(i: { with?: string[]; withFor?: Record<string, string[]> }, version: string): string[] {
  const name = (p: string) => p.replace(/(.)[@=<>].*$/, "$1");
  const extra = i.withFor?.[version] ?? [];
  return [...(i.with ?? []).filter((p) => !extra.some((e) => name(e) === name(p))), ...extra].map((w) => w.replaceAll("{version}", version));
}

/**
 * Installs <m.tool> <version> once into the tools cache (a lock directory serialises the
 * spec runner's shards, which may install the same release at once; a venv cannot move).
 */
function install(m: Matrix, version: string): { dir: string; link?: [string, string]; path?: string; inProject?: boolean } {
  const dir = join(MATRIX_DIR, `${m.tool}-${version}`);
  // Forge's pin: the engine fetches it, nothing to install here
  if ("pinned" in m.install) return { dir };
  if (existsSync(join(dir, ".complete"))) return installed(m, version, dir);
  mkdirSync(dirname(dir), { recursive: true });
  const lock = `${dir}.lock`;
  for (let n = 0; ; n++) {
    try {
      mkdirSync(lock);
      break;
    } catch {
      if (n > 1200) throw new Error(`${m.tool} ${version}: another install holds ${lock}`);
      Bun.sleepSync(500);
    }
  }
  try {
    if (!existsSync(join(dir, ".complete"))) {
      rmSync(dir, { recursive: true, force: true });
      installed(m, version, dir);
      writeFileSync(join(dir, ".complete"), "");
    }
  } finally {
    rmSync(lock, { recursive: true, force: true });
  }
  return installed(m, version, dir);
}

/** Installs into <dir> what is not there yet; returns how a project uses it. */
function installed(m: Matrix, version: string, dir: string): { dir: string; link?: [string, string]; path?: string; inProject?: boolean } {
  const i = m.install;
  if ("npm" in i) {
    if (!existsSync(join(dir, "node_modules", ".bin"))) {
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, "package.json"), '{"name":"m","private":true}\n');
      const r = sh("bun", ["add", "--exact", "--backend=hardlink", `${i.npm}@${version}`, ...peers(i, version)], { cwd: dir, env: storeEnv(m.tool) });
      if (r.status !== 0) throw new Error(`bun add ${i.npm}@${version}: ${r.stderr}`);
      // the project's tool runs on Node, as in a project without bun.lock
      rmSync(join(dir, "bun.lock"), { force: true });
    }
    if (i.onPath) return { dir, path: join(dir, "node_modules", ".bin") };
    if (i.inProject) return { dir, inProject: true };
    return { dir, link: [join(dir, "node_modules"), "node_modules"] };
  }
  if ("pypi" in i) {
    if (!existsSync(join(dir, ".venv", "bin", "python"))) {
      mkdirSync(dir, { recursive: true });
      let r = sh("uv", ["venv", "-q", "--python", "3.12", join(dir, ".venv")], { env: storeEnv(m.tool) });
      if (r.status === 0) r = sh("uv", ["pip", "install", "-q", "--python", join(dir, ".venv", "bin", "python"), `${i.pypi}==${version}`, ...peers(i, version)], { env: storeEnv(m.tool) });
      if (r.status !== 0) {
        rmSync(dir, { recursive: true, force: true });
        throw new Error(`uv pip install ${i.pypi}==${version}: ${r.stderr}`);
      }
    }
    return { dir, link: [join(dir, ".venv"), ".venv"] };
  }
  if ("urls" in i) {
    const dest = join(dir, i.dest ?? join("bin", m.tool));
    if (!existsSync(dest)) {
      const url = i.urls[PLATFORM]!.replaceAll("{version}", version);
      const member = (typeof i.member === "string" ? i.member : (i.member?.[PLATFORM] ?? "")).replaceAll("{version}", version);
      mkdirSync(dirname(dest), { recursive: true });
      const r = sh("sh", [
        "-c",
        `set -e; d=$(mktemp -d "$1/.fetch.XXXXXX"); cd "$d"; curl -fsSL --retry 2 -o asset "$2"
        case "$2" in *.zip) unzip -q -o asset ;; *.tar.gz|*.tgz) tar -xzf asset ;; esac
        if [ -n "$3" ]; then mv "$3" "$4"; else mv asset "$4"; fi; chmod 755 "$4"; cd /; rm -rf "$d"`,
        "sh",
        dir,
        url,
        member,
        i.node ? `${dest}.js` : dest,
      ]);
      if (r.status !== 0) throw new Error(`download ${url}: ${r.stderr}`);
      if (i.node) writeFileSync(dest, `#!/bin/sh\nexec node "${dest}.js" "$@"\n`, { mode: 0o755 });
    }
    return { dir, path: dirname(dest) };
  }
  return { dir };
}

/** A git repository holding the matrix's project, with the engine's adapter functions. */
function project(m: Matrix, version: string) {
  const inst = install(m, version);
  const dir = inst.inProject ? inst.dir : mkdtempSync(join(tmpdir(), `forge-matrix-${m.tool}-`));
  if (!existsSync(join(dir, ".git"))) git(dir, "init", "-q");
  const put = (files: Record<string, string | null>) => {
    for (const [f, text] of Object.entries(files)) {
      if (text === null) {
        rmSync(join(dir, f), { force: true });
        continue;
      }
      mkdirSync(dirname(join(dir, f)), { recursive: true });
      // the install's package.json keeps its dependencies (a tool may require being one)
      const merged = inst.inProject && f === "package.json" && existsSync(join(dir, f)) ? JSON.stringify({ ...JSON.parse(readFileSync(join(dir, f), "utf8")), ...JSON.parse(text) }) : text;
      writeFileSync(join(dir, f), merged);
    }
  };
  put({ ...m.files, ...m.filesFor?.[version] });
  if (inst.link) symlinkSync(inst.link[0], join(dir, inst.link[1]));
  const script = join(dir, ".git", "engine.sh");
  writeFileSync(script, [renderLib(), "IFS='\n'", renderEngine([], undefined, { untypedPackages: "source" })].join("\n"));
  const state = mkdtempSync(join(tmpdir(), "forge-matrix-state-"));
  const env: Record<string, string> = {
    FORGE_STATE_HOME: state,
    // a short TMPDIR: tflint's plugins listen on a Unix socket there, whose path macOS caps at 104 bytes
    TMPDIR: "/tmp",
    ...(inst.path ? { PATH: `${inst.path}:${process.env.PATH}` } : {}),
    ...Object.fromEntries(Object.entries({ ...m.env, ...m.envFor?.[version] }).map(([k, v]) => [k, v.replaceAll("{install}", inst.dir)])),
  };
  if (m.setup) {
    const r = sh("sh", ["-c", m.setup], { cwd: dir, env });
    if (r.status !== 0) throw new Error(`setup of ${m.tool} ${version}: ${r.stderr || r.stdout}`);
  }
  const tmp = mkdtempSync(join(tmpdir(), "forge-matrix-tmp-"));
  return {
    dir,
    put,
    cleanup: () => {
      for (const d of [state, tmp, ...(inst.inProject ? [] : [dir])]) rmSync(d, { recursive: true, force: true });
    },
    run(snippet: string, input?: string, cwd = ".") {
      const r = sh("sh", ["-c", `. "${script}"; FORGE_TMP="${tmp}"; c_id=adapter/${m.tool}; rm -f "$FORGE_TMP/notes"; ${snippet}; printf '\\nforge-rc=%s\\n' "$?"`], {
        cwd: join(dir, cwd),
        input,
        env,
      });
      let notes = "";
      try {
        notes = readFileSync(join(tmp, "notes"), "utf8");
      } catch {}
      const rc = Number(/\nforge-rc=(\d+)\n$/.exec(r.stdout)?.[1] ?? -1);
      // without the colours some tools print even when piped
      return { rc, out: `${r.stdout.replace(/\nforge-rc=\d+\n$/, "")}${r.stderr}`.replace(ANSI, ""), notes };
    },
  };
}

/** The operations of an adapter (every range's). */
const opsOf = (a: ToolAdapter) => [...new Set(a.ranges.flatMap((r) => Object.keys(r.ops)))].sort();

/** Whether this machine can install and run the matrix. */
function runnable(m: Matrix): string | undefined {
  if ("npm" in m.install && !hasBun) return "bun";
  if ("pypi" in m.install && !hasUv) return "uv";
  if ("urls" in m.install && !has("curl")) return "curl";
  if ("urls" in m.install && !m.install.urls[PLATFORM]) return `a release for ${PLATFORM}`;
  return (m.needs ?? []).find((n) => !has(n));
}

test("every adapter has a matrix, and each matrix runs every operation of its adapter (or says why it cannot)", () => {
  const missing: string[] = [];
  // adapters proven elsewhere, and why
  const elsewhere: Record<string, string> = {
    ruff: "tests/spec/16-adapters.test.ts: its matrix and its codes' history",
    ty: "Forge's pin, run by the ty and types-engine specs (§15.2)",
    "typeorm-ts-node-esm": "the typeorm package's bin for a TypeScript data source: typeorm's ranges and operations, run through ts-node (not in the matrix)",
    "typeorm-ts-node-commonjs": "as typeorm-ts-node-esm",
  };
  for (const a of ADAPTERS) if (!MATRICES.some((m) => m.tool === a.tool) && !elsewhere[a.tool]) missing.push(`${a.tool}: no tests/adapters/${a.tool}.matrix.json`);
  // the engine's table is single-quoted and tab-separated
  for (const a of ADAPTERS)
    for (const r of a.ranges) for (const [op, args] of Object.entries(r.ops)) if (args && /['\t]/.test(args)) missing.push(`${a.tool} ${op}: a quote or a tab in its arguments`);
  for (const m of MATRICES) {
    const a = adapterOf(m.tool);
    if (!a) {
      missing.push(`${m.tool}: no adapter`);
      continue;
    }
    const called = new Set(m.calls.map((c) => c.call.split(/\s+/)[0]!));
    for (const op of opsOf(a)) if (!called.has(op) && !m.untested?.[op]) missing.push(`${m.tool} ${op}: no call`);
    if (!a.pinned && m.versions.at(-1) !== a.verified) missing.push(`${m.tool}: matrix ends at ${m.versions.at(-1)}, verified is ${a.verified}`);
    if (a.pinned) continue;
    // the releases run: the oldest covered (when its range can do something), each boundary and the release before it, the newest; no more
    const first = a.ranges[0]!;
    const can = (r: (typeof a.ranges)[number]) => Object.values(r.ops).some((o) => o !== null);
    if (can(first) && !m.versions.includes(first.from) && !m.untestedVersions?.[first.from]) missing.push(`${m.tool}: the oldest covered release ${first.from} is not in the matrix (nor said why)`);
    a.ranges.slice(1).forEach((r, i) => {
      if (!m.versions.includes(r.from) && !m.untestedVersions?.[r.from]) missing.push(`${m.tool}: the boundary ${r.from} is not in the matrix`);
      if (!m.versions.some((v) => rangeOf(a, v) === a.ranges[i])) missing.push(`${m.tool}: no release before the boundary ${r.from}`);
    });
    const most = 2 + 2 * (a.ranges.length - 1);
    if (m.versions.length > most) missing.push(`${m.tool}: ${m.versions.length} releases, at most ${most} (oldest, newest, each boundary and the one before)`);
    // the minimum installed: a peer of the tool's package only when an operation needs it, and why
    const i = m.install;
    if (("npm" in i || "pypi" in i) && (i.with?.length || Object.keys(i.withFor ?? {}).length) && !m.withWhy) missing.push(`${m.tool}: peers installed without withWhy`);
  }
  expect(missing).toEqual([]);
});

for (const m of MATRICES) {
  const why = runnable(m);
  describe(m.tool, () => {
    // after the tool's releases: their installs go (the shared store stays), and the matrix's size is noted
    afterAll(() => {
      if (!OPT_IN) return;
      if (!KEEP) for (const d of [...m.versions.map((v) => `${m.tool}-${v}`), `.store-${m.tool}`]) rmSync(join(MATRIX_DIR, d), { recursive: true, force: true });
      sizes.push({ after: m.tool, mb: matrixMB() });
    });
    for (const version of m.versions) {
      const runs = !why && (OPT_IN || isCached(m, version));
      const title = runs ? version : `${version} (${why ? `needs ${why}` : "not installed: FORGE_ADAPTER_MATRIX=1 installs and runs it"})`;
      describe.if(runs)(title, () => {
        const a = adapterOf(m.tool)!;
        let p: ReturnType<typeof project>;
        beforeAll(() => {
          p = project(m, version);
          if (OPT_IN) sizes.push({ after: `${m.tool} ${version}`, mb: matrixMB() });
        }, T);
        afterAll(() => p?.cleanup());

        test(
          "its version is read, and every operation of its range runs without a usage error and exits and prints as its callers read it (a release no range covers says so)",
          () => {
            if (!a.pinned) expect(p.run(`forge_tool_version ${m.tool}`).out.trim()).toBe(version);
            const range = rangeOf(a, version);
            if (!range) {
              const op = m.calls[0]!.call.split(/\s+/)[0]!;
              expect(p.run(`forge_do ${m.tool} ${m.calls[0]!.call}`).notes).toContain(`not enforced: no Forge adapter covers ${m.tool} ${version}`);
              expect(op).toBeDefined();
              return;
            }
            const failures: string[] = [];
            for (const c of m.calls) {
              if (c.from && compareVersions(version, c.from) < 0) continue;
              if (c.to && compareVersions(version, c.to) >= 0) continue;
              const op = c.call.split(/\s+/)[0]!;
              if (c.write) p.put(c.write);
              if (c.before) {
                // the setup install reaches the registry: one retry, and its last lines when it fails
                const setup = `( _o=$(command ${m.tool} ${c.before} 2>&1); _r=$?; [ "$_r" = 0 ] || printf '%s\\n' "$_o" | tail -n 3; exit "$_r" )`;
                let b = p.run(setup, undefined, c.cwd);
                if (b.rc !== 0) b = p.run(setup, undefined, c.cwd);
                if (b.rc !== 0) failures.push(`${m.tool} ${c.before}: exit ${b.rc}: ${b.out.trim().split("\n").join(" | ")}`);
              }
              const r = p.run(`forge_do ${m.tool} ${c.call}`, c.input, c.cwd);
              if (range.ops[op] === null) {
                if (!r.notes.includes(`not enforced: ${m.tool} ${version} cannot ${op}`)) failures.push(`${op}: expected "cannot", got ${JSON.stringify(r.notes)}`);
                continue;
              }
              if (r.notes) failures.push(`${c.call}: note ${JSON.stringify(r.notes.trim())}`);
              if (!c.exit.includes(r.rc)) failures.push(`${c.call}: exit ${r.rc}, expected ${c.exit.join(" or ")}: ${r.out.trim().split("\n").slice(0, 4).join(" | ")}`);
              if (c.out && !new RegExp(c.out, "m").test(r.out)) failures.push(`${c.call}: output does not match /${c.out}/: ${r.out.trim().split("\n").slice(0, 4).join(" | ")}`);
            }
            expect(failures).toEqual([]);
          },
          T,
        );

        // a probe on a release that can run its operation, and that rejects unknown options (usageFrom)
        const probeOp = m.usageProbe?.split(/\s+/)[0];
        const probeRange = rangeOf(a, version);
        test.if(!!m.usageProbe && !!probeRange && typeof probeRange.ops[probeOp!] === "string" && (!m.usageFrom || compareVersions(version, m.usageFrom) >= 0))(
          "a command line the release rejects is skipped with its error, never a finding",
          () => {
            const r = p.run(`forge_do ${m.tool} ${m.usageProbe}`);
            expect(r.notes).toContain(`forge:adapter/${m.tool} skipped: ${m.tool} ${a.pinned ?? version} rejected the command`);
            expect(r.rc).toBe(0);
          },
          T,
        );
      });
    }
  });
}

describe("newest releases", () => {
  const minor = (v: string) => v.split(".").slice(0, 2).join(".");
  for (const m of MATRICES) {
    const i = m.install;
    if (!("npm" in i) && !("pypi" in i)) continue;
    test(`${m.tool}'s newest release is one the adapter was proven on (a newer minor needs the matrix extended)`, async () => {
      let latest: string;
      try {
        const url = "npm" in i ? `https://registry.npmjs.org/${i.npm}/latest` : `https://pypi.org/pypi/${i.pypi}/json`;
        const res = await fetch(url, { signal: AbortSignal.timeout(15_000) });
        const body = (await res.json()) as { version?: string; info?: { version: string } };
        latest = body.version ?? body.info!.version;
        // a prerelease under the latest tag (prisma 8.0.0-rc) is not a release
        if (latest.includes("-")) return;
      } catch {
        return; // offline: nothing to compare with
      }
      const a = adapterOf(m.tool)!;
      expect({ tool: m.tool, latest, verified: a.verified, covered: compareVersions(minor(latest), minor(a.verified)) <= 0 }).toMatchObject({ covered: true });
    });
  }
});

test.if(OPT_IN)(`the matrix stays within its disk budget (${BUDGET_MB} MB, FORGE_ADAPTER_MATRIX_BUDGET_MB)`, () => {
  const peak = sizes.reduce((x, y) => (y.mb > x.mb ? y : x), { after: "start", mb: 0 });
  console.log(`adapter matrix: peak ${peak.mb} MB (after ${peak.after}), now ${matrixMB()} MB`);
  expect(peak.mb).toBeLessThanOrEqual(BUDGET_MB);
});

afterAll(() => {
  if (OPT_IN && !KEEP) rmSync(MATRIX_DIR, { recursive: true, force: true });
});
