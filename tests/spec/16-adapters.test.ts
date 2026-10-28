/**
 * @spec 16 · Tool adapters
 *
 * A rule says what it needs from a tool (`forge_do <tool> <operation>`, a code a `tool`
 * target expects); the tool's adapter says how the installed release does it, or that it
 * cannot. Each adapter is proven on real binaries of the releases it claims (the version
 * matrix in tests/helpers.ts), its code history is ruff's own, and a newer release than the
 * adapter was proven on is flagged.
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ADAPTERS, adapterOf, directPattern, rangeOf } from "../../src/adapters";
import { RUFF } from "../../src/adapters/ruff";
import { RUFF_RULES } from "../../src/adapters/ruff-rules";
import { reportsEveryFile, ruffExampleProject } from "../../src/adapters/ruff-examples";
import { compareVersions } from "../../src/core/versions";
import { loadPacks } from "../../src/packs";
import { renderEngine, renderLib, resolveEngine } from "../../src/render";
import { check } from "../../src/targets/check";
import { ensureRuff, git, hasUv, ROOT, sh, TOOL_VERSIONS } from "../helpers";
import type { CheckTarget, Rule, ToolTarget } from "../../src/types";

const checkTarget = (t: CheckTarget) => check.validate!({} as Rule, t);

const T = 300_000;

/** Every `tool: ruff` target of the shipped packs, with its rule. */
const RUFF_TARGETS: { rule: Rule; t: ToolTarget }[] = [...loadPacks(join(ROOT, "packs")).values()].flatMap((p) =>
  p.rules.flatMap((rule) => rule.enforce.filter((t): t is ToolTarget => t.target === "tool" && t.tool === "ruff").map((t) => ({ rule, t }))),
);

/** A git repository whose project ruff is <bin> (.venv/bin/ruff), with the engine's adapter functions and every code the packs expect. */
function project(bin: string): { dir: string; run: (snippet: string, input?: string) => { stdout: string; stderr: string; notes: string } } {
  const dir = mkdtempSync(join(tmpdir(), "forge-adapters-"));
  git(dir, "init", "-q");
  mkdirSync(join(dir, ".venv", "bin"), { recursive: true });
  symlinkSync(bin, join(dir, ".venv", "bin", "ruff"));
  writeFileSync(join(dir, "pyproject.toml"), '[project]\nname = "p"\nversion = "0"\n');
  writeFileSync(join(dir, "a.py"), "import os\n");
  writeFileSync(join(dir, "b.py"), "x=1\n");
  const script = join(dir, ".engine.sh");
  writeFileSync(
    script,
    [
      renderLib(),
      "IFS='\n'",
      renderEngine([], resolveEngine(["forge_do", "forge_tool_report"]), { untypedPackages: "any", toolCodes: Object.keys(RUFF_RULES).map((code) => ({ tool: "ruff", code })) }),
      "FORGE_ADAPTER_USES='ruff\tlint\tpython/lint'",
    ].join("\n"),
  );
  const tmp = mkdtempSync(join(tmpdir(), "forge-adapters-tmp-"));
  return {
    dir,
    run: (snippet, input) => {
      const r = sh("sh", ["-c", `. "${script}"; FORGE_TMP="${tmp}"; c_id=python/lint; rm -f "$FORGE_TMP/notes"; ${snippet}`], { cwd: dir, input });
      let notes = "";
      try {
        notes = readFileSync(join(tmp, "notes"), "utf8");
      } catch {}
      return { stdout: r.stdout, stderr: r.stderr, notes };
    },
  };
}

