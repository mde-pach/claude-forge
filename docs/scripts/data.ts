// Writes what the site reads from the Forge binary into docs/.data/:
//   packs.json  claude-forge packs dump --json (the built-in packs, as the Rust loader reads them)
//   cli.json    claude-forge --help and each command's --help
// The binary is $FORGE_BIN when set, else this checkout's, built with cargo.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..", "..");
const out = join(root, "docs", ".data");

function binary(): string {
  if (process.env.FORGE_BIN) return process.env.FORGE_BIN;
  execFileSync("cargo", ["build", "--quiet", "--locked", "-p", "forge-cli", "--bin", "claude-forge"], { cwd: root, stdio: "inherit" });
  const meta = JSON.parse(execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], { cwd: root, encoding: "utf8", maxBuffer: 64 << 20 }));
  return join(meta.target_directory, "debug", "claude-forge");
}

const bin = binary();
const run = (args: string[]) => execFileSync(bin, args, { cwd: root, encoding: "utf8", maxBuffer: 256 << 20 });

mkdirSync(out, { recursive: true });
writeFileSync(join(out, "packs.json"), run(["packs", "dump", "--json"]));

const COMMANDS = ["init", "apply", "diff", "status", "explain", "remove", "check", "packs dump", "packs lint"];
const cli = [{ command: "", help: run(["--help"]) }, ...COMMANDS.map((c) => ({ command: c, help: run([...c.split(" "), "--help"]) }))];
writeFileSync(join(out, "cli.json"), JSON.stringify(cli, null, 2));
console.log(`docs data: ${out} (from ${bin})`);
