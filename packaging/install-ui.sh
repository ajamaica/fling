#!/usr/bin/env bash
set -euo pipefail

usage() { echo "Usage: $0 <linux-export-directory>" >&2; echo "Export the Godot project for Linux first; pass the directory containing the executable and .pck." >&2; exit 2; }
[ $# -eq 1 ] || usage
SOURCE_DIR="$(cd "$1" 2>/dev/null && pwd)" || usage
APP_DIR="$HOME/.local/share/fling-ui"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"

reject_symlinked_components_below_home() {
    local target="$1" relative component part
    case "$target" in
        "$HOME"/*) relative="${target#"$HOME"/}" ;;
        *) echo "Refusing install path outside HOME: $target" >&2; exit 1 ;;
    esac

    component="$HOME"
    while [ -n "$relative" ]; do
        part="${relative%%/*}"
        component="$component/$part"
        [ ! -L "$component" ] || {
            echo "Refusing symlinked install path component: $component" >&2
            exit 1
        }
        [ "$relative" = "$part" ] && break
        relative="${relative#*/}"
    done
}

reject_unsafe_leaf() {
    [ ! -L "$1" ] || { echo "Refusing symlinked install file: $1" >&2; exit 1; }
    [ ! -e "$1" ] || [ -f "$1" ] || { echo "Refusing non-regular install file: $1" >&2; exit 1; }
}

executable=""
for candidate in "$SOURCE_DIR/fling-ui" "$SOURCE_DIR/FlingUi" "$SOURCE_DIR/Fling UI"; do
    [ -f "$candidate" ] && [ ! -L "$candidate" ] && { executable="$(basename "$candidate")"; break; }
done
[ -n "$executable" ] || { echo "No Linux export executable found in $SOURCE_DIR (expected fling-ui, FlingUi, or Fling UI)." >&2; exit 2; }
[ -z "$(find "$SOURCE_DIR" -type l -print -quit)" ] || { echo "Refusing export containing symlinks: $SOURCE_DIR" >&2; exit 1; }

LAUNCHER="$BIN_DIR/fling-ui"
DESKTOP="$DESKTOP_DIR/fling-ui.desktop"
guard_install_targets() {
    local target
    for target in "$APP_DIR" "$LAUNCHER" "$DESKTOP"; do
        reject_symlinked_components_below_home "$target"
    done
}
guard_install_targets
reject_unsafe_leaf "$LAUNCHER"
reject_unsafe_leaf "$DESKTOP"

# These checks are repeated immediately before destructive/write operations.
# A same-user process can still race pathname checks in this Bash installer.
guard_install_targets
mkdir -p "$APP_DIR" "$BIN_DIR" "$DESKTOP_DIR"
guard_install_targets
find "$APP_DIR" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
guard_install_targets
cp -R "$SOURCE_DIR"/. "$APP_DIR"/
guard_install_targets
[ -z "$(find "$APP_DIR" -type l -print -quit)" ] || { echo "Refusing copied export containing symlinks: $APP_DIR" >&2; exit 1; }
reject_symlinked_components_below_home "$APP_DIR/$executable"
chmod +x "$APP_DIR/$executable"
guard_install_targets
reject_unsafe_leaf "$LAUNCHER"
launcher_tmp="$(mktemp "$BIN_DIR/.fling-ui.XXXXXX")"
desktop_tmp=""
cleanup_temps() {
    [ -z "$launcher_tmp" ] || { reject_symlinked_components_below_home "$launcher_tmp"; rm -f -- "$launcher_tmp"; }
    [ -z "$desktop_tmp" ] || { reject_symlinked_components_below_home "$desktop_tmp"; rm -f -- "$desktop_tmp"; }
}
trap cleanup_temps EXIT
cat > "$launcher_tmp" <<EOF
#!/usr/bin/env bash
exec "$APP_DIR/$executable" "\$@"
EOF
chmod 0755 "$launcher_tmp"
reject_unsafe_leaf "$LAUNCHER"
guard_install_targets
mv -f -- "$launcher_tmp" "$LAUNCHER"
launcher_tmp=""
guard_install_targets
reject_unsafe_leaf "$DESKTOP"
desktop_tmp="$(mktemp "$DESKTOP_DIR/.fling-ui.desktop.XXXXXX")"
cat > "$desktop_tmp" <<EOF
[Desktop Entry]
Type=Application
Name=Fling Trainer Manager
Comment=Manage single-player trainers for Steam games
Exec=$BIN_DIR/fling-ui
Terminal=false
Categories=Game;Utility;
StartupNotify=true
EOF
chmod 0644 "$desktop_tmp"
reject_unsafe_leaf "$DESKTOP"
guard_install_targets
mv -f -- "$desktop_tmp" "$DESKTOP"
desktop_tmp=""
trap - EXIT
echo "Installed Fling Trainer Manager in $APP_DIR"
