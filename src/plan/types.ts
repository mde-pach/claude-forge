import type { AuditedTool } from "../core/tools.js";
import type { TypesLanguage } from "../render.js";
import type { Roles, RolePattern } from "../roles.js";
import type { DesiredEntry, Level, Pack, ProjectConfig, Rule, RuleStatus, SwitchedOff } from "../types.js";
import type { ProjectFacts } from "../packs.js";

export interface PackStatus {
  id: string;
  why: string;
}

/** A file role an active rule names (§15.4), as forge status shows it. */
export interface RoleStatus {
  name: string;
  patterns: RolePattern[];
  notes: string[];
  /** The active rules naming it. */
  rules: string[];
}

/** What forge apply writes and appends at one level, and what forge status reports about it. */
export interface Plan {
  level: Level;
  base: string;
  /** Owned files, relative to base. */
  files: Map<string, string>;
  lockPath: string;
  settingsFile: string;
  settings: DesiredEntry[];
  rules: Record<string, string>;
  packs: PackStatus[];
  removedPacks: PackStatus[];
  active: RuleStatus[];
  disabled: { id: string; reason: string }[];
  /** Rules of active packs left out by their version gate (`when`), with the dependencies they need. */
  gated: { id: string; needs: string }[];
  /** Tool diagnostics an active pack switches off (pack.yaml tool_off), with the rule targets that asked for them. */
  switchedOff: SwitchedOff[];
  deprecated: { id: string; replaced_by: string }[];
  warnings: string[];
  tools: Record<AuditedTool, boolean>;
  /** Where each audited tool's audits run: "" (the root) and/or every workspace package that configures it. */
  toolDirs: Record<AuditedTool, string[]>;
  /** Project level: the checks fragment the hook scripts share (forge status runs its forge_missing_tools). */
  checksFragment?: string;
  /** The types engines the active rules use (§15.2). */
  types: TypesLanguage[];
  /** The file roles the active rules name, with their effective patterns (§15.4). */
  roles: RoleStatus[];
}

export interface PlanOptions {
  level: Level;
  /** Repository root (project) or home directory (user). */
  base: string;
  packs: Map<string, Pack>;
  /** Project only: permission entries go to .claude/settings.local.json. */
  local?: boolean;
  config?: ProjectConfig;
  localRules?: Rule[];
  facts?: ProjectFacts;
  /** The file roles' effective patterns (§15.4); default: Forge's defaults. */
  roles?: Roles;
}
