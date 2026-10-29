#!/usr/bin/env node
// Builds the npm packages of a release, ready for `npm publish`:
//   node npm/stage.mjs <version> <dir with claude-forge-<version>-<target>.tar.gz> <out dir>
// <out>/claude-forge is the main package (the JS launcher, the platform packages as
// optionalDependencies, all at <version>); <out>/<os>-<cpu> holds each platform's binary,
// with `os` and `cpu` set so npm installs only the matching one (as esbuild and biome do).
import { execFileSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const TARGETS = {
  "aarch64-apple-darwin": ["darwin", "arm64"],
  "x86_64-apple-darwin": ["darwin", "x64"],
  "aarch64-unknown-linux-musl": ["linux", "arm64"],
  "x86_64-unknown-linux-musl": ["linux", "x64"],
};

const [version, dist, out] = process.argv.slice(2);
if (!version || !dist || !out) {
  process.stderr.write("usage: stage.mjs <version> <dist dir> <out dir>\n");
  process.exit(2);
}
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error(`not a semver version: ${version}`);

const here = dirname(fileURLToPath(import.meta.url));
const main = JSON.parse(readFileSync(join(here, "claude-forge/package.json"), "utf8"));
const repository = main.repository;
mkdirSync(out, { recursive: true });

for (const [target, [os, cpu]] of Object.entries(TARGETS)) {
  const archive = join(dist, `claude-forge-${version}-${target}.tar.gz`);
  if (!existsSync(archive)) throw new Error(`missing ${archive}`);
  const name = `${main.name}-${os}-${cpu}`;
  if (!(name in main.optionalDependencies)) throw new Error(`${name} is not an optional dependency of ${main.name}`);
  const dir = join(out, `${os}-${cpu}`);
  mkdirSync(join(dir, "bin"), { recursive: true });
  const tmp = mkdtempSync(join(tmpdir(), "forge-stage-"));
  execFileSync("tar", ["-xzf", archive, "-C", tmp]);
  renameSync(join(tmp, `claude-forge-${version}-${target}`, "claude-forge"), join(dir, "bin/claude-forge"));
  chmodSync(join(dir, "bin/claude-forge"), 0o755);
  const pkg = {
    name,
    version,
    description: `The claude-forge binary for ${os} ${cpu}.`,
    repository,
    os: [os],
    cpu: [cpu],
    files: ["bin/"],
    preferUnplugged: true,
  };
  writeFileSync(join(dir, "package.json"), `${JSON.stringify(pkg, null, 2)}\n`);
}

const mainDir = join(out, "claude-forge");
cpSync(join(here, "claude-forge"), mainDir, { recursive: true });
main.version = version;
for (const dep of Object.keys(main.optionalDependencies)) main.optionalDependencies[dep] = version;
writeFileSync(join(mainDir, "package.json"), `${JSON.stringify(main, null, 2)}\n`);
chmodSync(join(mainDir, "bin/claude-forge.js"), 0o755);
process.stdout.write(`staged ${Object.keys(TARGETS).length + 1} packages in ${out}\n`);
