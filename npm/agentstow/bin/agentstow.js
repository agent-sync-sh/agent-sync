#!/usr/bin/env node
"use strict";

// Launcher for the prebuilt agentstow binary.
//
// The real executable ships in one platform package per target, declared as
// optional dependencies. npm installs only the one matching this machine
// (each declares `os` and `cpu`), so there is nothing to download here and no
// postinstall step — an install works offline and in a sandboxed CI.

const { spawnSync } = require("node:child_process");

const PACKAGE = `@agentstow/${process.platform}-${process.arch}`;
const EXE = process.platform === "win32" ? ".exe" : "";

function binaryPath() {
  try {
    return require.resolve(`${PACKAGE}/bin/agentstow${EXE}`);
  } catch {
    return null;
  }
}

const binary = binaryPath();

if (!binary) {
  process.stderr.write(
    `agentstow: no prebuilt binary for ${process.platform}-${process.arch}.\n` +
      `Expected the optional dependency ${PACKAGE}.\n` +
      `agentstow is now agent-sync. If your platform is unsupported, build from\n` +
      `source: cargo install agent-sync-sh  (details: https://agent-sync.sh)\n` +
      `If the install skipped optional dependencies, reinstall without --no-optional.\n`
  );
  process.exit(1);
}

let result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });

// A package that lost its executable bit in transit (1.1.2 shipped that way)
// is repairable on the spot; anything else is reported as-is.
if (result.error && result.error.code === "EACCES") {
  try {
    require("node:fs").chmodSync(binary, 0o755);
    result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
  } catch {}
}

if (result.error) {
  process.stderr.write(`agentstow: cannot run ${binary}: ${result.error.message}\n`);
  process.exit(1);
}

// A killed child reports a signal rather than a code; mirror the shell's
// 128+signal convention so callers still see a non-zero exit.
if (result.signal) {
  process.exit(1);
}

process.exit(result.status === null ? 1 : result.status);
