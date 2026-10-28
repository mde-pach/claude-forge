import type { GuardExample, Rule } from "./types.js";

/** A rule's code examples (strings); fixtures are for check rules only. */
export function codeExamples(rule: Pick<Rule, "examples">, kind: "match" | "no_match"): string[] {
  return (rule.examples?.[kind] ?? []).filter((e): e is string => typeof e === "string");
}

/**
 * A tool rule's examples: code written in the rule's scope (a string), or code written where
 * a one-file fixture puts it (`{files: {"src/Card.vue": …}}`: the file type and directory
 * of the example, for a rule that covers several file types).
 */
export function toolExamples(rule: Pick<Rule, "examples">, kind: "match" | "no_match"): { code: string; path?: string }[] {
  return (rule.examples?.[kind] ?? []).flatMap((e) => {
    if (typeof e === "string") return [{ code: e }];
    if ("command" in e || e.head || Object.keys(e.files).length !== 1) return [];
    const [path, code] = Object.entries(e.files)[0]!;
    return [{ code, path }];
  });
}

/** A guard rule's examples as commands, each with the files of the directory it runs from. */
export function guardExamples(rule: Pick<Rule, "examples">, kind: "match" | "no_match"): GuardExample[] {
  return (rule.examples?.[kind] ?? []).flatMap((e) => (typeof e === "string" ? [{ command: e }] : "command" in e ? [e] : []));
}
