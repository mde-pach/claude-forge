// Generates the pack and rule pages from packs/ (the docs never restate rule
// data by hand). Output: docs/packs/*.md and docs/reference/{rules,spec}.md,
// all gitignored. Used by the VitePress config at build time and by its dev
// server whenever a file under packs/ changes.
import { mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { loadPacks } from "../../src/packs";
import { checkTools } from "../../src/render";
import { toYaml } from "../../src/yaml";
import type { Pack, Rule, Target } from "../../src/types";

export const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
export const REPO_URL = "https://github.com/mde-pach/claude-forge";

const FENCE: Record<string, string> = { python: "python", typescript: "ts", tsx: "tsx", any: "sh" };

/** Escapes text for Markdown/Vue outside code. */
function text(s: string): string {
  return s.replace(/[<>{}|*_[\]\\]/g, (c) => `&#${c.charCodeAt(0)};`);
}
/** Inline code that survives tables (no raw pipes) and Vue. */
function code(s: string): string {
  return `<code>${s.replace(/[&<>{}|*_[\]\\`~]/g, (c) => `&#${c.charCodeAt(0)};`)}</code>`;
}
function fence(lang: string, body: string): string {
  const ticks = body.includes("```") ? "````" : "```";
  return `${ticks}${lang}\n${body.replace(/\n$/, "")}\n${ticks}`;
}
function slug(id: string): string {
  return id.replace("/", "-");
}
function source(rule: Rule): string {
  return relative(ROOT, rule.file);
}

export function targetSummary(t: Target): string {
  switch (t.target) {
    case "tool":
      return `${t.tool} ${code(t.expects)} (audited)`;
    case "ast-grep":
      return `ast-grep ${t.mode}`;
    case "permission":
      return `permission ${t.decision} ${code(t.rule)}`;
    case "guard":
      return `guard ${t.decision}`;
    case "sandbox":
      return `sandbox ${code(t.path)}`;
    case "check":
      return `check (${t.stage})`;
  }
}

function enforcementSummary(rule: Rule): string {
  const parts: string[] = [];
  for (const t of rule.enforce) {
    const s = targetSummary(t);
    if (t.target === "permission" && parts.some((p) => p.startsWith(`permission ${t.decision}`))) continue;
    parts.push(t.target === "permission" ? `permission ${t.decision}` : s);
  }
  return [...new Set(parts)].join(", ");
}

const TARGET_DOC = "/guide/authoring#targets";

function targetDetail(rule: Rule, t: Target, all: Target[]): string {
  const lines: string[] = [];
  switch (t.target) {
    case "tool": {
      const fallback = all.some((x) => x.target === "ast-grep");
      lines.push(
        `**[Tool audit](/guide/audits)**: the project's own ${code(t.tool)} must report ${code(t.expects)} for every example below that triggers, and nothing for the others.${fallback ? " If the project does not use " + t.tool + ", the ast-grep target below applies instead." : ""}`,
        "",
        `If the audit fails, \`forge status\` asks you to add:`,
        "",
        fence(t.tool === "ruff" || t.tool === "basedpyright" ? "toml" : "json", t.snippet),
      );
      break;
    }
    case "ast-grep":
      lines.push(
        `**[ast-grep](${TARGET_DOC}) ${t.mode}**: ${
          t.mode === "forbid"
            ? "any match in a changed file fails (after each edit and at Stop)."
            : t.mode === "no-increase"
              ? "a changed file may not have more matches than at `HEAD`."
              : "a changed file may not have fewer matches than at `HEAD`."
        }${rule.scope.files ? ` Files: ${rule.scope.files.map(code).join(", ")}.` : ""}`,
        "",
        fence("yaml", toYaml({ rule: t.rule, ...(t.constraints ? { constraints: t.constraints } : {}), ...(t.utils ? { utils: t.utils } : {}) })),
      );
      break;
    case "permission":
      lines.push(`**[Permission](${TARGET_DOC}) ${t.decision}**: ${code(t.rule)} in \`permissions.${t.decision}\`.`);
      break;
    case "guard":
      lines.push(`**[Guard](${TARGET_DOC}) ${t.decision}**: PreToolUse(Bash) tripwire, POSIX ERE on the JSON-escaped command:`, "", fence("regex", t.pattern));
      break;
    case "sandbox":
      lines.push(`**[Sandbox](${TARGET_DOC})**: \`sandbox.${t.path}\` in \`~/.claude/settings.json\`:`, "", fence("json", JSON.stringify(t.value)));
      break;
    case "check": {
      const meta = [
        `stage ${code(t.stage)}`,
        t.when_changed ? `only when ${t.when_changed.map(code).join(", ")} changed` : undefined,
        t.requires_local_db ? "only against a local database" : undefined,
        t.timeout ? `timeout ${t.timeout} s` : undefined,
        `tools: ${checkTools(t).map(code).join(", ") || "none"}`,
      ].filter(Boolean);
      lines.push(`**[Check](/guide/checks)** (${meta.join("; ")}). Full run (\`--all\`, CI):`, "", fence("sh", t.run));
      if (t.changed_run) lines.push("", "On changed files (`--changed`, Stop hook, pre-commit; `{files}` is the list):", "", fence("sh", t.changed_run));
      break;
    }
  }
  return lines.join("\n");
}

