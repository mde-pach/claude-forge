/** The script templates (templates/scripts, templates/helpers) and their @@KEY@@ placeholders. */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { AST_GREP_VERSION, packageRoot } from "../util.js";

/** Templates are read once per process: they ship with Forge and do not change while it runs. */
const cache = new Map<string, string>();
function read(dir: string, name: string): string {
  const key = `${dir}/${name}`;
  let text = cache.get(key);
  if (text === undefined) cache.set(key, (text = readFileSync(join(packageRoot(), "templates", dir, name), "utf8")));
  return text;
}

/** A script template (templates/scripts/<name>). */
export function template(name: string): string {
  return read("scripts", name);
}

/** A helper script shipped to .forge/helpers/ (templates/helpers/<name>). */
export function helperTemplate(name: string): string {
  return read("helpers", name);
}

/** Replaces each @@KEY@@ once per occurrence; values are inserted literally. */
export function fill(text: string, values: Record<string, string>): string {
  return text.replace(/@@([A-Z_]+)@@/g, (m, key: string) => (key in values ? values[key]! : m));
}

export function renderLib(): string {
  return fill(template("lib.sh"), { AST_GREP_VERSION });
}
