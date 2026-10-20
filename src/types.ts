export type Level = "user" | "project";
export type Language = "python" | "typescript" | "tsx" | "any";
export type Decision = "deny" | "ask";
export type Stage = "format" | "lint" | "typecheck" | "migration" | "test";
export type ToolName = "biome" | "ruff" | "basedpyright" | "tsc";
export type AstGrepMode = "forbid" | "no-increase" | "no-decrease";

export interface DepSpec {
  ecosystem: "pypi" | "npm";
  name: string;
  version?: string;
}

export interface Pack {
  id: string;
  description: string;
  level: Level;
  requires: string[];
  detect?: { files_any?: string[]; deps_any?: DepSpec[] };
  dir: string;
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
}
export interface PermissionTarget {
  target: "permission";
  decision: Decision;
  rule: string;
}
export interface GuardTarget {
  target: "guard";
  decision: Decision;
  pattern: string;
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
  when_changed?: string[];
  requires_local_db?: boolean;
  timeout?: number;
}
export type Target = ToolTarget | AstGrepTarget | PermissionTarget | GuardTarget | SandboxTarget | CheckTarget;

export interface Rule {
  id: string;
  description: string;
  scope: { language: Language; files?: string[] };
  enforce: Target[];
  examples?: { match?: string[]; no_match?: string[] };
  deprecated?: { replaced_by: string };
  /** Absolute path of the rule file. */
  file: string;
  /** sha256 of the rule file content. */
  hash: string;
  /** "pack" or "local" (project .forge/rules). */
  origin: "pack" | "local";
  pack: string;
}

export interface ProjectConfig {
  forge: string;
  packs?: { add?: string[]; remove?: string[] };
  disable?: { id: string; reason: string }[];
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
}
