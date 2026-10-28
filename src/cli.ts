// forge's entry point (dist/forge.js): runs the command line, and turns its errors into exit codes.
import { err, ForgeError } from "./cli/io.js";
import { main } from "./cli/main.js";

try {
  process.exitCode = main(process.argv.slice(2));
} catch (e) {
  if (e instanceof ForgeError) {
    err(e.message);
    process.exitCode = e.code;
  } else {
    err(`forge: ${(e as Error).message}`);
    process.exitCode = 1;
  }
}
