/**
 * Globs, as packs, rules, roles and forge.yaml write them: `**` spans directories, `*` and
 * `?` stay within one segment, `[0-9]` / `[!x]` one character, `{a,b}` alternatives (nested,
 * and holding any of these). Matched in TypeScript (globToRegExp) and rendered as shell
 * `case` patterns for the generated scripts (globToCase).
 */

const globCache = new Map<string, RegExp>();

/** The glob as an anchored RegExp on repository paths. */
export function globToRegExp(glob: string): RegExp {
  let re = globCache.get(glob);
  if (re) return re;
  re = new RegExp(`^${globSource(glob)}$`);
  globCache.set(glob, re);
  return re;
}

export function globMatch(glob: string, path: string): boolean {
  return globToRegExp(glob).test(path);
}

function globSource(glob: string): string {
  let out = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i]!;
    if (c === "*") {
      if (glob[i + 1] === "*") {
        i++;
        if (glob[i + 1] === "/") {
          i++;
          out += "(?:.*/)?";
        } else out += ".*";
      } else out += "[^/]*";
    } else if (c === "?") out += "[^/]";
    else if (c === "[" && glob.indexOf("]", i + 2) > 0) {
      // a bracket expression: [0-9], [!._] (one character, never "/")
      const end = glob.indexOf("]", i + 2);
      let body = glob.slice(i + 1, end);
      const neg = body.startsWith("!") || body.startsWith("^");
      if (neg) body = body.slice(1);
      out += `(?!/)[${neg ? "^" : ""}${body.replace(/[\\\]^]/g, "\\$&")}]`;
      i = end;
    } else if (c === "{") {
      // the matching brace, and the alternatives split at depth 0
      const alts: string[] = [];
      let depth = 0;
      let start = i + 1;
      let end = -1;
      for (let k = i + 1; k < glob.length && end < 0; k++) {
        if (glob[k] === "{") depth++;
        else if (glob[k] === "}" && depth > 0) depth--;
        else if (glob[k] === "}") end = k;
        else if (glob[k] === "," && depth === 0) {
          alts.push(glob.slice(start, k));
          start = k + 1;
        }
      }
      if (end < 0) {
        out += "\\{";
        continue;
      }
      alts.push(glob.slice(start, end));
      out += `(?:${alts.map(globSource).join("|")})`;
      i = end;
    } else out += c.replace(/[.+^$()|[\]\\]/g, "\\$&");
  }
  return out;
}

// ---------------------------------------------------------------- shell case patterns

function caseEscape(s: string): string {
  // bracket expressions ([0-9], [!x]) are kept: case patterns have them too
  return s
    .split(/(\[!?[^\]/'"\\]+\])/)
    .map((part, i) => (i % 2 ? part : part.replace(/[^A-Za-z0-9_./*?@+,=:%-]/g, "\\$&")))
    .join("");
}

/** A glob as shell `case` patterns. In `case`, `*` also matches `/`. */
export function globToCase(glob: string): string[] {
  const brace = /\{([^{}]*)\}/.exec(glob);
  if (brace) {
    return brace[1]!.split(",").flatMap((alt) => globToCase(glob.slice(0, brace.index) + alt + glob.slice(brace.index + brace[0].length)));
  }
  const out = new Set<string>();
  const variants = (g: string): string[] => {
    const i = g.indexOf("**/");
    if (i < 0) return [g.replace(/\*\*/g, "*")];
    const before = g.slice(0, i);
    const after = g.slice(i + 3);
    return [...variants(before + after), ...variants(before + "*/" + after)];
  };
  for (const v of variants(glob)) out.add(caseEscape(v.replace(/\*+/g, "*")));
  return [...out];
}

/** Globs as one `case` pattern list (`a|b|c`). */
export function casePatterns(globs: string[]): string {
  return [...new Set(globs.flatMap(globToCase))].join("|");
}

// ---------------------------------------------------------------- globs under a directory

/** The glob relative to dir ("" is the repository root). */
export function under(dir: string, glob: string): string {
  return dir ? `${dir}/${glob}` : glob;
}

/** The glob relative to each of dirs. */
export function inDirs(glob: string, dirs: string[]): string[] {
  return dirs.map((d) => `${d}/${glob}`);
}

/**
 * A glob list (`!` before an exclusion) relative to each workspace package of dirs (the
 * whole package when the list has no include glob); without dirs, the list as it is.
 */
export function scopeInDirs(files: string[], dirs?: string[]): string[] {
  if (!dirs) return files;
  const include = files.filter((g) => !g.startsWith("!"));
  return [...(include.length ? include : ["**"]).flatMap((g) => inDirs(g, dirs)), ...files.filter((g) => g.startsWith("!")).flatMap((g) => inDirs(g.slice(1), dirs).map((x) => "!" + x))];
}
