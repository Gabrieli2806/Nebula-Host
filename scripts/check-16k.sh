#!/usr/bin/env bash
# Report which native libraries in an Android package are not aligned for
# 16 KB memory pages. Google Play stops accepting updates without 16 KB
# support on 2027-02-01, and a 4 KB-aligned library fails to load on a device
# with 16 KB pages.
#
#   scripts/check-16k.sh <app.apk | app.aab | folder> [--strict]
#
# Every ELF `LOAD` segment must be aligned to at least 16384 bytes (0x4000).
# Checks the libraries under lib/ and those the app unpacks itself from its
# assets (the bundled Java runtimes). Prints one line per library; in GitHub
# Actions a misaligned one is also a warning annotation. With --strict the
# script fails when any library is misaligned.
#
# Needs: unzip (for a package), readelf (binutils or llvm-readelf).
set -euo pipefail

target="${1:?usage: check-16k.sh <apk|aab|folder> [--strict]}"
strict="${2:-}"

readelf_bin="$(command -v readelf || command -v llvm-readelf || true)"
if [ -z "$readelf_bin" ]; then
  echo "readelf not found (install binutils or llvm)" >&2
  exit 2
fi

work=""
cleanup() { if [ -n "$work" ]; then rm -rf "$work"; fi; }
trap cleanup EXIT

if [ -d "$target" ]; then
  root="$target"
else
  work="$(mktemp -d)"
  unzip -qq -o "$target" '*.so' '*.so.*' -d "$work" 2>/dev/null || true
  root="$work"
fi

total=0
bad=0
while IFS= read -r -d '' lib; do
  # Skip anything that is not an ELF file (a stray name match).
  if ! head -c 4 "$lib" | grep -q $'\x7fELF'; then
    continue
  fi
  total=$((total + 1))
  # The smallest alignment among the LOAD segments decides.
  min_align="$("$readelf_bin" -lW "$lib" 2>/dev/null | awk '$1 == "LOAD" { print $NF }' | while read -r a; do printf '%d\n' "$a"; done | sort -n | head -1)"
  rel="${lib#"$root"/}"
  if [ -z "$min_align" ]; then
    echo "??    $rel (no LOAD segments)"
    continue
  fi
  if [ "$min_align" -lt 16384 ]; then
    bad=$((bad + 1))
    echo "FAIL  $rel (aligned to $min_align)"
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
      echo "::warning title=Not 16 KB aligned::$rel is aligned to $min_align bytes"
    fi
  else
    echo "ok    $rel ($min_align)"
  fi
done < <(find "$root" -type f \( -name '*.so' -o -name '*.so.*' \) -print0 | sort -z)

echo
echo "$total native libraries, $bad not aligned for 16 KB pages."
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  echo "16 KB page alignment: $total native libraries checked, $bad not aligned." >> "$GITHUB_STEP_SUMMARY"
fi
if [ "$strict" = "--strict" ] && [ "$bad" -gt 0 ]; then
  exit 1
fi
