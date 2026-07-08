#!/bin/sh
# Native installer for the peektrace CLI.
#
# Downloads the prebuilt standalone binary for the host platform from the
# Mark-Life/peektrace GitHub Releases and installs it as `peektrace`. No Node,
# npm, or Bun is required on the target machine.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Mark-Life/peektrace/main/install.sh | sh
#
# Optional environment overrides (all have sensible defaults):
#   PEEKTRACE_VERSION      Pin a release tag, e.g. cli-v1.2.3 (default: newest cli-v*).
#   PEEKTRACE_INSTALL_DIR  Install directory (default: $HOME/.local/bin).
#   PEEKTRACE_BASE_URL     Release-download base (default: the GitHub releases URL).
#   PEEKTRACE_GITHUB_API   Repo API base (default: https://api.github.com/repos/Mark-Life/peektrace).
#
# The download URL is always composed as: "$PEEKTRACE_BASE_URL/$tag/$asset".
# Because the base is a variable, the script is fully testable against a local
# HTTP server by pointing PEEKTRACE_BASE_URL at it and pinning PEEKTRACE_VERSION.

set -eu

# --- configuration (env-overridable) ----------------------------------------

BASE_URL="${PEEKTRACE_BASE_URL:-https://github.com/Mark-Life/peektrace/releases/download}"
GITHUB_API="${PEEKTRACE_GITHUB_API:-https://api.github.com/repos/Mark-Life/peektrace}"
INSTALL_DIR="${PEEKTRACE_INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="peektrace"

# --- helpers -----------------------------------------------------------------

info() { printf '%s\n' "$*"; }
warn() { printf '%s\n' "$*" >&2; }
err() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

# Emit the npm fallback hint and abort.
fail_unsupported() {
  warn "error: $1"
  warn ""
  warn "The native installer has no prebuilt binary for your platform."
  warn "Install via npm instead (requires Node.js >= 20):"
  warn ""
  warn "  npm i -g peektrace"
  warn ""
  exit 1
}

# Download URL ($1) to file ($2) using curl, falling back to wget.
download() {
  _url="$1"
  _dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$_url" -o "$_dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$_dest" "$_url"
  else
    err "neither curl nor wget is available; cannot download $_url"
  fi
}

# Fetch URL ($1) to stdout using curl, falling back to wget.
fetch() {
  _url="$1"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$_url"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "$_url"
  else
    err "neither curl nor wget is available; cannot fetch $_url"
  fi
}

# --- platform detection ------------------------------------------------------

detect_asset() {
  _os="$(uname -s)"
  _arch="$(uname -m)"
  case "$_os" in
    Darwin)
      case "$_arch" in
        arm64 | aarch64) ASSET="peektrace-darwin-arm64" ;;
        x86_64 | amd64) ASSET="peektrace-darwin-x64" ;;
        *) fail_unsupported "unsupported macOS architecture: $_arch" ;;
      esac
      ;;
    Linux)
      case "$_arch" in
        x86_64 | amd64) ASSET="peektrace-linux-x64" ;;
        *) fail_unsupported "unsupported Linux architecture: $_arch (only linux-x64 has a prebuilt binary)" ;;
      esac
      ;;
    *)
      fail_unsupported "unsupported operating system: $_os"
      ;;
  esac
}

# --- version resolution ------------------------------------------------------

# Resolve the release tag into TAG. Uses PEEKTRACE_VERSION when set, otherwise
# picks the newest `cli-v*` tag_name from the GitHub releases API (no jq).
resolve_tag() {
  if [ -n "${PEEKTRACE_VERSION:-}" ]; then
    TAG="$PEEKTRACE_VERSION"
    info "Using pinned version: $TAG"
    return
  fi

  info "Resolving latest peektrace CLI release..."
  _releases="$(fetch "$GITHUB_API/releases")" ||
    err "failed to query GitHub API at $GITHUB_API/releases"

  # Extract the first (newest — the API returns releases newest-first) tag_name
  # that begins with cli-v. Matches: "tag_name": "cli-v1.2.3"
  TAG="$(
    printf '%s\n' "$_releases" |
      grep -o '"tag_name"[[:space:]]*:[[:space:]]*"cli-v[^"]*"' |
      head -n 1 |
      sed -e 's/.*"cli-v/cli-v/' -e 's/"$//'
  )"

  if [ -z "$TAG" ]; then
    err "could not find a cli-v* release via $GITHUB_API/releases"
  fi
  info "Latest version: $TAG"
}

