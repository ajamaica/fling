#!/usr/bin/env bash
# Inner release-bundle installer. The bootstrap has already verified this tree.
set -euo pipefail
BUNDLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -d "$BUNDLE_DIR/ui-export" ] || { echo "Bundle is missing ui-export" >&2; exit 1; }
/bin/bash "$BUNDLE_DIR/packaging/install-cli-from-source.sh"
/bin/bash "$BUNDLE_DIR/packaging/install-ui.sh" "$BUNDLE_DIR/ui-export"
printf 'Fling UI + CLI installed. Reboot or run `fling restart-steam` before first use.\n'
