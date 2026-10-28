/** The command line: flags, then the command. */
import { FORGE_VERSION } from "../util.js";
import { USAGE } from "../usage.js";
import { cmdApply } from "./commands/apply.js";
import { cmdDiff } from "./commands/diff.js";
import { cmdExplain } from "./commands/explain.js";
import { cmdInit } from "./commands/init.js";
import { cmdRemove } from "./commands/remove.js";
import { cmdStatus } from "./commands/status.js";
import { cmdTest } from "./commands/test.js";
import { type Flags, parseArgs } from "./flags.js";
import { ForgeError, out } from "./io.js";

/** The commands: each runs with the flags and its arguments, and returns the exit code. */
const COMMANDS: Record<string, (flags: Flags, args: string[]) => number> = {
  init: (flags) => cmdInit(flags),
  apply: (flags) => cmdApply(flags),
  diff: (flags) => cmdDiff(flags),
  status: (flags) => cmdStatus(flags),
  explain: (flags, args) => cmdExplain(args[0], flags),
  test: () => cmdTest(),
  remove: (flags) => cmdRemove(flags),
  version: () => {
    out(FORGE_VERSION);
    return 0;
  },
  help: () => {
    out(USAGE);
    return 0;
  },
};

export function main(argv: string[]): number {
  const { flags, args } = parseArgs(argv);
  const [cmd, ...rest] = args;
  if (cmd === undefined) {
    out(USAGE);
    return 2;
  }
  const command = Object.hasOwn(COMMANDS, cmd) ? COMMANDS[cmd] : undefined;
  if (!command) throw new ForgeError(`forge: unknown command ${cmd}\n\n${USAGE}`, 2);
  return command(flags, rest);
}
