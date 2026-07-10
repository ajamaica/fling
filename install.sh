#!/usr/bin/env bash
# fling installer — sets up the FLiNG trainer manager for Steam/Proton on Linux.
# Idempotent: safe to re-run (e.g. after an update).
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"
UNIT_DIR="$HOME/.config/systemd/user"
ENV_DIR="$HOME/.config/environment.d"
ENV_CONF="$ENV_DIR/10-fling-trainers.conf"
PT_ID="com.github.Matoking.protontricks"

say()  { printf '\033[1;36m>>>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!!\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# --- dependency checks -------------------------------------------------------
say "Checking dependencies..."
missing=()
for c in curl jq python3 file busctl systemctl; do command -v "$c" >/dev/null || missing+=("$c"); done
[ ${#missing[@]} -eq 0 ] || die "missing required tools: ${missing[*]} (install them and re-run)"

# protontricks: native binary OR flatpak
PT_FLATPAK=0
if command -v protontricks-launch >/dev/null; then
    say "protontricks: native ✓"
elif flatpak info "$PT_ID" >/dev/null 2>&1; then
    say "protontricks: Flatpak ✓"
    PT_FLATPAK=1
else
    warn "protontricks not found. Install it first:"
    warn "  native:  your distro's 'protontricks' package"
    warn "  flatpak: flatpak install --user flathub $PT_ID"
    die "protontricks is required"
fi

# xdotool/xprop are only needed for Gaming Mode window tagging — optional
command -v xdotool >/dev/null && command -v xprop >/dev/null \
    || warn "xdotool/xprop not found — Gaming Mode window auto-tagging will be skipped (desktop mode still fine)"

# --- install the CLI ---------------------------------------------------------
say "Installing fling to $BIN_DIR ..."
mkdir -p "$BIN_DIR"
install -m 0755 "$REPO_DIR/bin/fling" "$BIN_DIR/fling"

case ":$PATH:" in
    *":$BIN_DIR:"*) : ;;
    *) warn "$BIN_DIR is not in your PATH — add it to your shell profile:"
       warn '  export PATH="$HOME/.local/bin:$PATH"' ;;
esac

# --- global injection env ----------------------------------------------------
say "Enabling global trainer-injection env..."
mkdir -p "$ENV_DIR"
cat > "$ENV_CONF" <<'EOF'
# Make every Proton game start a Steam Runtime launcher service so FLiNG
# trainers (via fling-watch) can be injected into the game's own container.
# Managed by `fling`.
STEAM_COMPAT_LAUNCHER_SERVICE=proton
EOF
systemctl --user set-environment STEAM_COMPAT_LAUNCHER_SERVICE=proton 2>/dev/null || true

# --- flatpak permissions (only if protontricks is a Flatpak) -----------------
if [ "$PT_FLATPAK" = 1 ]; then
    say "Granting protontricks Flatpak the needed access..."
    # find the Steam root the CLI will use, to grant filesystem access
    STEAM_ROOT="$("$BIN_DIR/fling" _steamroot 2>/dev/null || echo "$HOME/.local/share/Steam")"
    flatpak override --user --filesystem="$STEAM_ROOT" "$PT_ID" || true
    flatpak override --user --filesystem="$HOME/Trainers:ro" "$PT_ID" || true
    flatpak override --user --talk-name="com.steampowered.*" "$PT_ID" || true
fi

# --- systemd auto-launch service ---------------------------------------------
say "Installing + enabling fling-watch.service..."
mkdir -p "$UNIT_DIR"
install -m 0644 "$REPO_DIR/systemd/fling-watch.service" "$UNIT_DIR/fling-watch.service"
systemctl --user daemon-reload
systemctl --user enable --now fling-watch.service

say "Installed. ✓"
echo
say "One-time activation: the running Steam predates the new env var. Either:"
say "  • reboot, or"
say "  • fling restart-steam   (bounces the gamescope Steam session)"
echo
say "Then, for any game:  fling auto \"<game name>\"  → launch it → trainer auto-attaches."
