// The spec runner (`bun run test`, `bun run test:report`): runs tests/spec/*.test.ts in
// parallel `bun test` processes. Bun runs the files of one process one after the other,
// and the specs mostly wait on processes they start (hooks, check.sh, git, real tools),
// so the suite is split into shards: whole files, or, for the long ones, sets of their
// tests (selected by full name with -t), balanced on the durations of the previous run
// (node_modules/.cache/forge-spec-timings.json). Each shard is its own process with its
// own FORGE_STATE_HOME, temporary repositories and beforeAll; a test's file runs in
// every shard that has one of its tests, so its tests must not depend on each other
// (they do not: the suite also passes with --randomize).
//
//   bun tests/run.ts [--jobs N] [--junit <file>] [<bun test options>] [<spec files>…]
//
// Other options go to every `bun test` (e.g. --randomize --seed=42); FORGE_TEST_JOBS sets
// the number of processes at a time (default: the CPU count). Shared work
// that would race between processes happens once, first: the tool caches (ensureTools,
// ensureVueTools, ensureTs5, ensureExampleTools) and the bunx/uvx caches of the pinned ast-grep and Squawk.
// FORGE_PROF_DIR=<dir> keeps each shard's JUnit report there and writes shards.tsv (shard,
// file, tests, planned cost, start and wall in seconds, exit status), for profiling.
import { spawn } from "node:child_process";
import { rm } from "node:fs/promises";
import { appendFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import { join, relative } from "node:path";
import { AST_GREP_VERSION, SQUAWK_VERSION } from "../src/util";
import { ensureExampleTools, ensureTools, ensureTs5, ensureVueTools, hasBun, hasUv, ROOT, TOOLS_CACHE } from "./helpers";

const T = "300000";
const argv = process.argv.slice(2);
let jobs = Number(process.env.FORGE_TEST_JOBS) || availableParallelism();
let junit = "";
const only: string[] = [];
const passThrough: string[] = [];
for (let i = 0; i < argv.length; i++) {
  const a = argv[i]!;
  if (a === "--") continue;
  if (a === "--jobs") jobs = Number(argv[++i]);
  else if (a === "--junit") junit = argv[++i]!;
  else if (a.startsWith("-")) {
    passThrough.push(a);
    // "--seed 42": the option's value
    if (/^--[^=]+$/.test(a) && argv[i + 1] !== undefined && !argv[i + 1]!.startsWith("-") && !argv[i + 1]!.endsWith(".ts")) passThrough.push(argv[++i]!);
  } else only.push(relative(ROOT, join(process.cwd(), a)));
}
const files = only.length
  ? only
  : readdirSync(join(ROOT, "tests/spec"))
      .filter((f) => f.endsWith(".test.ts"))
      .sort()
      .map((f) => `tests/spec/${f}`);

const work = mkdtempSync(join(tmpdir(), "forge-spec-run-"));
const started = performance.now();
const secs = (ms: number) => `${(ms / 1000).toFixed(1)}s`;

// ------------------------------------------------------------------ processes

interface Exit {
  status: number;
  out: string;
}
function run(cmd: string, args: string[], env: Record<string, string | undefined> = process.env): Promise<Exit> {
  return new Promise((done) => {
    const p = spawn(cmd, args, { cwd: ROOT, env, stdio: ["ignore", "pipe", "pipe"] });
    let out = "";
    p.stdout.on("data", (d) => (out += d));
    p.stderr.on("data", (d) => (out += d));
    p.on("error", (e) => done({ status: 127, out: String(e) }));
    p.on("close", (code) => done({ status: code ?? 1, out }));
  });
}
async function pool<T, R>(items: T[], n: number, fn: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(n, items.length) }, async () => {
      while (next < items.length) {
        const i = next++;
        out[i] = await fn(items[i]!);
      }
    }),
  );
  return out;
}

/**
 * The environment of a shard: its own FORGE_STATE_HOME (tests/helpers.ts makes one) and
 * its own TMPDIR, the tool cache shared. bunx installs a package in $TMPDIR at every run
 * of `bunx -p @ast-grep/cli@<v> ast-grep`, and two at once in the same place fail
 * ("Failed to link @ast-grep/cli: EEXIST"); one shard runs one at a time.
 */
