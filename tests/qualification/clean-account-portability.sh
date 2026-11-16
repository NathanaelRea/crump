#!/bin/sh
# Exercise a complete exact-asset Project and History under a distinct empty account.
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: $0 /absolute/or/relative/path/to/crump" >&2
  exit 2
fi

case "$1" in
/*) crump=$1 ;;
*) crump=$(cd "$(dirname "$1")" && pwd)/$(basename "$1") ;;
esac
if [ ! -x "$crump" ]; then
  echo "crump executable is not executable: $crump" >&2
  exit 2
fi
if [ "$(id -u)" -ne 0 ]; then
  echo "clean-account portability must run as root (for example: sudo $0 $crump)" >&2
  exit 2
fi

repo=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
chmod 755 "$work"
user="crump-qualification-$$"
home="$work/home"
source_project="$work/source-project"
clean_project="$work/clean-project"
cleanup() {
  userdel "$user" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

# The build may live beneath a private checkout or home directory. Both accounts
# must run the same executable bytes without needing access to that directory.
install -m 755 "$crump" "$work/crump"
crump="$work/crump"

mkdir -p "$source_project/media" "$home"
cp "$repo/fixtures/synth-alpha/maximum-topology.crump" "$source_project/project.crump"
cp "$repo/fixtures/synth-alpha/media/"*.wav "$source_project/media/"

"$crump" --project "$source_project" publish \
  --origin system --actor portability-setup \
  --summary "Initial portability publication" \
  --explanation "Prepare complete Project History for clean-account transfer"
"$crump" --project "$source_project" render --output baseline

cp -a "$source_project" "$clean_project"
useradd --home-dir "$home" --no-create-home --shell /bin/sh "$user"
chown -R "$user:$user" "$home" "$clean_project"

as_clean() {
  runuser -u "$user" -- env -i \
    HOME="$home" XDG_CONFIG_HOME="$home/xdg-config" \
    XDG_CACHE_HOME="$home/xdg-cache" XDG_DATA_HOME="$home/xdg-data" \
    "$crump" --project "$clean_project" "$@"
}

compare_exact() {
  if command -v cmp >/dev/null 2>&1; then
    cmp "$1" "$2"
    return
  fi
  left=$(sha256sum "$1" | awk '{print $1}')
  right=$(sha256sum "$2" | awk '{print $1}')
  test "$left" = "$right"
}

as_clean validate --json
as_clean status --json
as_clean inspect --json
as_clean play --plan --json
as_clean render --plan --output planned --json
if [ -e "$clean_project/planned" ]; then
  echo "render --plan mutated the clean-account Project" >&2
  exit 1
fi
as_clean module export qualification-max-v1 --name transferred-maximum
as_clean module inspect transferred-maximum --json
as_clean module preview transferred-maximum \
  --output "$clean_project/audition.wav"
test -s "$clean_project/audition.wav"
as_clean render --output transferred
compare_exact "$source_project/baseline/master.wav" "$clean_project/transferred/master.wav"
compare_exact "$source_project/baseline/manifest.json" "$clean_project/transferred/manifest.json"

as_clean edit target instrument/qualification-max-v1/cutoff-1 1100 \
  --summary "Clean-account portability edit"
as_clean inspect --json
as_clean render --output modified
as_clean history restore 000000000001 \
  --actor portability-test --explanation "Verify exact History recovery after transfer"
as_clean render --output restored
compare_exact "$source_project/baseline/master.wav" "$clean_project/restored/master.wav"
compare_exact "$source_project/baseline/manifest.json" "$clean_project/restored/manifest.json"

printf '%s\n' "clean-account portability: PASS"
