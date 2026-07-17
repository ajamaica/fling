#!/usr/bin/env bash
# fling uninstaller. Leaves downloaded trainers in ~/Trainers by default.
set -euo pipefail
say() { printf '\033[1;36m>>>\033[0m %s\n' "$*"; }

say "Stopping + disabling fling-watch.service..."
systemctl --user disable --now fling-watch.service 2>/dev/null || true
rm -f "$HOME/.config/systemd/user/fling-watch.service"
systemctl --user daemon-reload 2>/dev/null || true

say "Removing global injection env..."
rm -f "$HOME/.config/environment.d/10-fling-trainers.conf"
systemctl --user unset-environment STEAM_COMPAT_LAUNCHER_SERVICE 2>/dev/null || true

say "Removing the fling CLI..."
rm -f "$HOME/.local/bin/fling"
rm -f "$HOME/.local/bin/fling-rs"

if [ "${1:-}" = "--purge" ]; then
    say "Purging downloaded trainers (~/Trainers)..."
    rm -rf "$HOME/Trainers"
else
    say "Kept downloaded trainers in ~/Trainers (use --purge to delete them)."
fi

say "Done. (Flatpak overrides for protontricks were left in place; remove with:"
say "  flatpak override --user --reset com.github.Matoking.protontricks )"
say "Reboot or 'fling restart-steam' equivalent to drop the env from the running Steam."