describe.if(hasUv).each(TOOL_VERSIONS.ruffAdapters.map((v) => [v]))("ruff %s", (version) => {
  let p: ReturnType<typeof project>;
  beforeAll(() => {
    p = project(ensureRuff(version));
  }, T);

  test(
    "every operation of its range runs without a usage error, and lint prints one line per diagnostic",
    () => {
      const range = rangeOf(RUFF, version)!;
      expect(range).toBeDefined();
      expect(p.run("forge_tool_version ruff").stdout.trim()).toBe(version);
      const calls: Record<string, [string, string?]> = {
        lint: ["forge_do ruff lint a.py"],
        fix: ["forge_do ruff fix b.py"],
        "lint-json": ["forge_do ruff lint-json a.py"],
        "lint-json-stdin": ["forge_do ruff lint-json-stdin a.py", "import os\n"],
        "format-check": ["forge_do ruff format-check b.py"],
        format: ["forge_do ruff format b.py"],
        "format-stdin": ["forge_do ruff format-stdin b.py", "x=1\n"],
        rules: ["forge_do ruff rules"],
        "settings-warnings": ["forge_do ruff settings-warnings ."],
      };
      expect(Object.keys(calls).sort()).toEqual([...new Set(RUFF.ranges.flatMap((r) => Object.keys(r.ops)))].sort());
      for (const [op, [cmd, input]] of Object.entries(calls)) {
        const r = p.run(cmd, input);
        if (range.ops[op] === null) {
          expect(r.notes).toContain(`not enforced: ruff ${version} cannot ${op} (it needs ruff`);
          continue;
        }
        expect({ op, notes: r.notes.replace(/^forge:python\/lint not enforced for .*\n?/m, "") }).toEqual({ op, notes: "" });
        if (op === "lint") expect(r.stdout).toMatch(/^a\.py:1:8: F401 /m);
        if (op === "lint-json-stdin") expect(JSON.parse(r.stdout).map((d: { code: string }) => d.code)).toContain("F401");
        if (op === "format-stdin") expect(r.stdout).toBe("x = 1\n");
      }
    },
    T,
  );

  test(
    "each code the rules expect is available exactly from its first stable release, under the code this release gives the rule, which reports the rule's match examples",
    () => {
      const lines = p.run("forge_tool_codes ruff").stdout.trim().split("\n");
      expect(lines.length).toBe(Object.keys(RUFF_RULES).length);
      const failures: string[] = [];
      for (const l of lines) {
        const [code, have, since] = l.split("\t") as [string, string, string];
        const available = have !== "-";
        if (available !== compareVersions(version, since) >= 0) failures.push(`${code}: ${available ? `available as ${have}` : "unavailable"} in ${version}, first stable in ${since}`);
        if (!available) continue;
        for (const { rule, t } of RUFF_TARGETS.filter((x) => x.t.expects === code)) {
          // the rule's own config (its snippet, selecting the code as this release names it), its match examples
          const { dir, files } = ruffExampleProject(rule, t, have);
          if (!files.length) continue;
          const r = sh(join(p.dir, ".venv", "bin", "ruff"), ["check", "--no-cache", "--output-format", "json", ...files], { cwd: dir });
          if (!reportsEveryFile(r.stdout, have, files))
            failures.push(`${rule.id} ${code} as ${have}: ruff ${version} does not report its match examples (${(r.stderr || r.stdout).trim().split("\n")[0]})`);
        }
      }
      expect(failures).toEqual([]);
    },
    T,
  );
});

describe("selection and reporting", () => {
  /** A fake ruff answering `--version` with <version>, and exiting 2 with clap's usage line on anything else when <usage>. */
  function fake(version: string, usage = false): ReturnType<typeof project> {
    const dir = mkdtempSync(join(tmpdir(), "forge-fake-ruff-"));
    const bin = join(dir, "ruff");
    writeFileSync(
      bin,
      `#!/bin/sh\nif [ "$1" = --version ]; then echo "ruff ${version}"; exit 0; fi\n${usage ? `echo "error: unexpected argument '--output-format' found" >&2; echo >&2; echo "For more information, try '--help'." >&2; exit 2` : "exit 0"}\n`,
      { mode: 0o755 },
    );
    return project(bin);
  }

  test("a release older than every range is not covered: the check is not enforced, and says from which release Forge drives the tool", () => {
    const p = fake("0.0.280");
    expect(p.run("forge_do ruff lint a.py").notes).toBe("forge:python/lint not enforced: no Forge adapter covers ruff 0.0.280 (Forge drives ruff 0.1.4 and later)\n");
    expect(p.run("forge_tool_report").stdout).toBe("ruff 0.0.280: no Forge adapter covers it (Forge drives ruff 0.1.4 and later): python/lint not enforced\n");
  });

  test("a release newer than the adapter was proven on is driven as the newest range, and reported", () => {
    const p = fake("9.1.0");
    expect(p.run("forge_tool_report").stdout).toContain(`ruff 9.1.0 is newer than the releases Forge's adapter was proven on (up to ${RUFF.verified}): driven as ${RUFF.verified}`);
  });

  test("a command line the release rejects is never a finding: the check is skipped with the tool's error", () => {
    const p = fake("0.5.0", true);
    const r = p.run("forge_do ruff lint a.py; echo rc=$?");
    expect(r.stdout).toContain("rc=0");
    expect(r.notes).toContain("forge:python/lint skipped: ruff 0.5.0 rejected the command (ruff check --output-format concise a.py): unexpected argument '--output-format' found");
  });

  test("a check command that runs an adapted tool itself is rejected when the rule is loaded", () => {
    expect(() => checkTarget({ target: "check", stage: "lint", run: "ruff check ." })).toThrow(/forge_do ruff <operation>/);
    expect(() => checkTarget({ target: "check", stage: "typecheck", run: "ty check" })).toThrow(/forge_do ty <operation>/);
    expect(() => checkTarget({ target: "check", stage: "lint", run: "forge_do ruff lint {files}" })).not.toThrow();
    expect(adapterOf("ty")!.pinned).toBeDefined();
  });
});

