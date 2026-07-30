#!/usr/bin/env bash
set -euo pipefail

prototype_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
manifest="$prototype_dir/Cargo.toml"

if [[ "${1:-}" == "compare" ]]; then
  cargo run --quiet --release --manifest-path "$manifest" -- --demo
  printf '\nThe hard-swap file should expose clicks; the other two should stay continuous.\n'
  for version in crossfade zero-crossing hard-swap; do
    read -r -p "Press Enter to play $version..."
    pw-play "$prototype_dir/demo-output/$version.wav"
  done
  exit
fi

version="${1:-}"
if [[ -z "$version" ]]; then
  printf '%s\n' \
    'Choose a graph-replacement version:' \
    '  1. crossfade' \
    '  2. zero-crossing' \
    '  3. hard-swap (control case)'
  read -r -p '> ' choice
  case "$choice" in
    1) version="crossfade" ;;
    2) version="zero-crossing" ;;
    3) version="hard-swap" ;;
    *) printf 'Unknown choice: %s\n' "$choice" >&2; exit 2 ;;
  esac
fi

cargo run --quiet --release --manifest-path "$manifest" -- "$version"
