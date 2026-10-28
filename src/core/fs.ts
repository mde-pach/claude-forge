import { existsSync, readFileSync, statSync } from "node:fs";
import { parse as parseToml } from "smol-toml";

/** A file's text, or undefined when it does not exist or cannot be read. */
export function readText(file: string): string | undefined {
  try {
    return existsSync(file) ? readFileSync(file, "utf8") : undefined;
  } catch {
    return undefined;
  }
}

/** A JSON file's object, or undefined when it is missing or does not parse. */
export function readJson(file: string): Record<string, any> | undefined {
  const text = readText(file);
  if (text === undefined) return undefined;
  try {
    return JSON.parse(text) as Record<string, any>;
  } catch {
    return undefined;
  }
}

/** A TOML file's table, or undefined when it is missing or does not parse. */
export function readToml(file: string): Record<string, any> | undefined {
  const text = readText(file);
  if (text === undefined) return undefined;
  try {
    return parseToml(text) as Record<string, any>;
  } catch {
    return undefined;
  }
}

/** A file's text when it is at most `max` bytes, else (or unreadable) undefined. */
export function readSmallFile(file: string, max: number): string | undefined {
  try {
    if (statSync(file).size > max) return undefined;
    return readFileSync(file, "utf8");
  } catch {
    return undefined;
  }
}
