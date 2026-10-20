// The docs pages are generated from packs/: every pack and rule must appear.
import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { escapeProse, generate } from "../docs/.vitepress/packs";
import { loadPacks } from "../src/packs";
import { ROOT } from "./helpers";

test("every pack gets a page and every rule a section", () => {
  const out = mkdtempSync(join(tmpdir(), "forge-docs-"));
  const g = generate(out);
  const packs = loadPacks(join(ROOT, "packs"));
  expect(g.packs.map((p) => p.id).sort()).toEqual([...packs.keys()].sort());
  const index = readFileSync(join(out, "reference/rules.md"), "utf8");
  for (const p of packs.values()) {
    const page = readFileSync(join(out, `packs/${p.id}.md`), "utf8");
    expect(page).toContain(`# ${p.id} `);
    for (const r of p.rules) {
      expect(page).toContain(`### ${r.id} {#${r.id.replace("/", "-")}}`);
      expect(index).toContain(`[${r.id}](/packs/${p.id}#`);
      for (const ex of r.examples?.match ?? []) for (const line of ex.split("\n").filter(Boolean)) expect(page).toContain(line);
    }
    if (p.context) expect(page).toContain(`.claude/rules/forge-${p.id}.md`);
  }
  expect(readFileSync(join(out, "reference/spec.md"), "utf8")).toContain("Forge — Build Specification");
});

test("prose placeholders are escaped, code is not", () => {
  expect(escapeProse("Run `forge explain <rule-id>` for <rule-id>.\n```\n<keep>\n```\n")).toBe(
    "Run `forge explain <rule-id>` for &lt;rule-id>.\n```\n<keep>\n```\n",
  );
});
