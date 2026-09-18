#!/bin/sh
# fastskill installer script for Linux and macOS.
#
#   curl -fsSL https://github.com/gofastskill/fastskill/releases/latest/download/install.sh | sh
#   curl -fsSL https://github.com/gofastskill/fastskill/releases/download/v1.4.2/install.sh | sh
#
# Environment (all optional):
#   FASTSKILL_VERSION             stable (default) | latest | 1.4.2
#   FASTSKILL_INSTALL_DIR         bin dir (default: $XDG_BIN_HOME or ~/.local/bin)
#   FASTSKILL_NO_MODIFY_PATH=1    leave shell rc files untouched
#   FASTSKILL_UNMANAGED=1         no PATH edit, no receipt (images, CI)
#   FASTSKILL_INSTALLER_BASE_URL  https mirror serving the same asset names
#                                   (plain http only for 127.0.0.1, localhost, [::1])
#   FASTSKILL_GITHUB_TOKEN / GITHUB_TOKEN   private GitHub releases (header
#                                   only; never sent to a mirror)
#   FASTSKILL_INSTALL_ALLOW_SUDO=1          allow running as root
#
# This script only downloads and verifies. Placement, PATH and the install
# receipt are done by `fastskill self install`, so this file rarely changes.
# The body is a function called on the last line: a truncated download runs
# nothing.

