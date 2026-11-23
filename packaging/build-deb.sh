#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
version=$(awk -F '"' '/^version = / { print $2; exit }' "$repo_root/Cargo.toml")
architecture=${CRUMP_DEB_ARCHITECTURE:-amd64}
output_dir=${1:-"$repo_root/dist"}
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-0}
package_root=$(mktemp -d)
assembly=
cleanup() {
  rm -rf "$package_root"
  if [[ -n $assembly ]]; then
    rm -rf "$assembly"
  fi
}
trap cleanup EXIT

if [[ $(uname -m) != x86_64 || $architecture != amd64 ]]; then
  printf 'crump: package builds require an x86_64 host and amd64 target\n' >&2
  exit 2
fi

cargo build --manifest-path "$repo_root/Cargo.toml" --release --locked

install -Dm755 "$repo_root/target/release/crump" "$package_root/usr/bin/crump"
install -Dm644 "$repo_root/LICENSE" "$package_root/usr/share/doc/crump/copyright"
install -Dm644 "$repo_root/LICENSE" "$package_root/usr/share/doc/crump/LICENSE"
install -Dm644 "$repo_root/README.md" "$package_root/usr/share/doc/crump/README.md"
install -Dm644 "$repo_root/CONTEXT.md" "$package_root/usr/share/doc/crump/CONTEXT.md"
install -Dm644 "$repo_root/requirements/product-contract.md" \
  "$package_root/usr/share/doc/crump/requirements/product-contract.md"
if git -C "$repo_root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  while IFS= read -r -d '' relative; do
    install -Dm644 "$repo_root/$relative" "$package_root/usr/share/doc/crump/$relative"
  done < <(git -C "$repo_root" ls-files -z -- 'docs/*.md')
