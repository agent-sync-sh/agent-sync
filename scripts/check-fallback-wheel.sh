#!/usr/bin/env bash
# Assert that the py3-none-any fallback wheel fails loudly and says what to do.
#
# pip installs this wheel only when no platform wheel matches the machine, so it
# carries no binary and cannot do the job it was asked to do. Its entire purpose
# is to name that and point somewhere useful, which makes its message load-
# bearing for exactly the users who have no other way forward.
#
# This lives in one file because it used to live in two. verify-packaging.sh and
# the wheels job in .github/workflows/release.yml each carried their own copy of
# the same greps; 2.0.6 repointed the message at agent-sync-sh and updated only
# the copy that runs locally, so the gate passed and the release then failed on
# the copy nobody can run before pushing a tag — taking the PyPI publish with it.
# Both callers now invoke this, so the local gate exercises the release's check.
set -euo pipefail

wheel="${1:?usage: check-fallback-wheel.sh <wheel> [venv-dir]}"
venv="${2:-"$(mktemp -d)/fbvenv"}"

[ -f "$wheel" ] || { echo "no such wheel: $wheel" >&2; exit 1; }

python3 -m venv "$venv" >/dev/null 2>&1 || { echo "could not create a venv" >&2; exit 1; }
"$venv/bin/pip" install --quiet --no-index "$wheel" \
  || { echo "pip install of the fallback failed" >&2; exit 1; }

out="$venv/fallback.out"
if "$venv/bin/agent-sync" >"$out" 2>&1; then
  echo "the fallback exited 0; it must fail and explain itself" >&2
  cat "$out" >&2
  exit 1
fi
cat "$out"

grep -q 'no prebuilt binary' "$out" \
  || { echo "the fallback does not name the problem" >&2; exit 1; }
grep -q 'cargo install agent-sync-sh' "$out" \
  || { echo "the fallback gives no way forward" >&2; exit 1; }
