#!/usr/bin/env bash
# Verify the release pipeline without publishing anything.
#
# Packaging cannot be exercised through the CLI seam the rest of the suite uses,
# so this script is its test: it builds, assembles, installs offline from local
# tarballs, and runs the installed launcher. Every publish is a dry run.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

HOST_TARGET="${HOST_TARGET:-darwin-arm64}"
say() { printf '\n== %s ==\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }

say "cargo build --release"
cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml"
VERSION="$("$ROOT/target/release/agentstow" --version | awk '{print $NF}')"
[ -n "$VERSION" ] || fail "the binary reports no version"
echo "binary reports version $VERSION"

say "assemble npm packages"
# Only the host target has a real binary here; CI cross-builds the rest. The
# stubs let the launcher resolution and offline install be tested for real.
bash "$ROOT/scripts/build-npm.sh" "$WORK/pkgs" "$HOST_TARGET" >/dev/null
for target in darwin-arm64 darwin-x64 linux-arm64 linux-x64 win32-arm64 win32-x64; do
  [ -d "$WORK/pkgs/$target" ] && continue
  mkdir -p "$WORK/pkgs/$target/bin"
  printf '#!/bin/sh\necho stub\n' > "$WORK/pkgs/$target/bin/agentstow"
  chmod +x "$WORK/pkgs/$target/bin/agentstow"
  os=darwin; case "$target" in linux-*) os=linux ;; win32-*) os=win32 ;; esac
  cpu=arm64; case "$target" in *-x64) cpu=x64 ;; esac
  cat > "$WORK/pkgs/$target/package.json" <<JSON
{ "name": "@agentstow/$target", "version": "$VERSION", "license": "MIT",
  "os": ["$os"], "cpu": ["$cpu"], "files": ["bin/agentstow"] }
JSON
done
ls "$WORK/pkgs"

say "no postinstall hooks anywhere"
if grep -RlE '"(postinstall|preinstall|install)"' "$WORK/pkgs" >/dev/null 2>&1; then
  fail "a package declares an install script; installs must not run code or fetch"
fi
echo "none declared"

