/**
 * @spec 16.5 · Tool adapters: the version matrix
 *
 * Each adapter is run against real binaries of the releases it claims: for every release of
 * its matrix (tests/adapters/<tool>.json: the oldest covered, each boundary and the newest),
 * every operation of the covering range runs through `forge_do` without a usage error and
 * exits and prints as its callers read it, an operation the range cannot do is reported as
 * not enforced, and a command line the release rejects is skipped, never a finding. A tool's
 * newest release (npm, PyPI) newer than the adapter's `verified` minor fails, so the matrix
 * is extended.
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { adapterOf, rangeOf } from "../../src/adapters";
import type { ToolAdapter } from "../../src/adapters";
import { compareVersions } from "../../src/core/versions";
import { renderEngine, renderLib } from "../../src/render";
import { git, hasBun, hasUv, ROOT, sh, TOOLS_CACHE } from "../helpers";

const T = 600_000;

/** One tool's matrix (tests/adapters/<tool>.json). */
interface Matrix {
  tool: string;
  /** How a release is installed: an npm package (with its peers), a PyPI package, a release binary (URL with {version} {os} {arch}; the member of its archive), or Forge's own pin (the engine fetches it). */
  install: { npm: string; with?: string[] } | { pypi: string; with?: string[] } | { binary: string; member?: string } | { pinned: true };
  versions: string[];
  /** The sample project. */
  files: Record<string, string>;
  /** Shell commands run in the project before the calls (after git init). */
  setup?: string;
  /** Each `forge_do <tool> <call>`: the exit statuses it may end with, an ERE its output matches. */
  calls: { call: string; exit: number[]; out?: string; input?: string; from?: string; to?: string }[];
  /** A call the release must reject as a usage error (skipped, never a finding). */
  usageProbe?: string;
  /** Operations no call runs, and why (they need a database, an account, a browser…). */
  untested?: Record<string, string>;
  /** Why the matrix cannot run on this machine at all (an absent prerequisite is checked by `needs`). */
  needs?: string[];
}

const DIR = join(ROOT, "tests", "adapters");
const MATRICES: Matrix[] = existsSync(DIR)
  ? readdirSync(DIR)
      .filter((f) => f.endsWith(".json"))
      .sort()
      .map((f) => JSON.parse(readFileSync(join(DIR, f), "utf8")) as Matrix)
  : [];

const OS = process.platform === "darwin" ? "darwin" : "linux";
const ARCH = process.arch === "arm64" ? "arm64" : "amd64";
const has = (cmd: string) => sh("sh", ["-c", `command -v ${cmd}`]).status === 0;

/** Installs <m.tool> <version> once into the tools cache; returns what the project links. */
function install(m: Matrix, version: string): { link?: [string, string]; path?: string } {
  const dir = join(TOOLS_CACHE, "adapters", `${m.tool}-${version}`);
  const i = m.install;
  if ("npm" in i) {
    if (!existsSync(join(dir, "node_modules", ".bin"))) {
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, "package.json"), '{"name":"m","private":true}\n');
      const r = sh("bun", ["add", "--exact", `${i.npm}@${version}`, ...(i.with ?? []).map((w) => w.replaceAll("{version}", version))], { cwd: dir });
      if (r.status !== 0) throw new Error(`bun add ${i.npm}@${version}: ${r.stderr}`);
      // the project's tool runs on Node, as in a project without bun.lock
      rmSync(join(dir, "bun.lock"), { force: true });
    }
    return { link: [join(dir, "node_modules"), "node_modules"] };
  }
  if ("pypi" in i) {
    if (!existsSync(join(dir, ".venv", "bin", "python"))) {
      mkdirSync(dir, { recursive: true });
      let r = sh("uv", ["venv", "-q", "--python", "3.12", join(dir, ".venv")]);
      if (r.status === 0)
        r = sh("uv", ["pip", "install", "-q", "--python", join(dir, ".venv", "bin", "python"), `${i.pypi}==${version}`, ...(i.with ?? []).map((w) => w.replaceAll("{version}", version))]);
      if (r.status !== 0) {
        rmSync(dir, { recursive: true, force: true });
        throw new Error(`uv pip install ${i.pypi}==${version}: ${r.stderr}`);
      }
    }
    return { link: [join(dir, ".venv"), ".venv"] };
  }
  if ("binary" in i) {
    const bin = join(dir, "bin", m.tool);
    if (!existsSync(bin)) {
      mkdirSync(join(dir, "bin"), { recursive: true });
      const url = i.binary.replaceAll("{version}", version).replaceAll("{os}", OS).replaceAll("{arch}", ARCH);
      const r = sh("sh", [
        "-c",
        `set -e; cd "$1"; curl -fsSL --retry 2 -o asset "$2"; case "$2" in *.zip) unzip -q -o asset ;; *.tar.gz|*.tgz) tar -xzf asset ;; *) mv asset "$3" ;; esac; m="$4"; [ -z "$m" ] || mv "$m" bin/"$3.tmp"; [ ! -f bin/"$3.tmp" ] || mv bin/"$3.tmp" bin/"$3"; [ -f bin/"$3" ] || mv "$3" bin/"$3"; chmod 755 bin/"$3"`,
        "sh",
        dir,
        url,
        m.tool,
        i.member ?? "",
      ]);
      if (r.status !== 0) throw new Error(`download ${url}: ${r.stderr}`);
    }
    return { path: join(dir, "bin") };
  }
  return {};
}