const shardTmps: string[] = [];
const removing: Promise<void>[] = [];
/** The prepared repositories of the run (tests/helpers.ts fixture), shared by the shards. */
const fixtures = join(work, "fixtures");
mkdirSync(fixtures);
function shardEnv(): Record<string, string | undefined> {
  // short: sockets under it must fit in 104 bytes
  const tmp = mkdtempSync(join(tmpdir(), "fs-"));
  shardTmps.push(tmp);
  const env: Record<string, string | undefined> = { ...process.env, FORGE_TOOLS_CACHE: TOOLS_CACHE, FORGE_TEST_FIXTURES: fixtures, TMPDIR: `${tmp}/` };
  delete env.FORGE_STATE_HOME;
  return env;
}

// ------------------------------------------------------------------ JUnit

const decode = (s: string) =>
  s.replace(/&(#x[0-9a-f]+|#\d+|lt|gt|quot|apos|amp);/gi, (_, e: string) =>
    e[0] === "#" ? String.fromCodePoint(e[1] === "x" ? parseInt(e.slice(2), 16) : Number(e.slice(1))) : ({ lt: "<", gt: ">", quot: '"', apos: "'", amp: "&" } as Record<string, string>)[e]!,
  );
const attr = (tag: string, name: string) => {
  const m = new RegExp(`\\b${name}="([^"]*)"`).exec(tag);
  return m ? decode(m[1]!) : undefined;
};

interface Case {
  /** describe names and the test name, joined with spaces: what -t matches */
  full: string;
  file: string;
  time: number;
  skipped: boolean;
  failed: boolean;
  xml: string;
}
/** The testcases of a bun JUnit report, with their full names (describe blocks, then the test). */
function parseJUnit(xml: string): Case[] {
  const cases: Case[] = [];
  const stack: string[] = [];
  const re = /<testsuite\b[^>]*?(\/?)>|<\/testsuite>|<testcase\b([^>]*?)(\/>|>([\s\S]*?)<\/testcase>)/g;
  for (let m = re.exec(xml); m; m = re.exec(xml)) {
    const tag = m[0];
    if (tag.startsWith("<testsuite")) {
      if (!m[1]) stack.push(attr(tag, "name") ?? "");
    } else if (tag === "</testsuite>") stack.pop();
    else {
      const body = m[4] ?? "";
      // stack[0] is the file's suite
      cases.push({
        full: [...stack.slice(1), attr(tag, "name") ?? ""].join(" "),
        file: (attr(tag, "file") ?? "").replace(/^\.\//, ""),
        time: Number(attr(tag, "time") ?? 0),
        skipped: /<skipped\b/.test(body),
        failed: /<(failure|error)\b/.test(body),
        xml: tag,
      });
    }
  }
  return cases;
}

// ------------------------------------------------------------------ timings

interface FileTimings {
  /** wall time of a run of the whole file minus the time of its tests: start-up, beforeAll (the least of its shards') */
  overhead: number;
  /**
   * the most of its shards': a shard that runs several describe blocks pays their
   * beforeAll each, and one that waits (engines starting) more than its tests' CPU;
   * it orders the shards, so such a shard starts early instead of ending the phase late
   */
  overheadMax?: number;
  tests: Record<string, number>;
}
const timingsPath = join(ROOT, "node_modules", ".cache", "forge-spec-timings.json");
/** The committed timings (FORGE_UPDATE_TIMINGS=1 rewrites them), for a first run on a machine. */
const seedPath = join(ROOT, "tests", "spec-timings.json");
const readJson = (p: string) => {
  try {
    return JSON.parse(readFileSync(p, "utf8")) as Record<string, FileTimings>;
  } catch {
    return undefined;
  }
};
const timings: Record<string, FileTimings> = readJson(timingsPath) ?? readJson(seedPath) ?? {};

// ------------------------------------------------------------------ shared caches

async function prepare(): Promise<void> {
  ensureTools();
  if (hasBun) {
    ensureVueTools();
    ensureTs5();
    ensureExampleTools();
  }
  // first runs of bunx/uvx install into shared caches: once here, not racing in the shards
  const warm: [string, string[]][] = [];
  if (hasBun)
    warm.push(["bunx", ["-p", `@ast-grep/cli@${AST_GREP_VERSION}`, "ast-grep", "--version"]], ["bunx", [`squawk-cli@${SQUAWK_VERSION}`, "--version"]]);
  if (hasUv)
    warm.push(["uvx", ["-q", "--from", `ast-grep-cli==${AST_GREP_VERSION}`, "ast-grep", "--version"]], ["uvx", ["-q", "--from", `squawk-cli==${SQUAWK_VERSION}`, "squawk", "--version"]]);
  await Promise.all(warm.map(([c, a]) => run(c, a)));
}

// ------------------------------------------------------------------ shards

interface Shard {
  file: string;
  /** full test names, or undefined for the whole file */
  names?: string[];
  cost: number;
}

/** The tests of each file (a run that selects none lists them all, without running a hook). */
async function listTests(file: string): Promise<Case[]> {
  const out = join(work, `list-${file.replace(/\W/g, "_")}.xml`);
  await run("bun", ["test", file, "-t", "^(?!)", "--reporter=junit", `--reporter-outfile=${out}`], shardEnv());
  return existsSync(out) ? parseJUnit(readFileSync(out, "utf8")) : [];
}

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

async function plan(): Promise<Shard[]> {
  const known = files.map((f) => timings[f]);
  const total = known.reduce((s, t) => s + (t ? t.overhead + Object.values(t.tests).reduce((a, b) => a + b, 0) : 0), 0);
  // a shard of about a third of a worker's share keeps the workers busy to the end
  const target = Math.max(4, total / jobs / 3);
  const shards: Shard[] = [];
  await pool(files, jobs, async (file) => {
    const t = timings[file];
    const testTime = t ? Object.values(t.tests).reduce((a, b) => a + b, 0) : 0;
    // splitting repeats the file's overhead in every shard: each shard's tests take at least twice as long
    const k = t ? Math.min(Math.ceil(testTime / target), Math.floor(testTime / (2 * t.overhead)), 64) : 0;
    const most = t ? (t.overheadMax ?? t.overhead) : 0;
    if (k < 2) {
      shards.push({ file, cost: t ? most + testTime : 1e6 });
      return;
    }
    const names = [...new Set((await listTests(file)).map((c) => c.full))];
    if (names.length < 2) {
      shards.push({ file, cost: most + testTime });
      return;
    }
    // longest first, each to the cheapest of k shards
    const bins = Array.from({ length: Math.min(k, names.length) }, () => ({ names: [] as string[], cost: t!.overhead }));
    const median = Object.values(t!.tests).sort((a, b) => a - b)[Math.floor(Object.keys(t!.tests).length / 2)] ?? 1;
    for (const n of names.sort((a, b) => (t!.tests[b] ?? median) - (t!.tests[a] ?? median))) {
      const bin = bins.reduce((a, b) => (b.cost < a.cost ? b : a));
      bin.names.push(n);
      bin.cost += t!.tests[n] ?? median;
    }
    for (const b of bins) shards.push({ file, names: b.names, cost: b.cost - t!.overhead + most });
  });
  return shards.sort((a, b) => b.cost - a.cost);
}

// ------------------------------------------------------------------ main

interface ShardResult {
  s: Shard;
  cases: Case[];
  wall: number;
  /** cases with the results of the tests that ran again alone (the quiet run) */
  merged?: Case[];
}
let failedShards = 0;
let shardNo = 0;
async function runShard(s: Shard, env: Record<string, string | undefined>): Promise<ShardResult> {
  const out = join(work, `shard-${shardNo++}.xml`);
  const args = ["test", "--timeout", T, ...passThrough, s.file, "--reporter=junit", `--reporter-outfile=${out}`];
  if (s.names) args.push("-t", `^(?:${s.names.map(escapeRe).join("|")})$`);
  const t0 = performance.now();
  const no = shardNo - 1;
  const tmp = (env.TMPDIR ?? "").replace(/\/$/, "");
  // profiling (FORGE_PROF_DIR): the shard's number for the exec tracer, its timeline and report kept
  const r = await run("bun", args, process.env.FORGE_PROF_DIR ? { ...env, FORGE_PROF_SHARD: `s${no}` } : env);
  const wall = (performance.now() - t0) / 1000;
  // its temporary directory goes while the other shards run (removing them all at the end took seconds)
  if (shardTmps.includes(tmp)) removing.push(rm(tmp, { recursive: true, force: true }).catch(() => {}));
  if (process.env.FORGE_PROF_DIR) {
    if (existsSync(out)) writeFileSync(join(process.env.FORGE_PROF_DIR, `shard-s${no}.xml`), readFileSync(out));
    appendFileSync(join(process.env.FORGE_PROF_DIR, "shards.tsv"), `s${no}\t${s.file}\t${s.names?.length ?? "all"}\t${s.cost.toFixed(1)}\t${((t0 - started) / 1000).toFixed(2)}\t${wall.toFixed(2)}\t${r.status}\n`);
  }
  let cases = existsSync(out) ? parseJUnit(readFileSync(out, "utf8")) : [];
  if (s.names) {
    const want = new Set(s.names);
    cases = cases.filter((c) => want.has(c.full));
  }
  const failed = r.status !== 0 || cases.some((c) => c.failed) || (s.names !== undefined && new Set(cases.map((c) => c.full)).size !== s.names.length);
  const label = `${s.file}${s.names ? ` (${s.names.length} tests)` : ""}`;
  if (failed) {
    failedShards++;
    process.stdout.write(`\n✗ ${label} [${wall.toFixed(1)}s]\n${r.out}\n`);
  } else if (process.env.FORGE_TEST_VERBOSE) process.stdout.write(`✓ ${label} [${wall.toFixed(1)}s]\n`);
  return { s, cases, wall };
}

await prepare();
const shards = await plan();
const prepared = performance.now();
if (process.env.FORGE_PROF_DIR) appendFileSync(join(process.env.FORGE_PROF_DIR, "shards.tsv"), `prepare\t-\t-\t-\t0\t${((prepared - started) / 1000).toFixed(2)}\t0\n`);
// every shard, many at a time; tests that need a quiet machine skip themselves (helpers.ts othersRunning)
const quietList = join(work, "quiet");
const quietFiles = () => (existsSync(quietList) ? new Set(readFileSync(quietList, "utf8").split("\n").filter(Boolean).map((f) => relative(ROOT, f))) : new Set<string>());
// Those tests run again, alone. Once every shard of their file has ended, the run of the
// tests it skipped starts, so its beforeAll prepares during the parallel phase; its tests
// wait for their go (FORGE_TEST_GO), given once the parallel phase has ended and every
// such run is waiting, one file at a time.
const quiet: { file: string; go: string; run: Promise<ShardResult>; ended?: boolean }[] = [];
const left = new Map<string, number>();
for (const s of shards) left.set(s.file, (left.get(s.file) ?? 0) + 1);
const ended: ShardResult[] = [];
// the self-test of every pack (minutes) is built once, alongside the shards; the 13-packs
// shards, which read it, go last and wait for it
const SELFTEST_FILE = "tests/spec/13-packs.test.ts";
const selftestReady = shards.some((s) => s.file === SELFTEST_FILE) ? run("bun", [join(ROOT, "tests", "selftest-report.ts")], shardEnv()) : Promise.resolve(undefined);
shards.sort((a, b) => Number(a.file === SELFTEST_FILE) - Number(b.file === SELFTEST_FILE));
const results = await pool(shards, jobs, async (s) => {
  if (s.file === SELFTEST_FILE) await selftestReady;
  const r = await runShard(s, { ...shardEnv(), FORGE_TEST_PHASE: "parallel", FORGE_TEST_QUIET_LIST: quietList });
  ended.push(r);
  left.set(s.file, left.get(s.file)! - 1);
  if (left.get(s.file) === 0 && quietFiles().has(s.file)) {
    const skipped = ended.flatMap((e) => (e.s.file === s.file ? e.cases.filter((c) => c.skipped) : []));
    const go = join(work, `go-${quiet.length}`);
    if (skipped.length) {
      const q: (typeof quiet)[number] = { file: s.file, go, run: runShard({ file: s.file, names: [...new Set(skipped.map((c) => c.full))], cost: 0 }, { ...shardEnv(), FORGE_TEST_PHASE: "quiet", FORGE_TEST_GO: go }) };
      void q.run.then(() => (q.ended = true));
      quiet.push(q);
    }
  }
  return r;
});
// nothing else runs: every quiet run is waiting for its go (or has ended)
while (!quiet.every((q) => q.ended || existsSync(`${q.go}.ready`))) await Bun.sleep(50);
for (const q of quiet) {
  writeFileSync(q.go, "");
  const again = await q.run;
  const done = new Map(again.cases.map((c) => [c.full, c]));
  for (const r of results) if (r.s.file === q.file) r.merged = r.cases.map((c) => (c.skipped && done.get(c.full)) || c);
}

// timings for the next plan: per test, and per file the overhead of a shard, as in the
// parallel phase (a test that runs alone afterwards is skipped there: 0)
const next: Record<string, FileTimings> = { ...timings };
for (const file of files) {
  const rs = results.filter((r) => r.s.file === file);
  const tests: Record<string, number> = {};
  for (const r of rs) for (const c of r.cases) tests[c.full] = c.time;
  const overheads = rs.map((r) => r.wall - r.cases.reduce((a, c) => a + c.time, 0));
  next[file] = { overhead: Math.max(0.3, Math.min(...overheads)), overheadMax: Math.max(0.3, ...overheads), tests };
}
try {
  mkdirSync(join(ROOT, "node_modules", ".cache"), { recursive: true });
  writeFileSync(timingsPath, JSON.stringify(next));
  const round = (x: number) => Math.round(x * 10) / 10;
  if (process.env.FORGE_UPDATE_TIMINGS)
    writeFileSync(
      seedPath,
      JSON.stringify(
        Object.fromEntries(files.map((f) => [f, { overhead: round(next[f]!.overhead), overheadMax: round(next[f]!.overheadMax!), tests: Object.fromEntries(Object.entries(next[f]!.tests).map(([k, v]) => [k, round(v)])) }])),
        null,
        1,
      ) + "\n",
    );
} catch {}

const all = results.flatMap((r) => r.merged ?? r.cases);
const pass = all.filter((c) => !c.failed && !c.skipped).length;
const skip = all.filter((c) => c.skipped).length;
const fail = all.filter((c) => c.failed).length;
if (junit) {
  const byFile = new Map<string, Case[]>();
  for (const c of all) byFile.set(c.file, [...(byFile.get(c.file) ?? []), c]);
  const suites = [...byFile]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([file, cs]) => {
      return `  <testsuite name="${file}" file="${file}" tests="${cs.length}" failures="${cs.filter((c) => c.failed).length}" skipped="${cs.filter((c) => c.skipped).length}" time="${cs.reduce((a, c) => a + c.time, 0).toFixed(3)}">\n${cs.map((c) => `    ${c.xml}`).join("\n")}\n  </testsuite>`;
    });
  mkdirSync(join(junit, ".."), { recursive: true });
  writeFileSync(junit, `<?xml version="1.0" encoding="UTF-8"?>\n<testsuites name="bun test" tests="${all.length}" failures="${fail}" skipped="${skip}" time="${((performance.now() - started) / 1000).toFixed(3)}">\n${suites.join("\n")}\n</testsuites>\n`);
}
await Promise.all(removing);
for (const d of [work, ...shardTmps]) rmSync(d, { recursive: true, force: true });
const done = performance.now();
console.log(
  `\n ${pass} pass\n ${skip} skip\n ${fail} fail\nRan ${all.length} tests across ${files.length} files in ${shards.length} processes (${jobs} at a time). [${secs(done - started)}; preparing ${secs(prepared - started)}]`,
);
process.exitCode = failedShards || fail ? 1 : 0;
