#!/usr/bin/env bash
# Additional Linux PR gates used by run-tests.sh. Do not silently skip tools.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
BASE_REF=${1:-origin/main}
for tool in cargo cargo-nextest cargo-llvm-cov rustup python3 node git; do
    command -v "$tool" >/dev/null || {
        echo "error: missing $tool; see CONTRIBUTING.md local PR validation prerequisites" >&2
        exit 1
    }
done
# Corepack honors webdocs/package.json without replacing a global pnpm install.
if command -v corepack >/dev/null; then
    PNPM=(corepack pnpm)
elif command -v pnpm >/dev/null; then
    PNPM=(pnpm)
else
    echo 'error: pnpm or corepack must be on PATH; see CONTRIBUTING.md' >&2
    exit 1
fi
git rev-parse --verify "${BASE_REF}^{commit}" >/dev/null || {
    echo "error: fetch the PR target branch, or pass --base REF" >&2
    exit 1
}
git merge-base "$BASE_REF" HEAD >/dev/null
if ! rustup component list --installed | grep -q '^llvm-tools'; then
    echo 'error: run rustup component add llvm-tools-preview' >&2
    exit 1
fi
PNPM_VERSION=$(cd webdocs && "${PNPM[@]}" --version)
if [[ $(node --version) != v22.* ]] || [[ $PNPM_VERSION != 10.21.0 ]]; then
    echo 'error: CI requires Node 22 and pnpm 10.21.0' >&2
    exit 1
fi

export INSTA_UPDATE=no
echo 'Local PR gates: Windows execution, CodeQL analysis/upload and GitHub settings remain remote checks.' >&2
echo "Coverage base: $BASE_REF (local ref; fetch the target before running)" >&2
# The CI checker compares committed changes. Reject dirty Rust sources rather
# than silently omitting new/uncommitted production files from the coverage gate.
if [[ -n $(git status --porcelain --untracked-files=all -- 'crates/**/*.rs') ]]; then
    echo 'error: commit Rust changes before checking PR coverage (CI checks committed changes)' >&2
    exit 1
fi

cargo fmt --all -- --check
bash scripts/check-source-size.sh
python3 -m unittest discover -s scripts/tests -p 'test_local_ci.py'
cargo clippy --workspace --all-targets --all-features
cargo build --all-features
bash scripts/smoke-binary.sh "${CARGO_TARGET_DIR:-target}/debug/fastskill"
cargo nextest run --retries 3 --fail-fast -E 'not test(install_e2e_tests)'

# Fresh instrumentation counters: never allow a previous run to inflate coverage.
cargo llvm-cov clean --workspace
NEXTEST_THREADS=2 cargo llvm-cov nextest --all-features -E 'not test(install_e2e_tests)' --no-report
REPORT_DIR=$(mktemp -d "${TMPDIR:-/tmp}/fastskill-coverage.XXXXXX")
echo "Coverage reports: $REPORT_DIR" >&2
cargo llvm-cov report --lcov --output-path "$REPORT_DIR/lcov.info"
cargo llvm-cov report --json --summary-only --output-path "$REPORT_DIR/coverage-summary.json"
python3 scripts/check-modified-rust-coverage.py "$BASE_REF" "$REPORT_DIR/coverage-summary.json" 90

cd webdocs
"${PNPM[@]}" install --frozen-lockfile
# Advisory in CI too; do not weaken any of the mandatory checks below.
"${PNPM[@]}" audit --prod --audit-level high || echo 'warning: dependency audit failed (advisory, matching CI)' >&2
"${PNPM[@]}" check:content
"${PNPM[@]}" lint
"${PNPM[@]}" typecheck
"${PNPM[@]}" build
"${PNPM[@]}" check:export
