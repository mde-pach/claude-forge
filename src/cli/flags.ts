/** The command line: forge <command> [args] [--user | --local] [--force] [--dry-run] [--yes] [--json] [--pre-commit]. */
import { USAGE } from "../usage.js";
import { ForgeError } from "./io.js";

export interface Flags {
  user: boolean;
  local: boolean;
  force: boolean;
  dryRun: boolean;
  yes: boolean;
  json: boolean;
  /** Consent to add the commit-time check to a hook manager's (tracked) config. */
  preCommit: boolean;
}

/** The flags and the command words of argv. */
export function parseArgs(argv: string[]): { flags: Flags; args: string[] } {
  const flags: Flags = { user: false, local: false, force: false, dryRun: false, yes: false, json: false, preCommit: false };
  const args: string[] = [];
  for (const a of argv) {
    if (a === "--user") flags.user = true;
    else if (a === "--local") flags.local = true;
    else if (a === "--force") flags.force = true;
    else if (a === "--dry-run") flags.dryRun = true;
    else if (a === "--yes" || a === "-y") flags.yes = true;
    else if (a === "--json") flags.json = true;
    else if (a === "--pre-commit") flags.preCommit = true;
    else if (a === "--help" || a === "-h") args.unshift("help");
    else if (a === "--version" || a === "-v") args.unshift("version");
    else if (a.startsWith("-")) throw new ForgeError(`forge: unknown option ${a}\n\n${USAGE}`, 2);
    else args.push(a);
  }
  if (flags.user && flags.local) throw new ForgeError("forge: --user and --local cannot be combined", 2);
  return { flags, args };
}