function examples(rule: Rule): string {
  const ex = rule.examples;
  if (!ex?.match?.length && !ex?.no_match?.length) return "";
  const kind = rule.enforce.some((t) => t.target === "guard")
    ? "sh"
    : rule.enforce.some((t) => t.target === "permission")
      ? "text"
      : FENCE[rule.scope.language]!;
  // one-line examples share a block (one per line); multi-line ones get their own
  const block = (title: string, items: string[] = []) => {
    if (!items.length) return "";
    const single = items.filter((e) => e !== "" && !e.trimEnd().includes("\n"));
    const multi = items.filter((e) => !single.includes(e));
    const blocks = [...(single.length ? [fence(kind, single.join("\n"))] : []), ...multi.map((e) => fence(kind, e || "# (empty file)"))];
    return [`**${title}**`, ...blocks].join("\n\n");
  };
  return [
    "#### Examples",
    "",
    "These are tested by `forge test` on every change to the pack.",
    "",
    block("Triggers the rule", ex.match),
    block("Does not trigger", ex.no_match),
  ].join("\n");
}

function rulePage(rule: Rule): string {
  const scope = `${code(rule.scope.language)}${rule.scope.files ? `, files ${rule.scope.files.map(code).join(", ")}` : ""}`;
  const parts = [
    `### ${rule.id} {#${slug(rule.id)}}`,
    "",
    text(rule.description),
    "",
    `<div class="forge-meta"><span>Scope: ${scope}</span><span>Enforced by: ${enforcementSummary(rule)}</span></div>`,
    "",
    rule.deprecated ? `::: warning Deprecated\nReplaced by [${rule.deprecated.replaced_by}](#${slug(rule.deprecated.replaced_by)}).\n:::\n` : "",
    ...rule.enforce.map((t) => targetDetail(rule, t, rule.enforce) + "\n"),
    examples(rule),
    "",
    `<details class="details custom-block"><summary>Rule file: ${source(rule)}</summary>`,
    "",
    fence("yaml", readFileSync(rule.file, "utf8")),
    "",
    `[View on GitHub](${REPO_URL}/blob/main/${source(rule)})`,
    "",
    "</details>",
    "",
  ];
  return parts.join("\n");
}

/** A row of the pack facts list; the value is Markdown (rendered by markdown-it inside the HTML block). */
function fact(term: string, value: string): string {
  return `<div><dt>${term}</dt><dd>\n\n${value}\n\n</dd></div>`;
}

function detection(pack: Pack): string {
  if (pack.level === "user") return "Always, at user level (`forge apply --user`).";
  const d: string[] = [];
  for (const f of pack.detect?.files_any ?? []) d.push(`file ${code(f)}`);
  for (const dep of pack.detect?.deps_any ?? []) d.push(`${dep.ecosystem} dependency ${code(dep.name + (dep.version ? ` ${dep.version}` : ""))}`);
  return d.length ? `Any of: ${d.join(", ")}.` : "Only when added in `forge.yaml`.";
}

function packPage(pack: Pack, packs: Map<string, Pack>): string {
  const requiredBy = [...packs.values()].filter((p) => p.requires.includes(pack.id)).map((p) => `[${p.id}](./${p.id})`);
  const lines = [
    "---",
    `title: ${pack.id} pack`,
    "outline: [2, 3]",
    "---",
    "",
    `# ${pack.id} <Badge type="${pack.level === "user" ? "warning" : "tip"}" text="${pack.level} level" />`,
    "",
    text(pack.description),
    "",
    '<dl class="forge-facts">',
    fact("Active when", detection(pack)),
    fact("Requires", pack.requires.length ? pack.requires.map((r) => `[${r}](./${r})`).join(", ") : "—"),
    fact("Required by", requiredBy.join(", ") || "—"),
    fact("Rules", String(pack.rules.length)),
    fact("Source", `[packs/${pack.id}](${REPO_URL}/tree/main/packs/${pack.id})`),
    "</dl>",
    "",
  ];
  if (pack.context !== undefined)
    lines.push(
      "## Context",
      "",
      `Written verbatim to \`.claude/rules/forge-${pack.id}.md\`, so Claude reads these facts in every session. Context never enforces anything.`,
      "",
      '<div class="forge-context" v-pre>',
      "",
      escapeProse(pack.context.trim()),
      "",
      "</div>",
      "",
    );
  if (pack.initDir) {
    const files = readdirRecursive(pack.initDir);
    lines.push("## Init templates", "", `\`forge init\` creates these files when they do not exist: ${files.map(code).join(", ")}.`, "");
  }
  if (pack.rules.length) {
    lines.push("## Rules", "", "| Rule | Description | Enforced by |", "|---|---|---|");
    for (const r of pack.rules) lines.push(`| [${r.id}](#${slug(r.id)}) | ${text(r.description)} | ${enforcementSummary(r)} |`);
    lines.push("", ...pack.rules.map(rulePage));
  } else lines.push("This pack contributes context only.", "");
  return lines.join("\n");
}

