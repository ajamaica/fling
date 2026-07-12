#!/usr/bin/env bash
set -euo pipefail

reject_symlinked_components_below_home() {
    local target="$1" relative component part
    case "$target" in
        "$HOME"/*) relative="${target#"$HOME"/}" ;;
        *) echo "Refusing uninstall path outside HOME: $target" >&2; exit 1 ;;
    esac

    component="$HOME"
    while [ -n "$relative" ]; do
        part="${relative%%/*}"
        component="$component/$part"
        [ ! -L "$component" ] || {
            echo "Refusing symlinked uninstall path component: $component" >&2
            exit 1
        }
        [ "$relative" = "$part" ] && break
        relative="${relative#*/}"
    done
}

APP_DIR="$HOME/.local/share/fling-ui"
LAUNCHER="$HOME/.local/bin/fling-ui"
DESKTOP="$HOME/.local/share/applications/fling-ui.desktop"
for target in "$APP_DIR" "$LAUNCHER" "$DESKTOP"; do
    reject_symlinked_components_below_home "$target"
done
rm -rf "$APP_DIR"
for target in "$LAUNCHER" "$DESKTOP"; do
    reject_symlinked_components_below_home "$target"
done
rm -f "$LAUNCHER" "$DESKTOP"
echo "Removed Fling UI. Downloaded trainers were preserved in $HOME/Trainers."
