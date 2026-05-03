#!/usr/bin/env bash
# Notify ailoop about git hook events (post-commit | pre-push). Non-fatal if ailoop is missing or fails.
# Channel name: derived from origin as org/repo -> org-repo (ailoop forbids / and @ in channel names).

EVENT="${1:-post-commit}"

TOP="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
cd "$TOP" || exit 0

command -v ailoop >/dev/null 2>&1 || exit 0

sanitize_channel_slug() {
  local s="$1"
  s=$(printf '%s' "$s" | tr '[:upper:]' '[:lower:]')
  s=$(printf '%s' "$s" | sed 's/[^a-z0-9_-]/-/g')
  s=$(printf '%s' "$s" | sed 's/-\{2,\}/-/g;s/^-*//;s/-*$//')
  [[ -z "$s" ]] && s="repo"
  case "$s" in system|admin|internal|reserved|ailoop) s="git-${s}" ;; esac
  [[ "$s" =~ ^[a-zA-Z0-9] ]] || s="r${s}"
  [[ ${#s} -gt 64 ]] && s="${s:0:64}"
  printf '%s' "$s"
}

channel_from_origin() {
  local url path_part org rest repo
  url="$(git remote get-url origin 2>/dev/null)" || true
  if [[ -z "$url" ]]; then
    sanitize_channel_slug "$(basename "$TOP")"
    return
  fi
  path_part=""
  if [[ "$url" =~ ^git@[^:]+:(.+)$ ]]; then path_part="${BASH_REMATCH[1]}"
  elif [[ "$url" =~ ^ssh://[^/]+/(.+)$ ]]; then path_part="${BASH_REMATCH[1]}"
  elif [[ "$url" =~ ^https?://[^/]+/(.+)$ ]]; then path_part="${BASH_REMATCH[1]}"
  else path_part="${url#*:}"
  fi
  path_part="${path_part%.git}"
  org="${path_part%%/*}"
  rest="${path_part#*/}"
  repo="${rest%%/*}"
  if [[ -z "$repo" || "$repo" == "$path_part" ]]; then
    sanitize_channel_slug "$(basename "$TOP")"
    return
  fi
  sanitize_channel_slug "${org}-${repo}"
}

CHANNEL="$(channel_from_origin)"
WORKDIR="$(basename "$TOP")"

SHA="$(git rev-parse HEAD 2>/dev/null)" || exit 0
SHORT="$(git rev-parse --short HEAD 2>/dev/null)"

TITLE="$(git log -1 --format=%s HEAD 2>/dev/null)"
TITLE="${TITLE//\"/}"

BODY="$(git log -1 --format=%b HEAD 2>/dev/null | tr '\n' ' ' | sed 's/[[:space:]]\{2,\}/ /g' | cut -c1-4000)"
BODY="${BODY//\"/}"

TEXT="git-hook ${EVENT} worktree=${WORKDIR} commit=${SHA} short=${SHORT} title=${TITLE} message=${BODY}"
ailoop say "$TEXT" --channel "$CHANNEL" --server http://127.0.0.1:8080 2>/dev/null || true
exit 0
