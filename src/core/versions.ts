import { coerce, minVersion, satisfies, validRange } from "semver";

/** Compares dotted versions on their first three numbers (1.10.0 > 1.9.3). */
export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map((x) => parseInt(x, 10) || 0);
  const pb = b.split(".").map((x) => parseInt(x, 10) || 0);
  for (let i = 0; i < 3; i++) if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) - (pb[i] ?? 0);
  return 0;
}

/** The lowest version an npm range admits ("" when it is not a semver range: workspace:*, a URL). */
export function versionFromRange(range: string): string {
  if (!validRange(range)) return "";
  return minVersion(range)?.version ?? "";
}

/** The version a PEP 440 specifier pins or starts from (==, >=, ~=, ===), as semver; "" without one. */
export function versionFromPep440(spec: string): string {
  const m = /(?:==|>=|~=|===)\s*([0-9][0-9A-Za-z.]*)/.exec(spec);
  return m ? (coerce(m[1]!)?.version ?? "") : "";
}

/** A version string as semver ("" when it has none). */
export function coerceVersion(v: string): string {
  return coerce(v)?.version ?? "";
}

/** Whether a declared version is in range; an unknown version ("", workspace:*, a git URL) is accepted. */
export function versionSatisfies(version: string, range: string | undefined): boolean {
  if (!range) return true;
  const v = coerce(version)?.version;
  return !v || satisfies(v, range, { includePrerelease: true });
}
