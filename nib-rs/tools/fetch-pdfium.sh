#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LOCK="$SCRIPT_DIR/pdfium.lock"
DL_DIR="$ROOT/nib-rs/target/pdfium/dl"

MODE=all
case "${1:-}" in
  --host-only) MODE=host ;;
  --android-only) MODE=android ;;
  "" ) ;;
  *) echo "usage: $0 [--host-only|--android-only]" >&2; exit 2 ;;
esac

host_key() {
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) echo "linux-x64" ;;
    Darwin-arm64) echo "mac-arm64" ;;
    Darwin-x86_64) echo "mac-x64" ;;
    *) echo "" ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

TAG=""
want_dest() {
  local dest="$1"
  case "$MODE" in
    all) return 0 ;;
    android) [[ "$dest" == nib-android/* ]] ;;
    host) [[ "$dest" == nib-rs/target/pdfium/* ]] && [[ "$dest" == *"/$(host_key)" ]] ;;
  esac
}

while read -r a b c; do
  [[ -z "${a:-}" || "$a" == \#* ]] && continue
  if [[ "$a" == "tag" ]]; then TAG="$b"; continue; fi
  asset="$a"; expected="$b"; dest_rel="$c"
  want_dest "$dest_rel" || continue
  dest="$ROOT/$dest_rel"
  lib="$dest/libpdfium.so"
  [[ "$asset" == pdfium-mac-* ]] && lib="$dest/libpdfium.dylib"
  stamp="$dest/.pdfium-sha256"
  if [[ -f "$lib" && -f "$stamp" && "$(cat "$stamp")" == "$expected" ]]; then
    echo "up-to-date $asset"
    continue
  fi
  mkdir -p "$dest" "$DL_DIR"
  tgz="$DL_DIR/$asset"
  if [[ ! -f "$tgz" || "$(sha256_of "$tgz")" != "$expected" ]]; then
    curl -fsSL -o "$tgz" "https://github.com/bblanchon/pdfium-binaries/releases/download/$TAG/$asset"
  fi
  actual="$(sha256_of "$tgz")"
  [[ "$actual" == "$expected" ]] || { echo "sha256 mismatch for $asset" >&2; exit 1; }
  tmp="$(mktemp -d)"
  tar -xzf "$tgz" -C "$tmp" lib/libpdfium.* 2>/dev/null || tar -xzf "$tgz" -C "$tmp" "lib/libpdfium.$( [[ $asset == pdfium-mac-* ]] && echo dylib || echo so )"
  mv "$tmp"/lib/libpdfium.* "$lib"
  # Copy bundled LICENSE once into nib-rs/target/pdfium/ if present
  if [[ ! -f "$ROOT/nib-rs/target/pdfium/LICENSE" ]] && tar -tzf "$tgz" | grep -q '^LICENSE$'; then
    tar -xzf "$tgz" -C "$ROOT/nib-rs/target/pdfium" LICENSE
  fi
  rm -rf "$tmp"
  echo "$expected" > "$stamp"
  echo "fetched $asset"
done < "$LOCK"
