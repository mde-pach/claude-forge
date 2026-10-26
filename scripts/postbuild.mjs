// Adds the node shebang to dist/forge.js, makes it executable, writes the JSON Schemas
// shipped in the package (schemas/*.schema.json) and the bundled packs (dist/packs.json:
// the packs as loadPacks reads and validates them, which the CLI reads).
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "dist", "forge.js");
let text = readFileSync(out, "utf8");
if (text.startsWith("#!")) text = text.slice(text.indexOf("\n") + 1);
writeFileSync(out, "#!/usr/bin/env node\n" + text);
chmodSync(out, 0o755);

const { packSchema, ruleSchema, projectConfigSchema } = await import(join(root, "src", "schemas.ts")).catch(() => ({}));
if (packSchema) {
  mkdirSync(join(root, "schemas"), { recursive: true });
  for (const [name, schema] of [["pack", packSchema], ["rule", ruleSchema], ["forge", projectConfigSchema]])
    writeFileSync(join(root, "schemas", `${name}.schema.json`), JSON.stringify(schema, null, 2) + "\n");
}

const { packBundle } = await import(join(root, "src", "packs.ts"));
writeFileSync(join(root, "dist", "packs.json"), packBundle(join(root, "packs")));
