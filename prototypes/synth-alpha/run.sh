#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_file="$root/prototypes/synth-alpha/resource_probe.rs"
binary="$(mktemp "${TMPDIR:-/tmp}/crump-synth-probe.XXXXXX")"
trap 'rm -f "$binary"' EXIT

RUSTC_VERSION="$(rustc --version)" \
RUSTC_FLAGS="--edition 2024 -C opt-level=3 -C target-cpu=native" \
PROBE_SOURCE_SHA256="$(sha256sum "$source_file" | cut -d' ' -f1)" \
HOST_IDENTITY="$(uname -srmo)" \
rustc --edition 2024 -C opt-level=3 -C target-cpu=native "$source_file" -o "$binary"
"$binary"
