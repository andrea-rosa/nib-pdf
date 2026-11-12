#!/usr/bin/env bash
set -euo pipefail
# usage: check-elf-alignment.sh <dir | .so | .aar | .apk>...
# Fails unless every LOAD segment of every .so has alignment >= 0x4000 (power of two).

READELF=""
if command -v readelf >/dev/null 2>&1; then
  READELF=readelf
else
  ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
  newest="$(ls -d "$ANDROID_HOME"/ndk/*/toolchains/llvm/prebuilt/*/bin 2>/dev/null | sort -V | tail -1 || true)"
  if [[ -n "$newest" && -x "$newest/llvm-readelf" ]]; then
    READELF="$newest/llvm-readelf"
  fi
fi
[[ -n "$READELF" ]] || { echo "no readelf or NDK llvm-readelf found" >&2; exit 2; }

fail=0
check_so() {
  local f="$1" ok=1
  # 16 KB page-size requirement applies to 64-bit ABIs only; ELF32 is exempt.
  # (capture instead of `grep -q` on a pipe: grep -q exits early, readelf dies
  # with SIGPIPE and `set -o pipefail` then reports the pipeline as failed)
  local hdr
  hdr="$("$READELF" -h "$f")"
  if [[ "$hdr" == *"ELF32"* ]]; then
    echo "OK $f (ELF32, exempt)"
    return 0
  fi
  while read -r line; do
    # columns: Type Offset VirtAddr PhysAddr FileSiz MemSiz Flg Align
    align="$(echo "$line" | awk '{print $NF}')"
    # require hex >= 0x4000 and power of two
    if [[ "$align" =~ ^0x[0-9a-fA-F]+$ ]]; then
      v=$((align))
      if (( v >= 16384 && (v & (v - 1)) == 0 )); then
        continue
      fi
    fi
    echo "FAIL $f (align $align)"
    ok=0
  done < <("$READELF" -lW "$f" | awk '$1=="LOAD"{print}')
  [[ $ok -eq 1 ]] && echo "OK $f"
  return $((1-ok))
}

process() {
  local path="$1"
  if [[ -d "$path" ]]; then
    while IFS= read -r -d '' f; do check_so "$f" || fail=1; done < <(find "$path" -name '*.so' -print0)
  elif [[ "$path" == *.aar || "$path" == *.apk || "$path" == *.zip ]]; then
    local tmp; tmp="$(mktemp -d)"
    unzip -qq -o "$path" -d "$tmp"
    while IFS= read -r -d '' f; do check_so "$f" || fail=1; done < <(find "$tmp" -name '*.so' -print0)
    rm -rf "$tmp"
  elif [[ "$path" == *.so ]]; then
    check_so "$path" || fail=1
  else
    echo "skipping $path (not dir/.so/.aar/.apk)" >&2
  fi
}

[[ $# -ge 1 ]] || { echo "usage: $0 <dir|.so|.aar|.apk>..." >&2; exit 2; }
for p in "$@"; do process "$p"; done
exit "$fail"
