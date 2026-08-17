#!/usr/bin/env bash
# Smoke-test a built fastskill binary.
#
# This CLI builds its command tree at RUNTIME (cli-framework AppBuilder::register /
# register_out in main()), not at compile time -- a duplicate command path or a bad
# CommandSpec is a panic when the binary starts, not a compiler error. Compiling the
# binary (or cross-compiling it, in release.yml) never executes it, so a binary that
# fails to start (missing DLL, bad linkage, a runtime panic while building the command
# tree) would go completely undetected -- every CI job would still be green. This
# script actually runs the exact binary under test.
#
# Used by both release.yml (against the archived, per-target binary on every matrix
# leg) and test.yml (against the ubuntu and windows debug builds on every PR), so the
# checks below only ever need to be written -- and kept correct -- once.
#
# Usage: scripts/smoke-binary.sh <path-to-binary>
#
# Checks:
#   1. `<bin> --version` exits 0 and reports the version from the top-level Cargo.toml.
#   2. `<bin> --help` exits 0 and produces non-empty output.
#   3. In a fresh temp dir: `<bin> init --yes --skills-dir ./skills` then `<bin> list`,
#      both exit 0. This is the filesystem-write path, run offline.
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: $0 <path-to-binary>" >&2
  exit 1
fi

BIN_ARG="$1"

if [ ! -f "$BIN_ARG" ]; then
  echo "FAIL: binary not found at '$BIN_ARG'" >&2
  exit 1
fi

# Resolve to an absolute path before we ever cd, and independent of the caller's cwd,
# so the binary can still be invoked after we change directories below.
case "$BIN_ARG" in
  /*) BIN_ABS="$BIN_ARG" ;;
  *) BIN_ABS="$PWD/$BIN_ARG" ;;
esac

# Find Cargo.toml relative to the repo root (this script's parent directory), not the
# caller's cwd -- the caller may invoke us from anywhere.
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
CARGO_TOML="$REPO_ROOT/Cargo.toml"

if [ ! -f "$CARGO_TOML" ]; then
  echo "FAIL: could not find top-level Cargo.toml at '$CARGO_TOML'" >&2
  exit 1
fi

echo "Smoke-testing $BIN_ABS"

EXPECTED_VERSION=$(grep '^version = ' "$CARGO_TOML" | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "Expected version (from $CARGO_TOML): $EXPECTED_VERSION"

echo "--- Check 1: '$BIN_ABS --version' exits 0 and reports version $EXPECTED_VERSION ---"
VERSION_OUTPUT=$("$BIN_ABS" --version)
echo "Output: $VERSION_OUTPUT"
if [[ "$VERSION_OUTPUT" != *"$EXPECTED_VERSION"* ]]; then
  echo "FAIL: --version output does not contain expected version '$EXPECTED_VERSION'" >&2
  echo "This usually means a stale/mis-cached build shipped the wrong binary." >&2
  exit 1
fi

echo "--- Check 2: '$BIN_ABS --help' exits 0 and produces non-empty output ---"
HELP_OUTPUT=$("$BIN_ABS" --help)
if [ -z "$HELP_OUTPUT" ]; then
  echo "FAIL: --help produced no output" >&2
  exit 1
fi
echo "First lines of --help output:"
echo "$HELP_OUTPUT" | head -5

echo "--- Check 3: offline 'init' + 'list' in a fresh temp dir ---"
# `fastskill init` derives a default skill/project identifier from the current
# directory's basename, which must be alphanumeric/hyphens/underscores only.
# `mktemp -d` alone produces a basename like `tmp.XXXXXXXXXX` (dot -- invalid), so
# nest a cleanly-named subdirectory inside it rather than cd-ing straight into the
# mktemp dir.
SMOKE_PARENT=$(mktemp -d)
SMOKE_DIR="$SMOKE_PARENT/smoke-test"
mkdir -p "$SMOKE_DIR"
pushd "$SMOKE_DIR" >/dev/null

echo "Running: fastskill init --yes --skills-dir ./skills"
"$BIN_ABS" init --yes --skills-dir ./skills

echo "Running: fastskill list"
"$BIN_ABS" list

popd >/dev/null
rm -rf "$SMOKE_PARENT"

echo "Smoke test passed for $BIN_ABS"
