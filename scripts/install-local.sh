#!/usr/bin/env bash
# Build Rustcord and install it for the current user, including a launcher entry.
#
# Re-run this after pulling changes: it rebuilds and replaces the installed
# binary, icon and .desktop file in place, so the launcher always starts the
# current build.
#
# Overridable for packaging or a non-standard layout:
#   BIN_DIR=… APPS_DIR=… ICON_DIR=… scripts/install-local.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
bin_dir="${BIN_DIR:-$HOME/.local/bin}"
apps_dir="${APPS_DIR:-$data_dir/applications}"
icon_dir="${ICON_DIR:-$data_dir/icons/hicolor/scalable/apps}"

echo "building the release binary…"
cargo build --release --manifest-path "$root/Cargo.toml"

install -Dm755 "$root/target/release/rustcord" "$bin_dir/rustcord"
install -Dm644 "$root/assets/rustcord.svg" "$icon_dir/rustcord.svg"

# An absolute Exec path: a launcher started by the session may not have
# ~/.local/bin on PATH, and `rustcord` would then silently fail to start.
entry="$(mktemp)"
trap 'rm -f "$entry"' EXIT
sed "s|^Exec=.*|Exec=$bin_dir/rustcord|" "$root/packaging/rustcord.desktop" >"$entry"
install -Dm644 "$entry" "$apps_dir/rustcord.desktop"

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$apps_dir/rustcord.desktop"
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$apps_dir" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "${icon_dir%/scalable/apps}" 2>/dev/null || true
fi

echo
echo "binary:   $bin_dir/rustcord"
echo "launcher: $apps_dir/rustcord.desktop"
echo "icon:     $icon_dir/rustcord.svg"