say "npm pack every package"
mkdir -p "$WORK/tarballs"
for dir in "$WORK/pkgs"/*; do
  (cd "$dir" && npm pack --pack-destination "$WORK/tarballs" --loglevel error >/dev/null)
done
ls "$WORK/tarballs"

say "offline install from local tarballs"
mkdir -p "$WORK/consumer"
(cd "$WORK/consumer" && npm init -y --loglevel error >/dev/null)
HOST_TARBALL="$(ls "$WORK"/tarballs/agentstow-"$HOST_TARGET"-*.tgz 2>/dev/null || true)"
[ -n "$HOST_TARBALL" ] || HOST_TARBALL="$(ls "$WORK"/tarballs/*"$HOST_TARGET"*.tgz)"
LAUNCHER_TARBALL="$(ls "$WORK"/tarballs/agentstow-"$VERSION".tgz)"
# --offline proves nothing is fetched: npm fails outright if it needs the network.
(cd "$WORK/consumer" && npm install --offline --no-audit --no-fund --loglevel error \
  "$HOST_TARBALL" "$LAUNCHER_TARBALL") || fail "offline install failed"
echo "installed offline"

say "the installed launcher runs the real binary"
OUTPUT="$("$WORK/consumer/node_modules/.bin/agentstow" --version)"
echo "$OUTPUT"
case "$OUTPUT" in
  *"$VERSION"*) ;;
  *) fail "launcher did not report the built version" ;;
esac
"$WORK/consumer/node_modules/.bin/agentstow" doctor >/dev/null 2>&1 || true
CODE=0; "$WORK/consumer/node_modules/.bin/agentstow" frobnicate >/dev/null 2>&1 || CODE=$?
[ "$CODE" -eq 1 ] || fail "launcher did not propagate the exit code (got $CODE)"
echo "exit code propagated"

say "launcher fails clearly with no platform package"
mkdir -p "$WORK/bare" && (cd "$WORK/bare" && npm init -y --loglevel error >/dev/null)
(cd "$WORK/bare" && npm install --offline --no-audit --no-fund --loglevel error \
  --no-optional "$LAUNCHER_TARBALL") >/dev/null 2>&1 || true
if [ -x "$WORK/bare/node_modules/.bin/agentstow" ]; then
  MSG="$("$WORK/bare/node_modules/.bin/agentstow" --version 2>&1 || true)"
  case "$MSG" in
    *"no prebuilt binary"*|*"$VERSION"*) echo "handled" ;;
    *) fail "unhelpful message with no platform package: $MSG" ;;
  esac
fi

# Everything above is fully verifiable offline. What follows talks to the
# registry, where two outcomes are expected before the first real release and
# are reported rather than treated as pipeline faults:
#   - the launcher version is already taken (the 0.0.1 name-claim placeholder)
#   - the @agentstow scope does not exist yet, so scoped packages cannot resolve
# See docs/release-runbook.md.
say "npm publish --dry-run (registry)"
BLOCKED=0
for dir in "$WORK/pkgs"/*; do
  name="$(basename "$dir")"
  if out="$(cd "$dir" && npm publish --dry-run --loglevel error 2>&1)"; then
    echo "  $name: ok"
  elif printf '%s' "$out" | grep -q "cannot publish over the previously published"; then
    echo "  $name: version already published — bump the version before releasing"
    BLOCKED=$((BLOCKED + 1))
  elif printf '%s' "$out" | grep -qiE "404|not found|scope|permission|forbidden|E402|need to authorize"; then
    echo "  $name: scope not claimed yet — see docs/release-runbook.md"
    BLOCKED=$((BLOCKED + 1))
  else
    printf '%s\n' "$out" >&2
    fail "dry-run publish failed for $name for an unexpected reason"
  fi
done

say "cargo publish --dry-run"
if cargo publish --dry-run --allow-dirty --quiet --manifest-path "$ROOT/Cargo.toml" 2>"$WORK/cargo.err"; then
  echo "crate packages cleanly"
else
  cat "$WORK/cargo.err" >&2
  fail "cargo publish --dry-run failed"
fi

say "wheel builds, installs, and is executable"
# Only the host's wheel can be built here — build-wheels.py refuses to seal the
# host binary into another platform's wheel. Installing it for real is the
# point: a wheel whose binary lands non-executable builds and validates
# perfectly, and only `pip install` then `test -x` catches it.
if command -v python3 >/dev/null 2>&1; then
  if "$ROOT/scripts/build-wheels.py" "$WORK/wheelhouse" >"$WORK/wheels.log" 2>&1; then
    # Explicitly the platform wheel: py3-none-any sorts first and carries no
    # binary, so a plain `head -1` would test the fallback and report the
    # binary as broken.
    whl="$(ls "$WORK"/wheelhouse/*.whl 2>/dev/null | grep -v 'py3-none-any' | head -1)"
    fallback="$(ls "$WORK"/wheelhouse/*-py3-none-any.whl 2>/dev/null | head -1)"
    if [ -z "$whl" ]; then
      cat "$WORK/wheels.log" >&2
      fail "no wheel built for the host target"
    fi
    [ -n "$fallback" ] || fail "the py3-none-any fallback wheel was not built"
    python3 -m venv "$WORK/wheelvenv" >/dev/null 2>&1 || fail "could not create a venv"
    "$WORK/wheelvenv/bin/pip" install --quiet --no-index "$whl" || fail "pip install of the wheel failed"
    [ -x "$WORK/wheelvenv/bin/agentstow" ] || fail "the installed wheel binary is not executable"
    "$WORK/wheelvenv/bin/agentstow" --version >/dev/null || fail "the installed wheel binary does not run"
    echo "  $(basename "$whl"): installs and runs"

    # pip must prefer the platform wheel when both are offered, or every
    # supported user would silently get the fallback instead of the binary.
    python3 -m venv "$WORK/pickvenv" >/dev/null 2>&1 || fail "could not create a venv"
    "$WORK/pickvenv/bin/pip" install --quiet --no-index \
      --find-links "$WORK/wheelhouse" agentstow || fail "resolving from the wheelhouse failed"
    "$WORK/pickvenv/bin/agentstow" --version >/dev/null \
      || fail "pip chose the fallback over the platform wheel"
    echo "  pip prefers the platform wheel over py3-none-any"

    python3 -m venv "$WORK/fbvenv" >/dev/null 2>&1 || fail "could not create a venv"
    "$WORK/fbvenv/bin/pip" install --quiet --no-index "$fallback" || fail "pip install of the fallback failed"
    if "$WORK/fbvenv/bin/agentstow" >"$WORK/fb.out" 2>&1; then
      fail "the fallback exited 0; it must fail and explain itself"
    fi
    grep -q 'no prebuilt binary' "$WORK/fb.out" || fail "the fallback does not name the problem"
    grep -q 'cargo install agent-sync-sh' "$WORK/fb.out" || fail "the fallback gives no way forward"
    echo "  $(basename "$fallback"): explains itself and exits non-zero"
  else
    cat "$WORK/wheels.log" >&2
    fail "build-wheels.py failed"
  fi
else
  echo "  python3 not found — wheel check skipped"
fi

printf '\nLocal packaging checks passed.\n'
if [ "$BLOCKED" -gt 0 ]; then
  printf '%d package(s) cannot be published yet — see docs/release-runbook.md.\n' "$BLOCKED"
fi
