/** A tool adapter (§16): its version ranges, how each does each operation, and what it knows of its codes. */

/** One version range of a tool, with how each operation is invoked there. */
export interface AdapterRange {
  /** First version of the range (inclusive). */
  from: string;
  /** First version after it (exclusive); none: every later release. */
  to?: string;
  /** Where this range's behaviour is documented (a changelog entry, a release note). */
  source: string;
  /**
   * Per operation, the arguments after the tool's name: the caller's arguments replace a
   * `{args}` word, else follow. null: this range cannot do it (`needs` says from which
   * version it can).
   */
  ops: Record<string, string | null>;
}

export interface ToolAdapter {
  tool: string;
  /** Forge's own pinned tool (ty): its version is the pin, never detected. */
  pinned?: string;
  /** The newest release the ranges were proven on (the test matrix); later releases are assumed to behave as it does, and reported. */
  verified: string;
  /** Oldest first, without gaps. */
  ranges: AdapterRange[];
  /**
   * A usage error of the tool's command line (an option or a value this version does not
   * know): never a finding. The exit status and a line of its output that say so (a POSIX
   * ERE without quotes: it is rendered into a single-quoted shell string).
   */
  usage: { exit: number; pattern: string };
  /**
   * Which rule codes the installed version has, asked from the binary: the operation that
   * lists them (ruff `rule --all --output-format json`) and the history of the codes Forge's
   * rules expect (their first stable release, former codes and names).
   */
  codes?: { op: string; history: Record<string, { since: string; codes: string[]; names: string[] }> };
}