else
  while IFS= read -r -d '' document; do
    relative=${document#"$repo_root/"}
    install -Dm644 "$document" "$package_root/usr/share/doc/crump/$relative"
  done < <(find "$repo_root/docs" -type f -name '*.md' -print0)
fi
while IFS= read -r -d '' document; do
  relative=${document#"$repo_root/"}
  install -Dm644 "$document" "$package_root/usr/share/doc/crump/$relative"
done < <(find "$repo_root/docs/cookbook" -type f -name '*.md' -print0)
for relative in \
  docs/adr/0012-prepared-sample-transform-contract.md \
  docs/adr/0013-audio-derived-control-and-stutter.md \
  docs/verification.md; do
  install -Dm644 "$repo_root/$relative" "$package_root/usr/share/doc/crump/$relative"
done
install -Dm644 "$repo_root/docs/cookbook/feasibility.json" \
  "$package_root/usr/share/doc/crump/docs/cookbook/feasibility.json"
while IFS= read -r -d '' fixture; do
  relative=${fixture#"$repo_root/fixtures/cookbook/"}
  install -Dm644 "$fixture" \
    "$package_root/usr/share/doc/crump/fixtures/cookbook/$relative"
done < <(find "$repo_root/fixtures/cookbook" -type f -print0)
for fixture_set in reference expressive production; do
  if git -C "$repo_root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    while IFS= read -r -d '' relative; do
      install -Dm644 "$repo_root/$relative" "$package_root/usr/share/doc/crump/$relative"
    done < <(git -C "$repo_root" ls-files -z -- "fixtures/$fixture_set/*")
  else
    while IFS= read -r -d '' fixture; do
      relative=${fixture#"$repo_root/"}
      install -Dm644 "$fixture" "$package_root/usr/share/doc/crump/$relative"
    done < <(find "$repo_root/fixtures/$fixture_set" -type f ! -path '*/.crump/*' -print0)
  fi
done
for fixture_set in starter deep-house-reference; do
  unexpected=$(find "$repo_root/fixtures/$fixture_set" \
    \( -path '*/.crump' -o -path '*/render' \) -prune -o \
    ! -type d ! -type f -print -quit)
  if [[ -n $unexpected ]]; then
    printf 'crump: refusing non-regular fixture path: %s\n' "$unexpected" >&2
    exit 1
  fi
done
patches=(
  dh-bass dh-evolving-texture dh-granular-texture dh-keys dh-lead dh-pad
  dh-percussion dh-spectral-transform dh-transition
)
assets=(basic-table.wav multisample-high.wav multisample-low.wav sample-source.wav)
fixture_files=(
  fixtures/starter/README.md
  fixtures/starter/auditions.crump
  fixtures/starter/manifest.json
  fixtures/deep-house-reference/README.md
  fixtures/deep-house-reference/project.crump
  fixtures/deep-house-reference/render-evidence.json
  fixtures/deep-house-reference/media/manifest.json
  fixtures/synth-alpha/README.md
  fixtures/synth-alpha/generate_assets.py
  fixtures/synth-alpha/generate_max_fixture.py
  fixtures/synth-alpha/hybrid-synth.crump
  fixtures/synth-alpha/manifest.json
  fixtures/synth-alpha/maximum-granular-spectral.crump
  fixtures/synth-alpha/maximum-topology.crump
  fixtures/synth-alpha/minimal-granular.crump
  fixtures/synth-alpha/minimal-multisample.crump
  fixtures/synth-alpha/minimal-sample-source.crump
  fixtures/synth-alpha/minimal-spectral.crump
  fixtures/synth-alpha/minimal-wavetable.crump
  fixtures/synth-alpha/wavetable-demo.crump
)
for patch in "${patches[@]}"; do
  fixture_files+=(
    "fixtures/starter/instruments/$patch.crump"
    "fixtures/deep-house-reference/instruments/$patch.crump"
  )
done
for asset in "${assets[@]}"; do
  fixture_files+=(
    "fixtures/starter/media/$asset"
    "fixtures/deep-house-reference/media/$asset"
    "fixtures/synth-alpha/media/$asset"
  )
done
for relative in "${fixture_files[@]}"; do
  if [[ ! -f $repo_root/$relative || -L $repo_root/$relative ]]; then
    printf 'crump: missing or non-regular release fixture: %s\n' "$relative" >&2
    exit 1
  fi
  install -Dm644 "$repo_root/$relative" "$package_root/usr/share/doc/crump/$relative"
done
install -Dm644 "$repo_root/fixtures/reference/project.crump" \
  "$package_root/usr/share/doc/crump/examples/reference/project.crump"
for instrument in "$repo_root"/fixtures/reference/instruments/*.crump; do
  install -Dm644 "$instrument" \
    "$package_root/usr/share/doc/crump/examples/reference/instruments/$(basename "$instrument")"
done
install -d "$package_root/DEBIAN"

installed_size=$(du -sk "$package_root/usr" | awk '{ print $1 }')
cat >"$package_root/DEBIAN/control" <<CONTROL
Package: crump
Version: $version
Section: sound
Priority: optional
Architecture: $architecture
Maintainer: Crump contributors
Depends: libc6, libpipewire-0.3-0 (>= 1.2)
Recommends: libegl1, libgl1, libxkbcommon0, libwayland-client0 | libx11-6
Installed-Size: $installed_size
Homepage: https://github.com/NathanaelRea/crump
Description: terminal-first guided production instrument
 Crump authors, auditions, versions, and deterministically renders electronic
 music Projects from readable declarative source and declared WAV assets.
CONTROL

mkdir -p "$output_dir"
output_dir=$(cd -- "$output_dir" && pwd)
output="$output_dir/crump_${version}_${architecture}.deb"
if command -v dpkg-deb >/dev/null 2>&1; then
  dpkg-deb --root-owner-group --build "$package_root" "$output"
else
  for tool in ar tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      printf 'crump: package creation requires dpkg-deb or both GNU ar and GNU tar\n' >&2
      exit 1
    fi
  done
  if ! tar --version 2>/dev/null | head -n 1 | grep -q 'GNU tar'; then
    printf 'crump: the dpkg-deb fallback requires GNU tar\n' >&2
    exit 1
  fi
  assembly=$(mktemp -d)
  printf '2.0\n' >"$assembly/debian-binary"
  tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    -C "$package_root/DEBIAN" -cJf "$assembly/control.tar.xz" .
  tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    --exclude='./DEBIAN' -C "$package_root" -cJf "$assembly/data.tar.xz" .
  rm -f "$output"
  (
    cd "$assembly"
    ar crD "$output" debian-binary control.tar.xz data.tar.xz
  )
  rm -rf "$assembly"
  assembly=
fi
checksum=$(sha256sum "$output" | awk '{ print $1 }')
printf '%s  %s\n' "$checksum" "$(basename -- "$output")" >"$output.sha256"
printf 'Built %s\nChecksum %s\n' "$output" "$output.sha256"
