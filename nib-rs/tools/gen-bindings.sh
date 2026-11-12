#!/usr/bin/env bash
set -euo pipefail
# usage: gen-bindings.sh <path-to-libnib_ffi.so|dylib> <out-dir>
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="${1:?missing library path}"
OUT="${2:?missing out dir}"
mkdir -p "$OUT"
cd "$SCRIPT_DIR/.."
cargo run --quiet -p uniffi-bindgen -- \
  generate "$LIB" --language kotlin --out-dir "$OUT" --no-format
