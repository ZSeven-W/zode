#!/bin/sh
# zode installer — downloads a prebuilt `zode` binary from GitHub Releases.
#
# Quick install (latest release, including betas):
#   curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
#
# Options (flags or env vars):
#   --version <tag>   ZODE_VERSION=v0.1.0-beta.1   pin a release tag (default: latest)
#   --bin-dir <dir>   ZODE_BIN_DIR=~/.local/bin    install location (default: auto)
#
# Examples:
#   curl -fsSL .../install.sh | sh -s -- --version v0.1.0-beta.1
#   curl -fsSL .../install.sh | ZODE_BIN_DIR="$HOME/bin" sh
#
# Supports macOS (arm64/x64) and Linux (arm64/x64). Windows: use install.ps1.
set -eu

REPO="ZSeven-W/zode"
BIN="zode"
VERSION="${ZODE_VERSION:-}"
BIN_DIR="${ZODE_BIN_DIR:-}"

# ---- tiny helpers ---------------------------------------------------------
info() { printf '\033[36m=>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

# ---- parse flags ----------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:?--version needs a tag}"; shift 2 ;;
    --bin-dir) BIN_DIR="${2:?--bin-dir needs a path}"; shift 2 ;;
    -h|--help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) err "unknown argument: $1" ;;
  esac
done

# ---- downloader (curl or wget) --------------------------------------------
if have curl; then
  dl() { curl -fsSL "$1" -o "$2"; }
  dl_stdout() { curl -fsSL "$1"; }
elif have wget; then
  dl() { wget -qO "$2" "$1"; }
  dl_stdout() { wget -qO- "$1"; }
else
  err "need curl or wget to download"
fi

# ---- detect platform ------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin) plat_os="mac" ;;
  Linux)  plat_os="linux" ;;
  MINGW*|MSYS*|CYGWIN*) err "Windows detected — run install.ps1 instead (see README)" ;;
  *) err "unsupported OS: $os" ;;
esac
case "$arch" in
  x86_64|amd64)   plat_arch="x64" ;;
  arm64|aarch64)  plat_arch="arm64" ;;
  *) err "unsupported architecture: $arch" ;;
esac
suffix="${plat_arch}-${plat_os}"   # e.g. arm64-mac, x64-linux

# ---- resolve version (latest release, including pre-releases) -------------
if [ -z "$VERSION" ]; then
  info "resolving latest release..."
  VERSION="$(dl_stdout "https://api.github.com/repos/$REPO/releases" \
    | grep '"tag_name"' | head -1 \
    | sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')"
  [ -n "$VERSION" ] || err "could not resolve latest release tag (set --version)"
fi
ver_num="${VERSION#v}"                       # tag may or may not start with v
asset="${BIN}-${ver_num}-${suffix}.tar.gz"
url="https://github.com/$REPO/releases/download/${VERSION}/${asset}"

# ---- choose install dir ---------------------------------------------------
if [ -z "$BIN_DIR" ]; then
  if [ -w /usr/local/bin ] 2>/dev/null; then
    BIN_DIR="/usr/local/bin"
  else
    BIN_DIR="$HOME/.local/bin"
  fi
fi

# ---- download + extract + install -----------------------------------------
info "installing ${BIN} ${VERSION} (${suffix}) -> ${BIN_DIR}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

dl "$url" "$tmp/$asset" || err "download failed: $url
(check that ${asset} exists for ${VERSION} — see https://github.com/$REPO/releases)"
tar -xzf "$tmp/$asset" -C "$tmp" || err "failed to extract $asset"
[ -f "$tmp/$BIN" ] || err "archive did not contain a '$BIN' binary"

mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/$BIN" "$BIN_DIR/$BIN" 2>/dev/null \
  || { cp "$tmp/$BIN" "$BIN_DIR/$BIN" && chmod 0755 "$BIN_DIR/$BIN"; } \
  || err "could not write to $BIN_DIR (try: ZODE_BIN_DIR=\$HOME/.local/bin)"

info "installed: $BIN_DIR/$BIN"

# macOS: strip the Gatekeeper quarantine flag so the (ad-hoc-signed, un-notarized)
# binary runs without an "unidentified developer" prompt. No-op if not present.
if [ "$plat_os" = "mac" ]; then
  xattr -dr com.apple.quarantine "$BIN_DIR/$BIN" 2>/dev/null || true
fi

# ---- PATH hint + sanity check ---------------------------------------------
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$BIN_DIR is not on your PATH. Add it, e.g.:
    echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.profile && . ~/.profile" ;;
esac

if "$BIN_DIR/$BIN" --version >/dev/null 2>&1; then
  info "done — run '$BIN' to start, or '$BIN --help'"
else
  info "done — run '$BIN_DIR/$BIN' to start"
fi