/** Shell text with its comments and single-quoted strings blanked (newlines kept): what remains are commands, double-quoted strings and their $(…). */
function shellCode(text: string): string {
  let out = "";
  let state: "code" | "single" | "double" | "comment" = "code";
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    if (c === "\n") {
      if (state === "comment") state = "code";
      out += c;
      continue;
    }
    if (state === "single") {
      if (c === "'") state = "code";
      out += " ";
    } else if (state === "comment") out += " ";
    else if (c === "\\") {
      out += c + (text[i + 1] === "\n" ? "" : (text[i + 1] ?? ""));
      if (text[i + 1] !== "\n") i++;
    } else if (state === "double") {
      if (c === '"') state = "code";
      out += c;
    } else if (c === "'") {
      state = "single";
      out += " ";
    } else if (c === '"') {
      state = "double";
      out += c;
    } else if (c === "#" && (i === 0 || /[\s;&|(]/.test(text[i - 1]!))) {
      state = "comment";
      out += " ";
    } else out += c;
  }
  return out;
}

test("the engine runs an adapted tool only through forge_do: no command of templates/ starts with one, outside the functions that are its runner", () => {
  // a runner (§16.2) is where the tool's own command line lives: forge_ty, forge_pinned_<tool> and their helpers
  const runners = /^(forge_ty|forge_pinned_[a-z_]+|astgrep_bunx|forge_hadolint_bin)$/;
  const found: string[] = [];
  for (const d of ["templates/engine", "templates/scripts"])
    for (const f of readdirSync(join(ROOT, d)).filter((x) => x.endsWith(".sh"))) {
      let fn = "";
      // comments and single-quoted text (awk programs, messages, even over several lines) are not commands of the shell
      shellCode(readFileSync(join(ROOT, d, f), "utf8"))
        .split("\n")
        .forEach((line, i) => {
          const m = /^([a-z_][a-z0-9_]*)\(\) \{/.exec(line);
          if (m) fn = m[1]!;
          if (runners.test(fn)) return;
          // a case label (`yarn)`) names the tool, it does not run it
          const code = line.replace(/^\s*[^\s()]+\)(\s|$)/, " ");
          for (const a of ADAPTERS) {
            const t = a.tool.replace(/[.-]/g, "\\$&");
            // the tool's own pattern of direct uses where it has one (bun as a runtime is not one), else its name as a command
            const re = a.direct ? directPattern(a) : new RegExp(`(^|[;&|(\`]|\\$\\(|\\b(?:then|do|else)\\s)\\s*(command\\s+)?${t}(\\s|$)`);
            if (re.test(code)) found.push(`${d}/${f}:${i + 1} ${a.tool}: ${line.trim()}`);
          }
        });
    }
  expect(found).toEqual([]);
});

describe.if(hasUv)("ruff's history", () => {
  test(
    "the code history is ruff's own: regenerated from `ruff rule --all` of each release of the matrix, it is unchanged",
    () => {
      const r = sh("bun", [join(ROOT, "scripts", "ruff-rules.ts"), "--check"], { cwd: ROOT });
      expect(r.stderr).toBe("");
      expect(r.status).toBe(0);
    },
    T,
  );

  test("ruff's newest release on PyPI is one the adapter was proven on (a newer minor needs the matrix extended)", async () => {
    let latest: string;
    try {
      const res = await fetch("https://pypi.org/pypi/ruff/json", { signal: AbortSignal.timeout(15_000) });
      latest = ((await res.json()) as { info: { version: string } }).info.version;
    } catch {
      return; // offline: nothing to compare with
    }
    const minor = (v: string) => v.split(".").slice(0, 2).join(".");
    expect({ latest, verified: RUFF.verified, covered: compareVersions(minor(latest), minor(RUFF.verified)) <= 0 }).toMatchObject({ covered: true });
  });
});
