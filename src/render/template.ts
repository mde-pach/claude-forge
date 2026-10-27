/** The script templates (templates/scripts, templates/helpers) and their @@KEY@@ placeholders. */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { AST_GREP_VERSION, packageRoot } from "../util.js";

export function template(name: string): string {
  return readFileSync(join(packageRoot(), "templates", "scripts", name), "utf8");
}

/** A helper script shipped to .forge/helpers/ (templates/helpers/<name>). */
export function helperTemplate(name: string): string {
  return readFileSync(join(packageRoot(), "templates", "helpers", name), "utf8");
}

/** Replaces each @@KEY@@ once per occurrence; values are inserted literally. */
export function fill(text: string, values: Record<string, string>): string {
  return text.replace(/@@([A-Z_]+)@@/g, (m, key: string) => (key in values ? values[key]! : m));
}

export function renderLib(): string {
  return fill(template("lib.sh"), { AST_GREP_VERSION });
}
