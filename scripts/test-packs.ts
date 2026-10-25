// The development self-test of the bundled packs (bun run test:packs [packs-dir]): every
// pack and rule validated against the schemas and every example run (SPECS §14.1).
// Not part of the published CLI: `forge test` checks a project's local rules only.
import { join } from "node:path";
import { selfTest, testSummary } from "../src/selftest.ts";

const dir = process.argv[2] ?? join(import.meta.dir, "..", "packs");
const r = selfTest(dir);
for (const n of r.notes) console.log(`test:packs: ${n}`);
for (const f of r.failures) console.error(`forge:test ${f}`);
console.log(`test:packs: ${testSummary(r.counts, `${r.counts.packs} packs, ${r.counts.rules} rules`)}: ${r.failures.length ? `${r.failures.length} failure(s)` : "all passed"}`);
process.exitCode = r.failures.length ? 1 : 0;
