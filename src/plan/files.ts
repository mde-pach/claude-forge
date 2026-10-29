/** The files a plan writes (§7, §8): the plugin's hooks and scripts, check.sh, the ast-grep rules, helpers and context files. */
import { checkTools } from "../core/tools.js";
import { pluginRoot } from "../core/paths.js";
import {
  DEFAULT_ENGINE_SETTINGS,
  ENGINE_ROLES,
  type EngineSettings,
  astGrepRulePath,
  astGrepTestPath,
  fill,
  helperFiles,
  renderAstGrepRule,
  renderAstGrepTest,
  renderChecks,
  renderCheckScript,
  renderGuardRules,
  renderHooks,
  renderLib,
  renderPlugin,
  renderSgConfig,
  renderTestRules,
  template,
  typesLanguage,
  type TypesLanguage,
  wherePackages,
} from "../render/index.js";
import type { Roles } from "../roles.js";
import type { AstGrepTarget, DesiredEntry, Pack } from "../types.js";
import type { PlanContext } from "../targets/kind.js";
import type { PackStatus } from "./types.js";

/**
 * First line of every generated context file: the facts guide defaults, and an
 * explicit instruction wins (a rule followed against a clear request failed a task).
 */
export const CONTEXT_INTRO =
  "These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.\n\n";

/** `.claude/rules/` file name for a pack's context: forge-<id with "/" as "-">.md. */
export function contextFileName(packId: string): string {
  return `forge-${packId.replaceAll("/", "-")}.md`;
}

/** Permission asks that protect what Forge writes from being changed unnoticed (§8.3). */
const OWNED_ASKS = {
  user: ["Edit(~/.claude/skills/forge-user/**)", "Edit(~/.claude/forge.lock.json)"],
  project: ["Edit(.forge/**)", "Edit(forge.yaml)", "Edit(.claude/skills/forge-project/**)", "Edit(.claude/rules/forge-*.md)"],
};

const ask = (file: string, value: string): DesiredEntry => ({ file, path: "permissions.ask", value, append: true });

/** The user-level plugin (§7): the guard, the reviewer and notifications. Appends its permission asks to `settings`. */
export function userFiles(ctx: PlanContext, base: string): Map<string, string> {
  const root = pluginRoot("user");
  const files = new Map<string, string>([
    [`${root}/.claude-plugin/plugin.json`, renderPlugin("user")],
    [`${root}/hooks/hooks.json`, renderHooks("user")],
    [`${root}/scripts/lib.sh`, renderLib()],
    [`${root}/scripts/guard.sh`, template("guard.sh")],
    [`${root}/scripts/guard.rules`, renderGuardRules(ctx.out.guards, base)],
    [`${root}/scripts/notify.sh`, template("notify.sh")],
    [`${root}/scripts/review.sh`, template("review.sh")],
  ]);
  ctx.out.settings.push(...OWNED_ASKS.user.map((v) => ask(ctx.settingsFile, v)));
  return files;
}

/**
 * The project-level setup (§8): the plugin's hooks and scripts (sharing the generated
 * checks), check.sh, the ast-grep rules, helpers and context files. Prepends its
 * permission asks to `settings`.
 */
export function projectFiles(
  ctx: PlanContext,
  base: string,
  packs: { active: PackStatus[]; all: Map<string, Pack> },
  roles: { all: Roles; used: Map<string, string[]> },
  settings: EngineSettings = DEFAULT_ENGINE_SETTINGS,
): { files: Map<string, string>; fragment: string; types: TypesLanguage[] } {
  const { checks, greps, guards, astgrep } = ctx.out;
  const ids = (mode: AstGrepTarget["mode"]) =>
    astgrep
      .filter((a) => a.target.mode === mode)
      .map((a) => a.rule.id)
      .join("|");
  // the types engines the active ast-grep rules use (§15.2), and Python's when a check runs ty:
  // its engine answers the edit-time typecheck (§10.5)
  const types = [
    ...new Set([
      ...astgrep.map((a) => typesLanguage(a.rule, a.target)).filter((l): l is TypesLanguage => !!l),
      ...(checks.some((c) => checkTools(c.target).includes("ty")) ? (["python"] as const) : []),
    ]),
  ].sort();
  // forge_role knows the roles the active rules and the engine's helpers name
  const rendered: Roles = new Map([...roles.all].filter(([name]) => roles.used.has(name) || ENGINE_ROLES.includes(name)));
  // the packages the active where conditions ask about: the types engine sees them as they are (§15.2)
  settings = { ...settings, typesPackages: [...new Set(astgrep.flatMap((a) => wherePackages(a.rule, a.target)))].sort() };
  const fragment = renderChecks(checks, greps, types, rendered, "used", settings);
  const root = pluginRoot("project");
  const files = new Map<string, string>([
    [`${root}/.claude-plugin/plugin.json`, renderPlugin("project")],
    [`${root}/hooks/hooks.json`, renderHooks("project", { types: types.length > 0 })],
    [`${root}/scripts/lib.sh`, renderLib()],
    [`${root}/scripts/guard.sh`, template("guard.sh")],
    [`${root}/scripts/guard.rules`, renderGuardRules(guards, base)],
    [`${root}/scripts/review.sh`, template("review.sh")],
    // the generated checks the hook scripts source (check.sh holds its own copy: its integrity check runs it)
    [`${root}/scripts/checks.sh`, `${fragment}\n`],
    [`${root}/scripts/session-start.sh`, fill(template("session-start.sh"), { PACKS: packs.active.map((p) => p.id).join(", ") || "none" })],
    [`${root}/scripts/bash-snapshot.sh`, template("bash-snapshot.sh")],
    [`${root}/scripts/post-edit.sh`, fill(template("post-edit.sh"), { FORBID: [ids("forbid"), ids("advise")].filter(Boolean).join("|") })],
    [`${root}/scripts/typecheck.sh`, template("typecheck.sh")],
    [`${root}/scripts/stop.sh`, template("stop.sh")],
    [`${root}/scripts/test-edit.sh`, fill(template("test-edit.sh"), { TEST_RULES: renderTestRules(checks) })],
    [`${root}/scripts/turn-start.sh`, template("turn-start.sh")],
  ]);
  if (types.length) files.set(`${root}/scripts/session-end.sh`, template("session-end.sh"));
  for (const p of packs.active) {
    const context = packs.all.get(p.id)!.context;
    if (context !== undefined) files.set(`.claude/rules/${contextFileName(p.id)}`, CONTEXT_INTRO + context);
  }
  files.set(".forge/check.sh", renderCheckScript(checks, { forbid: ids("forbid"), noIncrease: ids("no-increase"), noDecrease: ids("no-decrease") }, greps, types, rendered, settings));
  files.set(".forge/.gitignore", "state/\n");
  for (const [path, text] of helperFiles(checks, types)) files.set(path, text);
  files.set(".forge/ast-grep/sgconfig.yml", renderSgConfig());
  for (const { rule, target, dirs } of astgrep) {
    files.set(`.forge/ast-grep/${astGrepRulePath(rule.id)}`, renderAstGrepRule(rule, target, dirs));
    if (rule.examples?.match?.length || rule.examples?.no_match?.length) files.set(`.forge/ast-grep/${astGrepTestPath(rule.id)}`, renderAstGrepTest(rule));
  }
  ctx.out.settings.unshift(...OWNED_ASKS.project.map((v) => ask(ctx.settingsFile, v)));
  return { files, fragment, types };
}
