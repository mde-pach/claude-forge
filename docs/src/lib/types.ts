// The shape of `claude-forge packs dump --json` (the Rust loader's model, crates/forge-packs),
// as far as the site reads it. schemas/*.json is the authority on the YAML.

export type Ecosystem = "npm" | "pypi";
export interface DepSpec {
  ecosystem: Ecosystem;
  name: string;
  version?: string;
}

export interface ContentMatch {
  files: string[];
  pattern: string;
}

export interface Detect {
  files_any?: string[];
  deps_any?: DepSpec[];
  library_any?: Ecosystem[];
  always?: true;
  sources_any?: string[];
  content_any?: ContentMatch[];
  yaml_keys?: string[];
  facts_any?: Record<string, string | string[]>;
}

export interface ContextWhen {
  deps?: DepSpec[];
  files?: string[];
  facts?: Record<string, string | string[]>;
  packs?: string[];
}

export interface ContextItem {
  id?: string;
  text: string;
  when?: ContextWhen;
}

export type Mode = "forbid" | "advise" | "no-increase" | "no-decrease";

export interface ToolTarget {
  target: "tool";
  tool: string;
  expects: string;
  snippet: string;
}
export interface AstGrepTarget {
  target: "ast-grep";
  mode: Mode;
  rule: unknown;
  constraints?: unknown;
  utils?: unknown;
  where?: unknown;
  needs?: DepSpec[];
}
export interface PermissionTarget {
  target: "permission";
  decision: "deny" | "ask";
  rule: string;
}
export interface GuardTarget {
  target: "guard";
  decision: "deny" | "ask";
  check?: string;
  command?: unknown;
  reason?: string;
}
export interface SandboxTarget {
  target: "sandbox";
  path: string;
  value: unknown;
}
export interface CheckTarget {
  target: "check";
  stage: string;
  run: string;
  changed_run?: string;
  fix_run?: string;
  format_stdin?: string;
  rerun?: string;
  when_changed?: string[];
  requires_local_db?: boolean;
  timeout?: number;
}
export interface GrepTarget {
  target: "grep";
  mode: Mode;
  pattern: string;
  ignore_case?: boolean;
}
export type Target = ToolTarget | AstGrepTarget | PermissionTarget | GuardTarget | SandboxTarget | CheckTarget | GrepTarget;

export type Example = string | { files: Record<string, string>; head?: Record<string, string> } | { command: string; files?: Record<string, string> };

export interface Rule {
  id: string;
  description: string;
  scope: { language: string; files?: string[] };
  enforce: Target[];
  examples?: { match?: Example[]; no_match?: Example[]; deps?: DepSpec[] };
  when?: { deps?: DepSpec[] };
  review?: string;
  deprecated?: { replaced_by: string };
  /** Relative to packs/. */
  file: string;
  pack: string;
}

export interface Pack {
  id: string;
  description: string;
  level: "project" | "user";
  detect?: Detect;
  requires: string[];
  /** Relative to packs/. */
  dir: string;
  parent: string | null;
  children: string[];
  /** Relative to packs/, when the pack has init templates. */
  initDir: string | null;
  context?: ContextItem[];
  tool_off?: { tool: string; expects: string; reason: string }[];
  rules: Rule[];
}

export interface CliHelp {
  /** "" for the top level, else the subcommand ("apply", "packs dump"). */
  command: string;
  help: string;
}
