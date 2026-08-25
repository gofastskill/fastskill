#!/usr/bin/env bash
set -euo pipefail

max_lines=1000
failed=0

while IFS= read -r file; do
    lines=$(wc -l < "$file")
    if [ "$lines" -gt "$max_lines" ]; then
        printf '%s has %s lines, limit is %s\n' "$file" "$lines" "$max_lines" >&2
        failed=1
    fi
done < <(
    find crates -name '*.rs' \
        ! -path '*/tests/*' \
        ! -path '*/fixtures/*' \
        ! -path '*/snapshots/*' \
        ! -name '*_test.rs' \
        ! -name 'tests.rs'
)

exit "$failed"
