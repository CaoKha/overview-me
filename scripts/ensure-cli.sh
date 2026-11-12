#!/usr/bin/env bash
# Makes sure <plugin root>/bin/overview-me exists and matches the plugin version.
# Run by the plugin's SessionStart hook; the skills run it too when the binary is missing.
#
# Order: keep a binary of the right version; else download the release for this OS and CPU
# (gh first, so a private repo works with your GitHub login, then curl); else build from
# source with cargo. OVERVIEW_ME_FROM_SOURCE=1 always builds from source (for working on
# overview-me itself with `claude --plugin-dir`).
#
# Prints nothing on success. On failure it says why on stderr and exits 0, so a session
# still starts; the skills then report the missing binary.
set -euo pipefail

REPO="CaoKha/overview-me"
ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
BIN="$ROOT/bin/overview-me"
VERSION="$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$ROOT/.claude-plugin/plugin.json" | head -n 1)"

say() { echo "overview-me: $*" >&2; }

build_from_source() {
  command -v cargo >/dev/null 2>&1 || return 1
  say "building the CLI from source (first time takes a minute)"
  cargo build --release --locked --quiet --manifest-path "$ROOT/Cargo.toml" >&2 || return 1
  mkdir -p "$ROOT/bin" || return 1
  cp "$ROOT/target/release/overview-me" "$BIN"
}

if [ "${OVERVIEW_ME_FROM_SOURCE:-}" = "1" ]; then
  build_from_source || say "cargo build failed"
  exit 0
fi

if [ -x "$BIN" ] && [ "$("$BIN" --version 2>/dev/null)" = "overview-me $VERSION" ]; then
  exit 0
fi

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  Linux-x86_64) target=x86_64-unknown-linux-musl ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-musl ;;
  *) target="" ;;
esac

TMP=""
trap '[ -z "$TMP" ] || rm -rf "$TMP"' EXIT

# `set -e` does not apply inside a function called from `||`, so every step checks itself.
download() {
  [ -n "$target" ] || return 1
  TMP="$(mktemp -d "${TMPDIR:-/tmp}/overview-me.XXXXXX")" || return 1
  local tag="v$VERSION" asset="overview-me-$target.tar.gz" want got
  if ! { command -v gh >/dev/null 2>&1 &&
    gh release download "$tag" -R "$REPO" -p "$asset" -p "$asset.sha256" -D "$TMP" --clobber >/dev/null 2>&1; }; then
    local url="https://github.com/$REPO/releases/download/$tag"
    curl -fsL "$url/$asset" -o "$TMP/$asset" || return 1
    curl -fsL "$url/$asset.sha256" -o "$TMP/$asset.sha256" || return 1
  fi
  want="$(cut -d ' ' -f 1 <"$TMP/$asset.sha256")" || return 1
  if command -v sha256sum >/dev/null 2>&1; then
    got="$(sha256sum "$TMP/$asset" | cut -d ' ' -f 1)"
  else
    got="$(shasum -a 256 "$TMP/$asset" | cut -d ' ' -f 1)"
  fi
  if [ -z "$want" ] || [ "$want" != "$got" ]; then
    say "checksum mismatch for $asset"
    return 1
  fi
  tar -xzf "$TMP/$asset" -C "$TMP" || return 1
  mkdir -p "$ROOT/bin" || return 1
  mv -f "$TMP/overview-me" "$BIN" || return 1
  chmod +x "$BIN"
}

download || build_from_source ||
  say "could not install the CLI: no release v$VERSION for $(uname -s) $(uname -m) and no cargo. Install Rust (https://rustup.rs) or the gh CLI, then start a new session."
exit 0