main() {
  set -eu

  say() { printf '%s\n' "$*" >&2; }
  die() { say "error: $*"; exit 1; }

  APP="fastskill"
  REPO="gofastskill/fastskill"
  TAG_PREFIX="v"
  # How the self group is reached: "self", or "cli self" when the app puts
  # its built-in commands under a namespace.
  SELF_CMD="cli self"
  VERSION="${FASTSKILL_VERSION:-stable}"
  TOKEN="${FASTSKILL_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}"
  MIRROR="${FASTSKILL_INSTALLER_BASE_URL:-}"
  PROTO='=https'
  if [ -n "$MIRROR" ]; then
    # A mirror never receives the GitHub token.
    TOKEN=""
    case "$MIRROR" in
      https://*) ;;
      http://127.0.0.1[:/]*|http://127.0.0.1|http://localhost[:/]*|http://localhost|http://\[::1\][:/]*|http://\[::1\])
        PROTO='=http,https' ;;
      *) die "FASTSKILL_INSTALLER_BASE_URL must be https (plain http is allowed only for a loopback address)" ;;
    esac
  fi

  if [ "$(id -u)" = "0" ] && [ -z "${FASTSKILL_INSTALL_ALLOW_SUDO:-}" ]; then
    die "refusing to install as root: it would land in root's home. Set FASTSKILL_INSTALL_ALLOW_SUDO=1 to override."
  fi

  os=$(uname -s); arch=$(uname -m)
  case "$os" in
    Linux)  os_part="unknown-linux-musl" ;;
    Darwin)
      os_part="apple-darwin"
      # An x86_64 shell under Rosetta on Apple silicon still gets the native build.
      if [ "$arch" = "x86_64" ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
        arch="arm64"
      fi ;;
    *) die "unsupported OS: $os (on Windows use install.ps1)" ;;
  esac
  case "$arch" in
    x86_64|amd64)  arch_part="x86_64" ;;
    aarch64|arm64) arch_part="aarch64" ;;
    *) die "unsupported architecture: $arch" ;;
  esac
  target="${arch_part}-${os_part}"
  asset="${APP}-${target}.tar.gz"

  command -v tar >/dev/null 2>&1 || die "'tar' is required"
  # fetch URL OUT [ACCEPT]. Redirects must stay on https (curl); the token
  # goes only in a header, never in a URL.
  if command -v curl >/dev/null 2>&1; then
    fetch() {
      if [ -n "$TOKEN" ]; then
        curl -fsSL --proto "$PROTO" --proto-redir '=https' --retry 3 -H "Authorization: Bearer ${TOKEN}" -H "Accept: ${3:-application/octet-stream}" -o "$2" "$1"
      else
        curl -fsSL --proto "$PROTO" --proto-redir '=https' --retry 3 -o "$2" "$1"
      fi
    }
  elif command -v wget >/dev/null 2>&1; then
    # GNU wget can refuse http redirects; BusyBox wget has no such flag.
    https_only=""
    if [ "$PROTO" = '=https' ] && wget --help 2>&1 | grep -q -- '--https-only'; then
      https_only="--https-only"
    fi
    fetch() {
      if [ -n "$TOKEN" ]; then
        wget -q $https_only --header="Authorization: Bearer ${TOKEN}" --header="Accept: ${3:-application/octet-stream}" -O "$2" "$1"
      else
        wget -q $https_only -O "$2" "$1"
      fi
    }
  else
    die "curl or wget is required"
  fi

  # Same rule as the binary: XDG_BIN_HOME counts only when absolute.
  case "${XDG_BIN_HOME:-}" in
    /*) default_bin="$XDG_BIN_HOME" ;;
    *)  default_bin="$HOME/.local/bin" ;;
  esac
  bin_dir="${FASTSKILL_INSTALL_DIR:-$default_bin}"
  mkdir -p "$bin_dir" || die "cannot create ${bin_dir}"
  # Temp dir inside the bin dir: same filesystem for the final rename, and
  # immune to a noexec /tmp.
  tmp=$(mktemp -d "${bin_dir}/.${APP}-install.XXXXXX") || die "cannot create a temp dir in ${bin_dir}"
  trap 'rm -rf "$tmp"' EXIT INT TERM

  if [ -n "$MIRROR" ]; then
    base="${MIRROR%/}"
    case "$VERSION" in
      stable|latest)
        fetch "${base}/latest.json" "${tmp}/latest.json"
        v=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "${tmp}/latest.json" | head -n 1)
        [ -n "$v" ] || die "could not read version from ${base}/latest.json"
        base="${base}/${TAG_PREFIX}${v}" ;;
      *) base="${base}/${TAG_PREFIX}${VERSION#v}" ;;
    esac
    archive_url="${base}/${asset}"; sums_url="${base}/SHA256SUMS"
  elif [ -n "$TOKEN" ]; then
    # Private repo: asset downloads go through the API by asset id.
    api="https://api.github.com/repos/${REPO}/releases"
    case "$VERSION" in
      stable) rel="${api}/latest" ;;
      latest) rel="${api}?per_page=1" ;;
      *)      rel="${api}/tags/${TAG_PREFIX}${VERSION#v}" ;;
    esac
    fetch "$rel" "${tmp}/release.json" "application/vnd.github+json"
    # One JSON member per line, compact or pretty-printed alike. An asset's
    # own "id" is the last one before its "name" (the uploader's comes after).
    asset_id() {
      tr ',{}[' '\n\n\n\n' < "${tmp}/release.json" | awk -v n="$1" '
        /^[[:space:]]*"id":/   { v = $0; gsub(/[^0-9]/, "", v); id = v }
        /^[[:space:]]*"name":/ { if (index($0, "\"" n "\"")) { print id; exit } }'
    }
    a_id=$(asset_id "$asset"); s_id=$(asset_id "SHA256SUMS")
    [ -n "$a_id" ] && [ -n "$s_id" ] || die "release assets not found for ${asset}"
    archive_url="${api}/assets/${a_id}"; sums_url="${api}/assets/${s_id}"
  else
    case "$VERSION" in
      stable) base="https://github.com/${REPO}/releases/latest/download" ;;
      latest)
        tag=$(fetch "https://api.github.com/repos/${REPO}/releases?per_page=1" "${tmp}/rel.json" "application/vnd.github+json" && sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "${tmp}/rel.json" | head -n 1)
        [ -n "$tag" ] || die "could not resolve the latest release tag"
        base="https://github.com/${REPO}/releases/download/${tag}" ;;
      *) base="https://github.com/${REPO}/releases/download/${TAG_PREFIX}${VERSION#v}" ;;
    esac
    archive_url="${base}/${asset}"; sums_url="${base}/SHA256SUMS"
  fi

  say "downloading ${asset} (${VERSION})"
  fetch "$archive_url" "${tmp}/${asset}"
  fetch "$sums_url" "${tmp}/SHA256SUMS"

  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "${tmp}/${asset}" | cut -d' ' -f1)
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "${tmp}/${asset}" | cut -d' ' -f1)
  else
    die "sha256sum or shasum is required to verify the download"
  fi
  expected=$(awk -v a="$asset" '$2 == a || $2 == "*" a { print $1 }' "${tmp}/SHA256SUMS")
  [ -n "$expected" ] || die "${asset} is not listed in SHA256SUMS"
  [ "$actual" = "$expected" ] || die "checksum mismatch for ${asset}"

  tar -xzf "${tmp}/${asset}" -C "$tmp" "$APP" || die "archive does not contain ${APP}"
  chmod 755 "${tmp}/${APP}"

  # SELF_CMD is split on purpose: "cli self" is two words.
  # shellcheck disable=SC2086
  set -- $SELF_CMD install --from-bootstrap --bin-dir "$bin_dir"
  [ -n "${FASTSKILL_NO_MODIFY_PATH:-}" ] && set -- "$@" --no-modify-path
  [ -n "${FASTSKILL_UNMANAGED:-}" ] && set -- "$@" --unmanaged

  # Bootstrap mode moves the binary into place; the trap removes the
  # now-empty temp dir.
  "${tmp}/${APP}" "$@"
}

main "$@"
