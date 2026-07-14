#!/usr/bin/env bash
# Build a deterministic, checksummed release bundle from a verified Godot export.
set -euo pipefail
usage() { echo "Usage: $0 <verified-linux-export-directory> [output-directory]" >&2; exit 2; }
[ $# -ge 1 ] && [ $# -le 2 ] || usage
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXPORT="$(cd "$1" 2>/dev/null && pwd)" || usage
OUTPUT="${2:-$PWD}"
mkdir -p "$OUTPUT"
OUTPUT="$(cd "$OUTPUT" && pwd)"
[ -z "$(find "$EXPORT" -type l -print -quit)" ] || { echo "Refusing export containing symlinks" >&2; exit 1; }
executable=""
for name in fling-ui fling-ui.x86_64 FlingUi FlingUi.x86_64 'Fling UI' 'Fling UI.x86_64'; do
    [ -f "$EXPORT/$name" ] && [ ! -L "$EXPORT/$name" ] && { executable="$name"; break; }
done
[ -n "$executable" ] || { echo "No supported Linux export executable found" >&2; exit 2; }
for tool in python3 sha256sum mktemp; do command -v "$tool" >/dev/null || { echo "Missing tool: $tool" >&2; exit 1; }; done
stage="$(mktemp -d "${TMPDIR:-/tmp}/fling-package.XXXXXXXX")"
cleanup() { rm -rf -- "$stage"; }
trap cleanup EXIT HUP INT TERM
bundle="$stage/fling-linux-x86_64"
mkdir -p "$bundle/bin" "$bundle/systemd" "$bundle/packaging" "$bundle/ui-export"
install -m 0755 "$ROOT/bin/fling" "$bundle/bin/fling"
install -m 0644 "$ROOT/systemd/fling-watch.service" "$bundle/systemd/fling-watch.service"
install -m 0755 "$ROOT/packaging/install-bundle.sh" "$bundle/install-bundle.sh"
install -m 0755 "$ROOT/packaging/install-cli-from-source.sh" "$bundle/packaging/install-cli-from-source.sh"
install -m 0755 "$ROOT/packaging/install-ui.sh" "$bundle/packaging/install-ui.sh"
cp -R "$EXPORT"/. "$bundle/ui-export"/
chmod 0755 "$bundle/ui-export/$executable"
epoch="${SOURCE_DATE_EPOCH:-0}"
archive="$OUTPUT/fling-linux-x86_64.tar.gz"
python3 - "$stage" "$archive" "$epoch" <<'PY'
import gzip, os, pathlib, sys, tarfile
stage, archive, epoch = pathlib.Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
root = stage / "fling-linux-x86_64"
with open(archive, "wb") as raw:
    with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=epoch) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.GNU_FORMAT) as output:
            for path in [root, *sorted(root.rglob("*"), key=lambda item: item.as_posix())]:
                info = output.gettarinfo(path, arcname=path.relative_to(stage).as_posix())
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.mtime = epoch
                if info.isfile():
                    with path.open("rb") as source:
                        output.addfile(info, source)
                else:
                    output.addfile(info)
PY
(cd "$OUTPUT" && sha256sum fling-linux-x86_64.tar.gz > SHA256SUMS)
printf 'Created %s and %s\n' "$archive" "$OUTPUT/SHA256SUMS"