function readdirRecursive(dir: string, base = dir): string[] {
  return readdirSync(dir)
    .sort()
    .flatMap((f) => (statSync(join(dir, f)).isDirectory() ? readdirRecursive(join(dir, f), base) : [relative(base, join(dir, f))]));
}

function packsIndex(packs: Map<string, Pack>): string {
  const all = [...packs.values()];
  const rules = all.reduce((n, p) => n + p.rules.length, 0);
  const row = (p: Pack) =>
    `| [${p.id}](./${p.id}) | ${text(p.description)} | ${p.level === "user" ? "always (user level)" : detection(p).replace(/^Any of: /, "").replace(/\.$/, "")} | ${p.rules.length} |`;
  return [
    "---",
    "title: Packs",
    "---",
    "",
    "# Packs",
    "",
    `Forge ships **${all.length} packs** with **${rules} rules**. A pack groups the rules for one tool or framework; it is active when Forge detects it in the project (or \`forge.yaml\` adds it), and it pulls in the packs it requires.`,
    "",
    "These pages are generated from [`packs/`](" + REPO_URL + "/tree/main/packs) on every build: what you read here is exactly what `forge apply` uses.",
    "",
    "## User level",
    "",
    "| Pack | Description | Active | Rules |",
    "|---|---|---|---|",
    ...all.filter((p) => p.level === "user").map(row),
    "",
    "## Project level",
    "",
    "| Pack | Description | Detected by | Rules |",
    "|---|---|---|---|",
    ...all.filter((p) => p.level === "project").map(row),
    "",
    "See [all rules](/reference/rules) on one page, or [write your own](/guide/authoring).",
    "",
  ].join("\n");
}

function rulesIndex(packs: Map<string, Pack>): string {
  const lines = [
    "---",
    "title: All rules",
    "---",
    "",
    "# All rules",
    "",
    "Every rule Forge ships, generated from `packs/`. Rule ids are stable and never reused.",
    "",
    "| Rule | Description | Scope | Enforced by |",
    "|---|---|---|---|",
  ];
  for (const p of packs.values())
    for (const r of p.rules)
      lines.push(`| [${r.id}](/packs/${p.id}#${slug(r.id)}) | ${text(r.description)} | ${code(r.scope.language)} | ${enforcementSummary(r)} |`);
  return lines.join("\n") + "\n";
}

/** Escapes `<` outside fenced blocks and inline code, so placeholders like <rule-id> stay text. */
export function escapeProse(md: string): string {
  let inFence = false;
  return md
    .split("\n")
    .map((line) => {
      if (/^\s*(```|~~~)/.test(line)) {
        inFence = !inFence;
        return line;
      }
      if (inFence) return line;
      return line
        .split(/(`[^`]*`)/)
        .map((part, i) => (i % 2 ? part : part.replace(/</g, "&lt;")))
        .join("");
    })
    .join("\n");
}

export interface Generated {
  packs: { id: string; level: string }[];
  files: string[];
}

/** Writes the generated pages under docsDir; returns what it wrote. */
export function generate(docsDir = join(ROOT, "docs"), packsDir = join(ROOT, "packs")): Generated {
  const packs = loadPacks(packsDir);
  const files: string[] = [];
  const write = (rel: string, body: string) => {
    mkdirSync(dirname(join(docsDir, rel)), { recursive: true });
    writeFileSync(join(docsDir, rel), body);
    files.push(rel);
  };
  rmSync(join(docsDir, "packs"), { recursive: true, force: true });
  write("packs/index.md", packsIndex(packs));
  for (const p of packs.values()) write(`packs/${p.id}.md`, packPage(p, packs));
  write("reference/rules.md", rulesIndex(packs));
  const spec = escapeProse(readFileSync(join(ROOT, "SPECS.md"), "utf8"));
  write("reference/spec.md", `---\ntitle: Specification\noutline: [2, 3]\n---\n\n<div v-pre>\n\n${spec}\n\n</div>\n`);
  return { packs: [...packs.values()].map((p) => ({ id: p.id, level: p.level })), files };
}
