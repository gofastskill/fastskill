#!/usr/bin/env bash
# Notify ailoop about git hook events (post-commit | pre-push). Non-fatal if ailoop is missing or fails.

EVENT="${1:-post-commit}"

TOP="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
cd "$TOP" || exit 0

command -v ailoop >/dev/null 2>&1 || exit 0

REPO="$(basename "$TOP")"
SHA="$(git rev-parse HEAD 2>/dev/null)" || exit 0
SHORT="$(git rev-parse --short HEAD 2>/dev/null)"

TITLE="$(git log -1 --format=%s HEAD 2>/dev/null)"
TITLE="${TITLE//\"/}"

BODY="$(git log -1 --format=%b HEAD 2>/dev/null | tr '\n' ' ' | sed 's/[[:space:]]\{2,\}/ /g' | cut -c1-4000)"
BODY="${BODY//\"/}"

TEXT="git-hook ${EVENT} repo=${REPO} commit=${SHA} short=${SHORT} title=${TITLE} message=${BODY}"
ailoop say "$TEXT" --server http://127.0.0.1:8080 2>/dev/null || true
exit 0
