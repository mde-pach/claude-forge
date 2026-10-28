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
  /** Forge's own pinned tool (ty, gitleaks): its version is the pin, never detected. */
  pinned?: string;
  /** The newest release the ranges were proven on (the test matrix); later releases are assumed to behave as it does, and reported. */
  verified: string;
  /**
   * How the engine runs it (§16.2): "project" (the default: the project's own install,
   * node_modules/.bin, .venv or its Python environment, forge_exec_tool), "path" (the
   * executable of that name on PATH: terraform, helm, the package managers), or the shell
   * function that runs Forge's pinned release with the tool's arguments (forge_ty).
   */
  runner?: string;
  /**
   * A shell function printing which adapter drives the project's tool, when it is one of
   * several (terraform or tofu: forge_tf_bin). The rule names the first.
   */
  pick?: string;
  /** The arguments that make it print its version (default `--version`); its first x.y[.z] is the version. */
  versionArgs?: string;
  /** Oldest first, without gaps. */
  ranges: AdapterRange[];
  /**
   * A usage error of the tool's command line (an option or a value this version does not
   * know): never a finding. The exit status and a line of its output, stdout or stderr,
   * that say so (a POSIX ERE without quotes: it is rendered into a single-quoted shell
   * string). null: the tool has no such signature distinct from its findings.
   */
  usage: { exit: number; pattern: string } | null;
  /**
   * A check command that runs the tool itself (rejected when the rule is loaded: it must say
   * `forge_do <tool> <op>`): a regular expression over the command. Default: the tool's name
   * as a command word.
   */
  direct?: string;
  /**
   * Which rule codes the installed version has, asked from the binary: the operation that
   * lists them (ruff `rule --all --output-format json`) and the history of the codes Forge's
   * rules expect (their first stable release, former codes and names).
   */
  codes?: { op: string; history: Record<string, { since: string; codes: string[]; names: string[] }> };
}