/** A git repository holding the matrix's project, with the engine's adapter functions. */
function project(m: Matrix, version: string) {
  const inst = install(m, version);
  const dir = mkdtempSync(join(tmpdir(), `forge-matrix-${m.tool}-`));
  git(dir, "init", "-q");
  for (const [f, text] of Object.entries(m.files)) {
    mkdirSync(dirname(join(dir, f)), { recursive: true });
    writeFileSync(join(dir, f), text);
  }
  if (inst.link) symlinkSync(inst.link[0], join(dir, inst.link[1]));
  const script = join(dir, ".git", "engine.sh");
  writeFileSync(script, [renderLib(), "IFS='\n'", renderEngine([], undefined, { untypedPackages: "source" })].join("\n"));
  const state = mkdtempSync(join(tmpdir(), "forge-matrix-state-"));
  // a short TMPDIR: tflint's plugins listen on a Unix socket there, whose path macOS caps at 104 bytes
  const env = { FORGE_STATE_HOME: state, TMPDIR: "/tmp", ...(inst.path ? { PATH: `${inst.path}:${process.env.PATH}` } : {}) };
  if (m.setup) {
    const r = sh("sh", ["-c", m.setup], { cwd: dir, env });
    if (r.status !== 0) throw new Error(`setup of ${m.tool} ${version}: ${r.stderr || r.stdout}`);
  }
  const tmp = mkdtempSync(join(tmpdir(), "forge-matrix-tmp-"));
  return {
    dir,
    cleanup: () => {
      for (const d of [dir, state, tmp]) rmSync(d, { recursive: true, force: true });
    },
    run(snippet: string, input?: string) {
      const r = sh("sh", ["-c", `. "${script}"; FORGE_TMP="${tmp}"; c_id=adapter/${m.tool}; rm -f "$FORGE_TMP/notes"; ${snippet}; printf '\\nforge-rc=%s\\n' "$?"`], { cwd: dir, input, env });
      let notes = "";
      try {
        notes = readFileSync(join(tmp, "notes"), "utf8");
      } catch {}
      const rc = Number(/\nforge-rc=(\d+)\n$/.exec(r.stdout)?.[1] ?? -1);
      return { rc, out: `${r.stdout.replace(/\nforge-rc=\d+\n$/, "")}${r.stderr}`, notes };
    },
  };
}

/** The operations of an adapter (every range's). */
const opsOf = (a: ToolAdapter) => [...new Set(a.ranges.flatMap((r) => Object.keys(r.ops)))].sort();

/** Whether this machine can install and run the matrix. */
function runnable(m: Matrix): string | undefined {
  if ("npm" in m.install && !hasBun) return "bun";
  if ("pypi" in m.install && !hasUv) return "uv";
  if ("binary" in m.install && !has("curl")) return "curl";
  return (m.needs ?? []).find((n) => !has(n));
}

test("every adapter has a matrix, and each matrix runs every operation of its adapter (or says why it cannot)", () => {
  const missing: string[] = [];
  for (const m of MATRICES) {
    const a = adapterOf(m.tool);
    if (!a) {
      missing.push(`${m.tool}: no adapter`);
      continue;
    }
    const called = new Set(m.calls.map((c) => c.call.split(/\s+/)[0]!));
    for (const op of opsOf(a)) if (!called.has(op) && !m.untested?.[op]) missing.push(`${m.tool} ${op}: no call`);
    if (!a.pinned && m.versions.at(-1) !== a.verified) missing.push(`${m.tool}: matrix ends at ${m.versions.at(-1)}, verified is ${a.verified}`);
    // the first release of each range that can do something, and a release of each range that can do nothing (it says so)
    for (const r of a.ranges) {
      const can = Object.values(r.ops).some((o) => o !== null);
      if (can && !a.pinned && !m.versions.includes(r.from)) missing.push(`${m.tool}: ${r.from}, where a range starts, is not in the matrix`);
      if (!can && !m.versions.some((v) => rangeOf(a, v) === r)) missing.push(`${m.tool}: no release of ${r.from}-${r.to} in the matrix`);
    }
  }
  expect(missing).toEqual([]);
});

for (const m of MATRICES) {
  const why = runnable(m);
  describe.if(!why).each(m.versions.map((v) => [v]))(`${m.tool} %s`, (version) => {
    const a = adapterOf(m.tool)!;
    let p: ReturnType<typeof project>;
    beforeAll(() => {
      p = project(m, version);
    }, T);

    test(
      "its version is read, and every operation of its range runs without a usage error and exits and prints as its callers read it",
      () => {
        const range = rangeOf(a, version)!;
        expect(range).toBeDefined();
        if (!a.pinned) expect(p.run(`forge_tool_version ${m.tool}`).out.trim()).toBe(version);
        const failures: string[] = [];
        for (const c of m.calls) {
          if (c.from && compareVersions(version, c.from) < 0) continue;
          if (c.to && compareVersions(version, c.to) >= 0) continue;
          const op = c.call.split(/\s+/)[0]!;
          const r = p.run(`forge_do ${m.tool} ${c.call}`, c.input);
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

    test.if(!!m.usageProbe)(
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
      } catch {
        return; // offline: nothing to compare with
      }
      const a = adapterOf(m.tool)!;
      expect({ tool: m.tool, latest, verified: a.verified, covered: compareVersions(minor(latest), minor(a.verified)) <= 0 }).toMatchObject({ covered: true });
    });
  }
});
