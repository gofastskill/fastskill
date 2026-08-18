#!/usr/bin/env bash
# Entrypoint for the nightly quality container (spec 005, P4).
#
# Runs both suites of the nightly soft tier and merges their output. This tier must
# NEVER fail the branch on findings (spec 005: "a poor error message or doc drift is
# a report, not a failure") — only genuine infrastructure breakage (no binary, an
# unreachable gateway, a malformed harness) should surface as a failed job. Both
# suites signal that distinction the same way: exit 0 means "ran" regardless of what
# it found, exit 2 means "could not run at all". This script fails (non-zero exit,
# which fails the docker run step and therefore the job) only on a 2 from either
# suite; a "poor" or "drift" verdict inside a 0 exit never does.
#
# Budget split (spec 005 Q6/Q12 — 300 gateway requests total per run, shared across
# both suites): 120 to error-quality, 180 to doc-walk, set explicitly here rather
# than left to whichever suite runs first. Reasoning: error-quality's live suite is
# currently 8 cases x 3 trials = 24 requests (see ci/quality/error_quality/README.md)
# so 120 is 5x headroom for the case set to grow ("extend as new error paths land",
# spec 005 §1) without a workflow edit. Doc-walk classifies every runnable block
# across 4 docs (spec 005 Q10) — a larger, still-growing corpus — so it gets the
# larger share. 120 + 180 = 300 exactly: the two caps together ARE the total cap,
# not an independent limit layered on top of it.
#
# P4 may land before P3 (spec 005 doc-walk, ci/quality/docwalk/run_docwalk.py). A
# sibling PR not having merged yet is not an infrastructure failure, so this script
# checks for that file and skips the step with a clear note instead of erroring —
# and it must NOT be stubbed out here (that file belongs to the other PR).

set -uo pipefail

OUT="${QUALITY_OUT:-/out}"
mkdir -p "$OUT"
SUMMARY="$OUT/summary.md"
: > "$SUMMARY"

ERROR_MAX="${ERROR_QUALITY_MAX_REQUESTS:-120}"
DOCWALK_MAX="${DOCWALK_MAX_REQUESTS:-180}"
BINARY="/usr/local/bin/fastskill"

infra_broke=0

echo "================================================================================"
echo "error-message quality  (cap: ${ERROR_MAX} gateway requests)"
echo "================================================================================"
python3 ci/quality/error_quality/run_suite.py \
    --binary "$BINARY" \
    --json "$OUT/error_quality.json" \
    --summary "$SUMMARY" \
    --max-requests "$ERROR_MAX"
error_rc=$?
echo "error-quality exit code: $error_rc"
if [ "$error_rc" -eq 2 ]; then
    echo "INFRA FAILURE: error-quality suite could not run (exit 2)." >&2
    infra_broke=1
elif [ "$error_rc" -ne 0 ]; then
    echo "INFRA FAILURE: error-quality suite exited $error_rc (expected 0 or 2)." >&2
    infra_broke=1
fi

DOCWALK_SCRIPT="ci/quality/docwalk/run_docwalk.py"
echo
echo "================================================================================"
if [ -f "$DOCWALK_SCRIPT" ]; then
    echo "doc-walk drift  (cap: ${DOCWALK_MAX} gateway requests)"
    echo "================================================================================"
    python3 "$DOCWALK_SCRIPT" \
        --binary "$BINARY" \
        --json "$OUT/docwalk.json" \
        --summary "$SUMMARY" \
        --max-requests "$DOCWALK_MAX"
    docwalk_rc=$?
    echo "docwalk exit code: $docwalk_rc"
    if [ "$docwalk_rc" -eq 2 ]; then
        echo "INFRA FAILURE: doc-walk suite could not run (exit 2)." >&2
        infra_broke=1
    elif [ "$docwalk_rc" -ne 0 ]; then
        echo "INFRA FAILURE: doc-walk suite exited $docwalk_rc (expected 0 or 2)." >&2
        infra_broke=1
    fi
else
    echo "doc-walk drift  SKIPPED"
    echo "================================================================================"
    echo "$DOCWALK_SCRIPT does not exist on this ref yet (spec 005 P3 not merged)."
    echo "This is expected if P4 landed before P3 — not treated as an infra failure."
    {
        echo ""
        echo "## Doc-walk drift"
        echo ""
        echo "_Skipped: \`$DOCWALK_SCRIPT\` does not exist on this ref yet (spec 005 P3)._"
    } >> "$SUMMARY"
fi

echo
if [ "$infra_broke" -eq 1 ]; then
    echo "One or more suites could not run at all — failing the job." >&2
    exit 1
fi

echo "Nightly quality tier ran to completion (soft tier: findings above never fail this job)."
exit 0
