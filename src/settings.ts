import { applyEdits, modify, parse, type FormattingOptions, type JSONPath, type ParseError } from "jsonc-parser/lib/esm/main.js";
import type { SettingsEntry } from "./types.js";
import type { DesiredEntry } from "./plan.js";

const DEFAULT_FORMAT: FormattingOptions = { tabSize: 2, insertSpaces: true, eol: "\n" };

/** The indentation an existing file uses (tabs or N spaces), so appended entries match it. */
export function detectFormatting(text: string): FormattingOptions {
  const indents = text.split("\n").map((l) => /^([ \t]+)\S/.exec(l)?.[1]).filter((x): x is string => !!x);
  if (!indents.length) return DEFAULT_FORMAT;
  if (indents.filter((i) => i.startsWith("\t")).length > indents.length / 2) return { tabSize: 1, insertSpaces: false, eol: "\n" };
  const widths = indents.filter((i) => !i.includes("\t")).map((i) => i.length);
  return { tabSize: widths.length ? Math.min(...widths) : 2, insertSpaces: true, eol: "\n" };
}

function read(text: string): unknown {
  const errors: ParseError[] = [];
  const v = parse(text, errors, { allowTrailingComma: true, disallowComments: false });
  if (errors.length) throw new Error("settings file is not valid JSON(C)");
  return v;
}

function at(root: unknown, path: string[]): unknown {
  let cur = root;
  for (const k of path) {
    if (cur === null || typeof cur !== "object" || Array.isArray(cur)) return undefined;
    cur = (cur as Record<string, unknown>)[k];
  }
  return cur;
}

export function equal(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function entryKey(e: SettingsEntry): string {
  return `${e.file}\0${e.path}\0${JSON.stringify(e.value)}`;
}

/** Whether the entry is present in the settings text. */
export function hasEntry(text: string, e: SettingsEntry): boolean {
  const cur = at(read(text || "{}"), e.path.split("."));
  return Array.isArray(cur) ? cur.some((v) => equal(v, e.value)) : cur !== undefined && equal(cur, e.value);
}

/**
 * Appends entries (comments preserved). Arrays get missing items appended;
 * scalars are set only if absent. Returns the entries actually added.
 */
export function addEntries(text: string, entries: DesiredEntry[]): { text: string; added: SettingsEntry[] } {
  let out = text.trim() ? text : "{}\n";
  const FORMAT = detectFormatting(text);
  const added: SettingsEntry[] = [];
  for (const e of entries) {
    const path = e.path.split(".");
    const cur = at(read(out), path);
    let edits;
    if (e.append) {
      if (Array.isArray(cur)) {
        if (cur.some((v) => equal(v, e.value))) continue;
        edits = modify(out, [...path, -1], e.value, { formattingOptions: FORMAT, isArrayInsertion: true });
      } else if (cur === undefined) edits = modify(out, path, [e.value], { formattingOptions: FORMAT });
      else throw new Error(`${e.file}: ${e.path} is not an array; cannot append ${JSON.stringify(e.value)}`);
    } else {
      if (cur !== undefined) continue;
      edits = modify(out, path, e.value, { formattingOptions: FORMAT });
    }
    out = applyEdits(out, edits);
    added.push({ file: e.file, path: e.path, value: e.value });
  }
  if (!out.endsWith("\n")) out += "\n";
  return { text: out, added };
}

/**
 * Removes exactly the listed entries, then prunes containers the removal
 * emptied. Returns undefined when the file ends up as `{}`.
 */
export function removeEntries(text: string, entries: SettingsEntry[]): string | undefined {
  let out = text;
  const FORMAT = detectFormatting(text);
  for (const e of entries) {
    const path = e.path.split(".");
    const cur = at(read(out), path);
    let target: JSONPath | undefined;
    // the innermost container that may be left empty: the array itself, or the scalar's parent
    const top = Array.isArray(cur) ? path.length : path.length - 1;
    if (Array.isArray(cur)) {
      const i = cur.findIndex((v) => equal(v, e.value));
      if (i >= 0) target = [...path, i];
    } else if (cur !== undefined && equal(cur, e.value)) target = path;
    if (!target) continue;
    out = applyEdits(out, modify(out, target, undefined, { formattingOptions: FORMAT }));
    // prune containers emptied by this removal
    for (let n = top; n > 0; n--) {
      const parent = at(read(out), path.slice(0, n));
      const empty = Array.isArray(parent) ? parent.length === 0 : parent !== null && typeof parent === "object" && Object.keys(parent).length === 0;
      if (!empty) break;
      out = applyEdits(out, modify(out, path.slice(0, n), undefined, { formattingOptions: FORMAT }));
    }
  }
  const root = read(out);
  if (root !== null && typeof root === "object" && !Array.isArray(root) && Object.keys(root).length === 0) return undefined;
  return out;
}

/** Scalar entries the file already sets to a different value (left unchanged). */
export function conflictingScalars(text: string, entries: DesiredEntry[]): { entry: DesiredEntry; current: unknown }[] {
  if (!text.trim()) return [];
  const root = read(text);
  return entries
    .filter((e) => !e.append)
    .map((entry) => ({ entry, current: at(root, entry.path.split(".")) }))
    .filter(({ entry, current }) => current !== undefined && !equal(current, entry.value));
}
