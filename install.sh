#!/usr/bin/env bash
# Builds and installs Crump for the current user: the binary into ~/.cargo/bin and a hidden
# desktop entry plus icon under ~/.local/share so the window gets its name and icon.
# Never uses sudo; missing system packages are reported, not installed.
# Usage: ./install.sh [--uninstall]
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
desktop_file="$data_home/applications/crump.desktop"
icon_file="$data_home/icons/hicolor/512x512/apps/crump.png"

refresh_caches() {
  if command -v update-desktop-database >/dev/null; then
    update-desktop-database -q "$data_home/applications" || true
  fi
  if command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache -q -t "$data_home/icons/hicolor" || true
  fi
}

if [[ ${1:-} == --uninstall ]]; then
  if command -v cargo >/dev/null && cargo install --list | grep '^crump ' >/dev/null; then
    cargo uninstall crump
  fi
  rm -f "$desktop_file" "$icon_file"
  refresh_caches
  printf 'crump: uninstalled\n'
  exit 0
elif [[ $# -gt 0 ]]; then
  printf 'usage: %s [--uninstall]\n' "$0" >&2
  exit 2
fi

missing=()
command -v cargo >/dev/null || missing+=(cargo)
command -v pkg-config >/dev/null || missing+=(pkg-config)
command -v cc >/dev/null || missing+=(c-compiler)
if command -v pkg-config >/dev/null && ! pkg-config --exists libpipewire-0.3; then
  missing+=(pipewire-headers)
fi
# bindgen loads libclang at build time; look for it the way clang-sys does.
if ! { [[ -n ${LIBCLANG_PATH:-} ]] || ldconfig -p 2>/dev/null | grep 'libclang[-.0-9]*\.so' >/dev/null; }; then
  missing+=(libclang)
fi

if ((${#missing[@]})); then
  printf 'crump: missing build prerequisites: %s\n' "${missing[*]}" >&2
  if [[ " ${missing[*]} " == *" cargo "* ]]; then
    printf '  Rust: https://rustup.rs\n' >&2
  fi
  if command -v dnf >/dev/null; then
    printf '  sudo dnf install gcc pkgconf-pkg-config pipewire-devel clang-devel\n' >&2
  elif command -v apt-get >/dev/null; then
    printf '  sudo apt install build-essential pkg-config libpipewire-0.3-dev libclang-dev\n' >&2
  elif command -v pacman >/dev/null; then
    printf '  sudo pacman -S base-devel pkgconf pipewire clang\n' >&2
  fi
  exit 1
fi

cargo install --path "$repo_root" --locked

install -Dm644 "$repo_root/assets/branding/icon.png" "$icon_file"
mkdir -p "$(dirname -- "$desktop_file")"
# NoDisplay: the window needs a Project directory, so a menu launcher would only fail. The entry
# exists so the compositor can match the window's app_id ("crump") to a name and icon.
cat >"$desktop_file" <<'EOF'
[Desktop Entry]
Type=Application
Name=Crump
Comment=Text-first DAW
Exec=crump
Icon=crump
Terminal=false
NoDisplay=true
StartupWMClass=crump
Categories=AudioVideo;Audio;
EOF
refresh_caches

bin_dir=${CARGO_HOME:-"$HOME/.cargo"}/bin
if [[ ":$PATH:" != *":$bin_dir:"* ]]; then
  printf 'crump: add %s to PATH to run crump\n' "$bin_dir"
fi
printf 'crump: installed. Try: crump new my-sketch && cd my-sketch && crump\n'
