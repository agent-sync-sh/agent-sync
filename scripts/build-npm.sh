#!/usr/bin/env bash
# Assemble the npm packages from built binaries.
#
#   scripts/build-npm.sh <out-dir> [target ...]
#
# Each target is a node-style "<platform>-<arch>" name. The binary for a target
# is expected at target/<rust-triple>/release/agent-sync, except for the host
# target in a plain `cargo build --release`, which is also accepted.
#
# Produces <out-dir>/agent-sync-sh (the launcher) and <out-dir>/<target> packages.
set -euo pipefail

OUT="${1:?usage: build-npm.sh <out-dir> [target ...]}"
shift
TARGETS=("$@")
if [ ${#TARGETS[@]} -eq 0 ]; then
  TARGETS=(darwin-arm64 darwin-x64 linux-arm64 linux-x64 win32-arm64 win32-x64)
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

rust_triple() {
  case "$1" in
    darwin-arm64) echo aarch64-apple-darwin ;;
    darwin-x64)   echo x86_64-apple-darwin ;;
    linux-arm64)  echo aarch64-unknown-linux-gnu ;;
    linux-x64)    echo x86_64-unknown-linux-gnu ;;
    win32-arm64)  echo aarch64-pc-windows-msvc ;;
    win32-x64)    echo x86_64-pc-windows-msvc ;;
    *) echo "unknown target: $1" >&2; return 1 ;;
  esac
}

node_os()  { case "$1" in darwin-*) echo darwin ;; linux-*) echo linux ;; win32-*) echo win32 ;; esac; }
node_cpu() { case "$1" in *-arm64) echo arm64 ;; *-x64) echo x64 ;; esac; }
exe_for()  { case "$1" in win32-*) echo .exe ;; *) echo "" ;; esac; }

mkdir -p "$OUT"

# The launcher, with its version and optional dependencies pinned to VERSION.
cp -R "$ROOT/npm/agent-sync-sh" "$OUT/agent-sync-sh"
node - "$OUT/agent-sync-sh/package.json" "$VERSION" <<'JS'
const fs = require("node:fs");
const [file, version] = process.argv.slice(2);
const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
pkg.version = version;
for (const name of Object.keys(pkg.optionalDependencies ?? {})) {
  pkg.optionalDependencies[name] = version;
}
fs.writeFileSync(file, JSON.stringify(pkg, null, 2) + "\n");
JS
cp "$ROOT/README.md" "$OUT/agent-sync-sh/README.md" 2>/dev/null || true

for target in "${TARGETS[@]}"; do
  triple="$(rust_triple "$target")"
  exe="$(exe_for "$target")"
  binary="$ROOT/target/$triple/release/agent-sync$exe"
  # A host-target build without --target lands in target/release.
  [ -f "$binary" ] || binary="$ROOT/target/release/agent-sync$exe"
  if [ ! -f "$binary" ]; then
    echo "missing binary for $target (looked for target/$triple/release/agent-sync$exe)" >&2
    exit 1
  fi

  dir="$OUT/$target"
  mkdir -p "$dir/bin"
  cp "$binary" "$dir/bin/agent-sync$exe"
  chmod +x "$dir/bin/agent-sync$exe"

  node - "$dir/package.json" "$target" "$VERSION" "$(node_os "$target")" "$(node_cpu "$target")" "$exe" <<'JS'
const fs = require("node:fs");
const [file, target, version, os, cpu, exe] = process.argv.slice(2);
fs.writeFileSync(
  file,
  JSON.stringify(
    {
      name: `@agent-sync-sh/${target}`,
      version,
      description: `agent-sync binary for ${target}`,
      license: "MIT",
      repository: { type: "git", url: "git+https://github.com/agent-sync-sh/agent-sync.git" },
      // npm installs an optional dependency only when os and cpu match, which
      // is what keeps all but one of these off any given machine.
      os: [os],
      cpu: [cpu],
      files: [`bin/agent-sync${exe}`],
      preferUnplugged: true,
    },
    null,
    2
  ) + "\n"
);
JS
done

echo "assembled ${#TARGETS[@]} platform packages plus the launcher in $OUT"
