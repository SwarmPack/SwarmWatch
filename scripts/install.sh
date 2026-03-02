#!/usr/bin/env bash
set -euo pipefail

# SwarmWatch installer (macOS + Linux)
#
# Usage:
#   curl -fsSL https://github.com/SwarmPack/SwarmWatch/releases/latest/download/install.sh | bash
#
# Omni-style behavior:
# - Always install from the GitHub "latest release" endpoint (no custom pinned `latest` release).
# - This script downloads assets from whatever GitHub currently marks as the latest release.

REPO="SwarmPack/SwarmWatch"
BASE="https://github.com/${REPO}/releases/latest/download"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

die() {
  printf "\033[31m✗ %s\033[0m\n" "$*" 1>&2
  exit 1
}

# Minimal Omni-like output (no verbose internals unless SWARMWATCH_INSTALL_VERBOSE=1)
is_verbose() {
  [[ "${SWARMWATCH_INSTALL_VERBOSE:-}" == "1" ]]
}

info() {
  printf "\033[36m➜ %s\033[0m\n" "$*"
}

ok() {
  printf "\033[32m✓ %s\033[0m\n" "$*"
}

warn() {
  printf "\033[33m! %s\033[0m\n" "$*" 1>&2
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing dependency: $1"
}

need_cmd curl
need_cmd tar

info "SwarmWatch installer"

tmpdir="$(mktemp -d)"
cleanup() { rm -rf "$tmpdir"; }
trap cleanup EXIT

asset=""

case "$OS" in
  darwin)
    case "$ARCH" in
      arm64) asset="swarmwatch-macos-arm64.tar.gz" ;;
      x86_64) asset="swarmwatch-macos-x64.tar.gz" ;;
      *) die "unsupported mac arch: $ARCH" ;;
    esac
    ;;
  linux)
    case "$ARCH" in
      x86_64|amd64) asset="swarmwatch-linux-x64.tar.gz" ;;
      *) die "unsupported linux arch: $ARCH (only x64 supported in CI right now)" ;;
    esac
    ;;
  *)
    die "unsupported OS: $OS (use the Windows installer for Windows)"
    ;;
esac

url="${BASE}/${asset}"
info "Downloading latest release…"

curl -fL "$url" -o "$tmpdir/$asset"

info "Extracting…"
tar -xzf "$tmpdir/$asset" -C "$tmpdir"

if [[ "$OS" == "darwin" ]]; then
  # Expect SwarmWatch.app in the tarball.
  app_path="$(find "$tmpdir" -maxdepth 3 -name 'SwarmWatch.app' -print -quit)"
  [[ -n "$app_path" ]] || die "SwarmWatch.app not found in archive"

  info "Installing to /Applications…"
  rm -rf "/Applications/SwarmWatch.app" || true
  cp -R "$app_path" "/Applications/SwarmWatch.app"

  # macOS: clear quarantine attribute so first launch works smoothly.
  # Keep message minimal unless verbose.
  if command -v xattr >/dev/null 2>&1; then
    if is_verbose; then
      info "Finalizing app…"
    fi
    xattr -dr com.apple.quarantine "/Applications/SwarmWatch.app" || true
  fi

  ok "Installed. Open SwarmWatch from /Applications."
else
  # Linux: expect an AppImage.
  appimage="$(find "$tmpdir" -maxdepth 3 -type f -name '*.AppImage' -print -quit)"
  [[ -n "$appimage" ]] || die "AppImage not found in archive"

  install_dir="${XDG_DATA_HOME:-$HOME/.local/share}/SwarmWatch"
  mkdir -p "$install_dir"
  cp "$appimage" "$install_dir/SwarmWatch.AppImage"
  chmod +x "$install_dir/SwarmWatch.AppImage"

  ok "Installed: $install_dir/SwarmWatch.AppImage"
  info "Run it with: $install_dir/SwarmWatch.AppImage"
fi