# --- checksum verification ---------------------------------------------------

# Verify that $1 (downloaded file) matches its entry for $ASSET in $2 (SHA256SUMS).
verify_checksum() {
  _file="$1"
  _sums="$2"

  # Pull the expected hash for our asset filename from the SHA256SUMS file.
  # Lines look like: "<hash>  peektrace-linux-x64"
  _expected="$(
    grep -E "[[:space:]]$ASSET\$" "$_sums" |
      head -n 1 |
      awk '{ print $1 }'
  )"
  if [ -z "$_expected" ]; then
    err "no SHA256SUMS entry found for $ASSET; refusing to install"
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    _actual="$(sha256sum "$_file" | awk '{ print $1 }')"
  elif command -v shasum >/dev/null 2>&1; then
    _actual="$(shasum -a 256 "$_file" | awk '{ print $1 }')"
  else
    err "neither sha256sum nor shasum is available; cannot verify download"
  fi

  if [ "$_expected" != "$_actual" ]; then
    err "checksum mismatch for $ASSET
  expected: $_expected
  actual:   $_actual
Aborting without installing."
  fi
  info "Checksum verified."
}

# --- PATH guidance -----------------------------------------------------------

# Print shell-appropriate instructions if INSTALL_DIR is not already on PATH.
print_path_hint() {
  case ":$PATH:" in
    *":$INSTALL_DIR:"*)
      return
      ;;
  esac

  _shell_name="$(basename "${SHELL:-sh}")"
  info ""
  info "NOTE: $INSTALL_DIR is not on your PATH."
  case "$_shell_name" in
    zsh)
      info "Add it by running:"
      info "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> \"\$HOME/.zshrc\" && source \"\$HOME/.zshrc\""
      ;;
    bash)
      info "Add it by running:"
      info "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> \"\$HOME/.bashrc\" && source \"\$HOME/.bashrc\""
      ;;
    fish)
      info "Add it by running:"
      info "  fish_add_path \"$INSTALL_DIR\""
      ;;
    *)
      info "Add this line to your shell profile:"
      info "  export PATH=\"$INSTALL_DIR:\$PATH\""
      ;;
  esac
}

# --- main --------------------------------------------------------------------

main() {
  detect_asset
  resolve_tag

  _url="$BASE_URL/$TAG/$ASSET"
  _sums_url="$BASE_URL/$TAG/SHA256SUMS"

  # Temp workspace, cleaned up on any exit.
  TMPDIR_PEEK="$(mktemp -d 2>/dev/null || mktemp -d -t peektrace)"
  trap 'rm -rf "$TMPDIR_PEEK"' EXIT INT TERM

  _bin_tmp="$TMPDIR_PEEK/$ASSET"
  _sums_tmp="$TMPDIR_PEEK/SHA256SUMS"

  info "Downloading $ASSET ($TAG)..."
  download "$_url" "$_bin_tmp" ||
    err "download failed: $_url"
  download "$_sums_url" "$_sums_tmp" ||
    err "download failed: $_sums_url"

  verify_checksum "$_bin_tmp" "$_sums_tmp"

  info "Installing to $INSTALL_DIR/$BIN_NAME..."
  mkdir -p "$INSTALL_DIR"
  # Move into place then chmod so the exec bit is guaranteed regardless of umask.
  mv "$_bin_tmp" "$INSTALL_DIR/$BIN_NAME"
  chmod 0755 "$INSTALL_DIR/$BIN_NAME"

  info ""
  info "Installed peektrace ($TAG) -> $INSTALL_DIR/$BIN_NAME"
  print_path_hint
  info ""
  info "Get started:"
  info "  peektrace serve"
}

main
