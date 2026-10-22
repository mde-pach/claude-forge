import { FORGE_VERSION } from "./util.js";

/** `forge --help`; also rendered by the docs CLI page. */
export const USAGE = `forge ${FORGE_VERSION}: generates a standalone Claude Code setup from rule packs.

Usage:
  forge init                         new project: create tool configs from pack templates, then apply
  forge apply [--user|--local]       generate or update the setup (project, user level, or local settings)
  forge diff [--user]                show what apply would change (exit 1 if anything differs)
  forge status [--user] [--json]     packs, rules (active / enforced), audits, drift, missing tools
  forge explain <rule-id>            a rule, where it is enforced here, and its audit
  forge test                         validate packs and run every rule's examples
  forge remove [--user]              delete owned files and the settings entries forge added

Options: --force (overwrite drifted files), --dry-run (print the diff, write nothing),
         --yes (confirm settings changes non-interactively)`;
