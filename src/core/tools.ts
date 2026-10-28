import { ENGINE_PYTHON, run, TY_VERSION } from "../util.js";

/**
 * The tools Forge knows by name: the audited tools a `tool` target names, the project tools
 * check commands run through `tool <name>`, Forge's own pinned ones, and the system tools
 * found on PATH. One list each; the types and the schema enums derive from them.
 */

/** Tools a `tool` target audits (§11). */
export const AUDITED_TOOLS = ["biome", "ruff", "ty", "tsc", "eslint"] as const;
export type AuditedTool = (typeof AUDITED_TOOLS)[number];

/** Project tools resolved through `tool <name>` (audited tools first). */
export const PROJECT_TOOLS = [
  "biome",
  "ruff",
  "tsc",
  "alembic",
  "pytest",
  "prisma",
  "vitest",
  "playwright",
  "astro",
  "python",
  "sqlfluff",
  "vue-tsc",
  "ngc",
  "mikro-orm",
  "graphql-codegen",
  "wrangler",
  "better-auth",
  "prettier",
  "eslint",
  "oxlint",
] as const;
/**
 * Tools resolved from the project's Python environment (the others from node_modules/.bin):
 * .venv/bin, else the environment lib.sh's forge_py_env finds (VIRTUAL_ENV, uv, Poetry, the
 * interpreter on PATH).
 */
export const PYTHON_TOOLS = new Set(["ruff", "alembic", "pytest", "python", "sqlfluff"]);
/** Forge's own pinned tools used in check commands: ty (Python's type checker, through uvx, §15.2). */
export const FORGE_TOOLS = ["ty"] as const;
/**
 * System tools, found on PATH: runtimes, package managers, and infrastructure CLIs a
 * project installs outside node_modules and .venv. A check names terraform, kustomize:
 * the engine runs the project's own (tofu in an OpenTofu project; `kubectl kustomize`
 * without kustomize). yarn counts only as Yarn 2 or later (Berry), the Yarn the
 * typescript/yarn pack is for.
 */
export const SYSTEM_TOOLS = ["uv", "uvx", "bun", "npm", "pnpm", "yarn", "docker", "terraform", "tofu", "helm", "kustomize", "kubectl"] as const;
/** System tools a check runs through an engine function that picks the binary (forge_terraform, forge_kustomize). */
export const RESOLVED_TOOLS = ["terraform", "kustomize"] as const;

/** Tools that engine helpers run, which their name in a command does not show. */
const HELPER_TOOLS: [RegExp, string][] = [
  [/\bforge_(zizmor|hadolint)\b/, "uvx"],
  [/\bforge_(trusted_deps|openapi_lint|lockfile_lint)\b/, "bun"],
  [/\bforge_tf_validate\b/, "terraform"],
  [/\bforge_helm\b/, "helm"],
  [/\bforge_sqlfluff\b/, "sqlfluff"],
];

/**
 * Package managers are words of many messages ("the pnpm field"): they count only where a
 * command starts (a line's start, after ;, &, |, (, $(, a backtick, then, do or else).
 */
const COMMAND_POSITION_ONLY = new Set<string>(["npm", "pnpm", "yarn"]);

/** Tools invoked as commands in a shell string, in order of first use. */
export function toolsInCommand(cmd: string): string[] {
  const found: { tool: string; at: number }[] = [];
  for (const tool of [...PROJECT_TOOLS, ...FORGE_TOOLS, ...SYSTEM_TOOLS]) {
    // a tool an adapter drives is also named by `forge_do <tool>` (§16.1)
    const lead = COMMAND_POSITION_ONLY.has(tool) ? "(^|[;&|(`]|\\$\\(|\\b(?:then|do|else)\\s|\\bforge_do\\s)[\\t ]*" : "(^|[\\s;&|(`]|\\$\\()";
    const m = new RegExp(`${lead}${tool}(?=$|[\\s;&|)\`])`, "m").exec(cmd);
    if (m) found.push({ tool, at: m.index });
  }
  return found.sort((a, b) => a.at - b.at).map((f) => f.tool);
}

/**
 * Project tools whose name has a "-" (no POSIX shell function can wrap them) that a
 * command runs directly instead of as `forge_exec_tool <name>`: run directly, they
 * would be whatever is on PATH, not the project's own.
 */
export function directDashedTools(cmd: string): string[] {
  const out = new Set<string>();
  for (const tool of PROJECT_TOOLS.filter((t) => t.includes("-"))) {
    const re = new RegExp(`(^|[\\s;&|(\`]|\\$\\()${tool.replace(/[.-]/g, "\\$&")}(?=$|[\\s;&|)\`])`, "g");
    for (const m of cmd.matchAll(re)) if (!/\bforge_exec_tool\s+$/.test(cmd.slice(0, m.index! + m[1]!.length))) out.add(tool);
  }
  return [...out];
}

/** The shell commands of a check target. */
export interface CheckCommands {
  run: string;
  changed_run?: string;
  fix_run?: string;
  format_stdin?: string;
  rerun?: string;
}

/** A check target's shell commands, in the order the fields are declared (absent ones as ""). */
export function commandsOf<T extends CheckCommands>(t: T): string[] {
  return [t.run, t.changed_run ?? "", t.fix_run ?? "", t.format_stdin ?? "", t.rerun ?? ""];
}

/** The tools a check target runs: named in its commands, or run by the engine helpers it calls. */
export function checkTools<T extends CheckCommands>(t: T): string[] {
  const commands = commandsOf(t);
  const helpers = HELPER_TOOLS.filter(([re]) => commands.some((c) => re.test(c))).map(([, tool]) => tool);
  return [...new Set([...commands.flatMap(toolsInCommand), ...helpers])];
}

/** Forge's pinned ty (§15.2), run through uvx: the argv before ty's own arguments. */
export const TY_COMMAND = ["uvx", "-q", "--python", ENGINE_PYTHON, "--from", `ty==${TY_VERSION}`, "ty"];

/** Whether a command is on PATH. */
export function commandExists(name: string): boolean {
  return run("sh", ["-c", `command -v ${name}`]).status === 0;
}
