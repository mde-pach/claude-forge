/** forge init: create the stack's missing tool configs, then apply. */
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { plan, readProjectInputs, type Plan } from "../../plan/index.js";
import { gitToplevel } from "../../util.js";
import { applyEdits, modify } from "jsonc-parser/lib/esm/main.js";
import { cmdApply } from "../commands/apply.js";
import { claudeGate, shippedPacks } from "../context.js";
import type { Flags } from "../flags.js";
import { ForgeError, out } from "../io.js";

export function cmdInit(flags: Flags): number {
  if (flags.user) throw new ForgeError("forge: init is project-level only", 2);
  claudeGate();
  const top = gitToplevel(process.cwd());
  if (!top) throw new ForgeError("forge: not inside a git repository (run git init first)");
  if (existsSync(join(top, ".forge", "forge.lock.json"))) throw new ForgeError("forge: init is for new projects; this one already has forge (use forge apply)");
  const packs = shippedPacks();
  const p = plan({ level: "project", base: top, packs, ...readProjectInputs(top) });
  for (const pk of p.packs) {
    const dir = packs.get(pk.id)!.initDir;
    if (!dir) continue;
    for (const f of readdirSync(dir, { recursive: true }).map(String).sort()) {
      const src = join(dir, f);
      if (statSync(src).isDirectory()) continue;
      const dest = join(top, f);
      if (existsSync(dest)) out(`forge: ${f} exists, kept`);
      else if (flags.dryRun) out(`forge: would create ${f} (from ${pk.id})`);
      else {
        mkdirSync(dirname(dest), { recursive: true });
        const off = initSwitchOffs(f, readFileSync(src, "utf8"), p.switchedOff);
        if (off === undefined) cpSync(src, dest);
        else writeFileSync(dest, off);
        out(`forge: created ${f} (from ${pk.id})`);
      }
    }
  }
  return cmdApply(flags);
}

/**
 * An init template with the diagnostics active packs switch off (pack.yaml tool_off) set to
 * "off", the rest of the file as it is; undefined when none concerns it. Biome:
 * biome.json's linter.rules.<group>.<name>.
 */
export function initSwitchOffs(file: string, text: string, offs: Plan["switchedOff"]): string | undefined {
  if (!/(^|\/)biome\.jsonc?$/.test(file)) return undefined;
  const mine = offs.filter((o) => o.tool === "biome");
  if (!mine.length) return undefined;
  let out = text;
  for (const o of mine) {
    const [, group, name] = o.expects.split("/");
    out = applyEdits(out, modify(out, ["linter", "rules", group!, name!], "off", { formattingOptions: { insertSpaces: true, tabSize: 2 } }));
  }
  return out;
}
