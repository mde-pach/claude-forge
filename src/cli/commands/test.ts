import { localSelfTest, testSummary } from "../../selftest/index.js";
import { gitToplevel } from "../../util.js";
import { ForgeError, err, out } from "../io.js";

/** forge test: the project's local rules (.forge/rules); the bundled packs are tested by `bun run test:packs`. */
export function cmdTest(): number {
  const top = gitToplevel(process.cwd());
  if (!top) throw new ForgeError("forge: not inside a git repository (forge test checks the rules in .forge/rules)");
  const r = localSelfTest(top);
  if (!r.counts.rules && !r.failures.length) {
    out("forge: no local rules in .forge/rules; nothing to test");
    return 0;
  }
  for (const n of r.notes) out(`forge: ${n}`);
  for (const f of r.failures) err(`forge:test ${f}`);
  out(`forge: ${testSummary(r.counts, `${r.counts.rules} local rule(s)`)}: ${r.failures.length ? `${r.failures.length} failure(s)` : "all passed"}`);
  return r.failures.length ? 1 : 0;
}
