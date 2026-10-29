/**
 * ngc, the Angular compiler of the project's `@angular/compiler-cli` (§16), on the TypeScript
 * its peer range allows. What Forge asks of it (`ngc -p <tsconfig> --noEmit`: tsc's command
 * line, with the template type-check) is the same on every covered release:
 *
 * - 9.0.0 is the oldest covered: ngc compiles with Ivy by default from it (angular
 *   CHANGELOG_ARCHIVE.md, 9.0.0: "Angular now compiles with Ivy by default" (#32219), "make
 *   the Ivy compiler the default for ngc"), whose template type-checker
 *   (`strictTemplates`) is what typescript/angular/typecheck relies on. Earlier releases run
 *   View Engine, which type-checks templates differently (and need TypeScript < 3.6, Node
 *   versions long out of support).
 * - Later majors (19.0.0 made components standalone by default, CHANGELOG.md 19.0.0; each
 *   major moves the TypeScript peer range) do not change the command line (verified 9.0.0,
 *   12.0.0, 13.0.0, 16.0.0, 19.0.0, 22.2.3).
 * - Usage errors exit 1, like template and type errors, with tsc's `error TS5023: Unknown
 *   compiler option '--x'.` or `error TS6044: Compiler option 'x' expects an argument.` on
 *   stderr, coloured even when piped. Unlike tsc, ngc prints tsconfig errors without the
 *   file (`error TS5023: Unknown compiler option 'bogusOpt'.`, `error TS6046: Argument for
 *   '--target' option must be ...`), so the pattern asks for the `--` of a command-line
 *   option and leaves TS6046 out.
 *
 * VERSION: `ngc --version` prints `unknown` (13.0.0-22.2.3) or reads ./tsconfig.json (9.x,
 * 12.x); the version is @angular/compiler-cli's package.json `version`: see ngc.md.
 */
import type { ToolAdapter } from "./types.js";

export const NGC: ToolAdapter = {
  tool: "ngc",
  verified: "22.2.3",
  // `ngc --version` prints `unknown` (13+) or reads ./tsconfig.json (9, 12): the package says it
  package: "@angular/compiler-cli",
  usage: { exit: 1, pattern: "error.* TS(5023|5025): .*Unknown compiler option .--|error.* TS6044: " },
  ranges: [
    {
      from: "9.0.0",
      source: "angular CHANGELOG_ARCHIVE.md 9.0.0: Angular now compiles with Ivy by default (#32219)",
      ops: {
        // typescript/angular/typecheck, once per tsconfig of file role @angular-tsconfig
        typecheck: "-p {args} --noEmit",
      },
    },
  ],
};
