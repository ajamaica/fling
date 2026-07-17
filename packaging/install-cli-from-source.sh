#!/usr/bin/env bash
# Install the Fling CLI and user service from a trusted source checkout/bundle.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$HOME/.local/bin"
UNIT_DIR="$HOME/.config/systemd/user"
ENV_DIR="$HOME/.config/environment.d"
ENV_CONF="$ENV_DIR/10-fling-trainers.conf"
PT_ID="com.github.Matoking.protontricks"
say()  { printf '>>> %s\n' "$*"; }
warn() { printf '!!! %s\n' "$*"; }
die()  { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

reject_symlinked_components_below_home() {
    local target="$1" relative component part
    case "$target" in "$HOME"/*) relative="${target#"$HOME"/}" ;; *) die "install path is outside HOME: $target" ;; esac
    component="$HOME"
    while [ -n "$relative" ]; do
        part="${relative%%/*}"
        component="$component/$part"
        [ ! -L "$component" ] || die "refusing symlinked install path component: $component"
        [ "$relative" = "$part" ] && break
        relative="${relative#*/}"
    done
}
guard_targets() {
    reject_symlinked_components_below_home "$BIN_DIR/fling"
    reject_symlinked_components_below_home "$BIN_DIR/fling-rs"
    reject_symlinked_components_below_home "$ENV_CONF"
    reject_symlinked_components_below_home "$UNIT_DIR/fling-watch.service"
}

CLI_BINARY="${FLING_CLI_BINARY:-$REPO_DIR/bin/fling-rs}"
if [ ! -f "$CLI_BINARY" ]; then
    command -v cargo >/dev/null || die "bundle is missing the prebuilt fling-rs binary"
    say "building Rust CLI from source"
    cargo build --manifest-path "$REPO_DIR/Cargo.toml" --release
    CLI_BINARY="$REPO_DIR/target/release/fling-rs"
fi

missing=()
for c in curl jq python3 file busctl systemctl; do command -v "$c" >/dev/null || missing+=("$c"); done
[ ${#missing[@]} -eq 0 ] || die "missing required tools: ${missing[*]} (install them and re-run)"
PT_FLATPAK=0
if command -v protontricks-launch >/dev/null; then
    say "protontricks: native"
elif command -v flatpak >/dev/null && flatpak info "$PT_ID" >/dev/null 2>&1; then
    say "protontricks: Flatpak"
    PT_FLATPAK=1
else
    die "protontricks is required (install the distro package or user Flatpak $PT_ID)"
fi
command -v xdotool >/dev/null && command -v xprop >/dev/null || warn "xdotool/xprop unavailable; Gaming Mode auto-tagging will be skipped"

guard_targets
mkdir -p "$BIN_DIR" "$ENV_DIR" "$UNIT_DIR"
guard_targets
install -m 0755 "$REPO_DIR/bin/fling" "$BIN_DIR/fling"
install -m 0755 "$CLI_BINARY" "$BIN_DIR/fling-rs"
guard_targets
cat > "$ENV_CONF" <<'EOF'
# Managed by Fling. Make Proton games expose their launcher service.
STEAM_COMPAT_LAUNCHER_SERVICE=proton
EOF
systemctl --user set-environment STEAM_COMPAT_LAUNCHER_SERVICE=proton 2>/dev/null || true
if [ "$PT_FLATPAK" = 1 ]; then
    STEAM_ROOT="$("$BIN_DIR/fling" _steamroot 2>/dev/null || echo "$HOME/.local/share/Steam")"
    flatpak override --user --filesystem="$STEAM_ROOT" "$PT_ID" || true
    flatpak override --user --filesystem="$HOME/Trainers:ro" "$PT_ID" || true
    flatpak override --user --talk-name='com.steampowered.*' "$PT_ID" || true
fi
guard_targets
install -m 0644 "$REPO_DIR/systemd/fling-watch.service" "$UNIT_DIR/fling-watch.service"
systemctl --user daemon-reload
systemctl --user enable --now fling-watch.service
say "Fling CLI and watcher installed"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) warn "$BIN_DIR is not in PATH" ;; esac
