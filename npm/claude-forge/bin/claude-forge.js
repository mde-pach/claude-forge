#!/usr/bin/env node
// Runs the claude-forge binary of this platform's package (an optional dependency, the
// one npm installs for this OS and CPU), passing arguments, stdio and the exit status on.
"use strict";

const { spawnSync } = require("node:child_process");

const PACKAGES = {
  "darwin arm64": "@maximedepachtere/claude-forge-darwin-arm64",
  "darwin x64": "@maximedepachtere/claude-forge-darwin-x64",
  "linux arm64": "@maximedepachtere/claude-forge-linux-arm64",
  "linux x64": "@maximedepachtere/claude-forge-linux-x64",
};
const INSTALL = "curl -LsSf https://github.com/mde-pach/claude-forge/releases/latest/download/install.sh | sh";

function fail(message) {
  process.stderr.write(`claude-forge: ${message}\n`);
  process.exit(1);
}

const pkg = PACKAGES[`${process.platform} ${process.arch}`];
if (!pkg) fail(`no prebuilt binary for ${process.platform} ${process.arch}; install from source: cargo install --git https://github.com/mde-pach/claude-forge forge-cli`);

let bin;
try {
  bin = require.resolve(`${pkg}/bin/claude-forge`);
} catch {
  fail(`${pkg} is not installed (were optional dependencies omitted?). Reinstall claude-forge with optional dependencies, or install with: ${INSTALL}`);
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (result.error) fail(`cannot run ${bin}: ${result.error.message}`);
if (result.signal) process.kill(process.pid, result.signal);
process.exit(result.status ?? 1);
