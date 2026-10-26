export type Level = "user" | "project";
export type Language = "python" | "typescript" | "tsx" | "yaml" | "any";
export type Decision = "deny" | "ask";
export type Stage = "format" | "lint" | "typecheck" | "migration" | "test";
export type ToolName = "biome" | "ruff" | "ty" | "tsc";
export type AstGrepMode = "forbid" | "no-increase" | "no-decrease" | "advise";

export interface DepSpec {
  ecosystem: "pypi" | "npm";
  name: string;
  version?: string;
}

export interface Pack {
  /** Path in the pack tree: language first, then package, then sub-package (e.g. "typescript/nextjs/react"). */
  id: string;
  description: string;
  level: Level;
  requires: string[];
  detect?: { files_any?: string[]; deps_any?: DepSpec[]; library_any?: ("pypi" | "npm")[] };
  dir: string;
  /** Parent pack id; a child is active only when its parent is. */
  parent?: string;
  children: string[];
  context?: string;
  initDir?: string;
  rules: Rule[];
}

export interface ToolTarget {
  target: "tool";
  tool: ToolName;
  expects: string;
  snippet: string;
}
export interface AstGrepTarget {
  target: "ast-grep";
  mode: AstGrepMode;
  rule: Record<string, unknown>;
  // Optional ast-grep keys passed through (constraints, utils, transform).
  constraints?: Record<string, unknown>;
  utils?: Record<string, unknown>;
  /** Engine capabilities on metavariables (§15.1): a match counts only when they hold. */
  where?: Record<string, WhereCondition>;
}

/**
 * What the static type of an expression must be (§15.1): declared, or a subtype of a
 * type declared, in a package or module, optionally with one of the given names (the
 * declared name, never an import alias). Never a name alone.
 */
export interface TypeSpec {
  package?: string | string[];
  module?: string | string[];
  name?: string | string[];
  /** A type the checker does not know (Any, Unknown, any, unknown): no-match (default) or match. */
  unknown?: "match" | "no-match";
}
export type WhereCondition = { type: TypeSpec } | { not: WhereCondition };
export interface PermissionTarget {
  target: "permission";
  decision: Decision;
  rule: string;
}
/** Built-in guard checks (guard_check_<name> in guard.sh): they decide on the parsed command. */
export type GuardCheck = "ephemeral-exec";
export interface GuardTarget {
  target: "guard";
  decision: Decision;
  /** POSIX ERE on the JSON-escaped command; with `check`, a pre-filter. */
  pattern: string;
  /** A built-in check that decides when `pattern` matches. */
  check?: GuardCheck;
}
export interface SandboxTarget {
  target: "sandbox";
  path: string;
  value: unknown;
}
export interface CheckTarget {
  target: "check";
  stage: Stage;
  run: string;
  changed_run?: string;
  /** Lint checks: safe fixes applied to an edited file before it is linted ({files}). */
  fix_run?: string;
  /** The formatter as a filter: formats stdin as if it were {file}, prints the result. Formats base copies, so formatting is not counted as added lines. */
  format_stdin?: string;
  /** Test checks: runs only the given test files ({files}); Stop re-runs failing tests with it at the start of the turn. */
  rerun?: string;
  when_changed?: string[];
  requires_local_db?: boolean;
  /** warning diagnostics count like errors (§10.1) */
  fail_on_warnings?: true;
  timeout?: number;
}
/** A POSIX ERE matched line by line in the rule's files (comments, config text, .astro files). */
export interface GrepTarget {
  target: "grep";
  mode: AstGrepMode;
  pattern: string;
  /** Match regardless of case (grep -i). */
  ignore_case?: boolean;
}
export type Target = ToolTarget | AstGrepTarget | PermissionTarget | GuardTarget | SandboxTarget | CheckTarget | GrepTarget;

/**
 * A rule example. A string is code written to a file inside the rule's scope.
 * A fixture (check rules only) is a set of files: `head` is committed first,
 * then `files` are written on top as the change being checked.
 */
export type Example = string | ExampleFixture | GuardExample;
/** A guard example run in a directory holding `files` (e.g. node_modules/.bin/<tool>), its cwd. */
export interface GuardExample {
  command: string;
  files?: Record<string, string>;
}
export interface ExampleFixture {
  files: Record<string, string>;
  head?: Record<string, string>;
}

export interface Rule {
  id: string;
  description: string;
  scope: { language: Language; files?: string[] };
  enforce: Target[];
  /** deps: the packages (exact versions) the examples are type-checked against, for rules with type constraints (§15.1). */
  examples?: { match?: Example[]; no_match?: Example[]; deps?: DepSpec[] };
  deprecated?: { replaced_by: string };
  /** The rule is active only where every listed dependency is declared in its version range (a version gate). */
  when?: { deps: DepSpec[] };
  /** For an ask: the one question an independent reviewer answers when nobody can (§9.3). */
  review?: string;
  /** Absolute path of the rule file. */
  file: string;
  /** sha256 of the rule file content. */
  hash: string;
  /** "pack" or "local" (project .forge/rules). */
  origin: "pack" | "local";
  /** Pack id: the rule id without its last segment. */
  pack: string;
}

export interface ProjectConfig {
  forge: string;
  packs?: { add?: string[]; remove?: string[] };
  disable?: { id: string; reason: string }[];
  /** Config-ratchet findings the user accepted (the exact line check.sh printed). */
  accept?: { rule: string; change: string; reason: string }[];
}

export interface SettingsEntry {
  file: string;
  path: string;
  value: unknown;
}

export interface Lock {
  forge_version: string;
  claude_code_min: string;
  rules: Record<string, string>;
  files: Record<string, string>;
  settings: SettingsEntry[];
  /** Blocks Forge added to files it does not own (a hook manager's config); written only when there is one. */
  blocks: LockBlock[];
}

/** Forge's marked block in a file it does not own: the pre-commit check in a hook manager (§10.3). */
export interface LockBlock {
  file: string;
  kind: "pre-commit" | "husky" | "lefthook" | "hooks";
  /** Forge created the file: remove deletes it when nothing else is left. */
  created: boolean;
  /** blockHash() of the block as written. */
  hash: string;
}
