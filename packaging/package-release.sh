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
CLI_BINARY="${FLING_CLI_BINARY:-$ROOT/target/x86_64-unknown-linux-gnu/release/fling-rs}"
[ -f "$CLI_BINARY" ] && [ ! -L "$CLI_BINARY" ] || { echo "Missing prebuilt Rust CLI; build fling-rs for x86_64 Linux first" >&2; exit 2; }
python3 - "$CLI_BINARY" <<'PY' || { echo "Rust CLI must be a 64-bit little-endian x86-64 ELF artifact" >&2; exit 2; }
import pathlib, struct, sys
data = pathlib.Path(sys.argv[1]).read_bytes()
valid = False
if len(data) >= 64 and data[:7] == b'\x7fELF\x02\x01\x01':
    e_type, machine, version = struct.unpack_from('<HHI', data, 16)
    entry, phoff = struct.unpack_from('<QQ', data, 24)
    ehsize, phentsize, phnum = struct.unpack_from('<HHH', data, 52)
    end = phoff + phentsize * phnum
    valid = (e_type in (2, 3) and machine == 62 and version == 1 and entry != 0 and
             ehsize == 64 and phentsize == 56 and phnum > 0 and phoff >= 64 and
             end >= phoff and end <= len(data))
    executable_load = False
    if valid:
        for offset in range(phoff, end, phentsize):
            p_type, flags = struct.unpack_from('<II', data, offset)
            file_offset, file_size = struct.unpack_from('<Q', data, offset + 8)[0], struct.unpack_from('<Q', data, offset + 32)[0]
            file_end = file_offset + file_size
            if p_type == 1 and flags & 1 and file_end >= file_offset and file_end <= len(data):
                executable_load = True
        valid = executable_load
raise SystemExit(0 if valid else 1)
PY
stage="$(mktemp -d "${TMPDIR:-/tmp}/fling-package.XXXXXXXX")"
cleanup() { rm -rf -- "$stage"; }
trap cleanup EXIT HUP INT TERM
bundle="$stage/fling-linux-x86_64"
mkdir -p "$bundle/bin" "$bundle/systemd" "$bundle/packaging" "$bundle/ui-export"
install -m 0755 "$ROOT/bin/fling" "$bundle/bin/fling"
install -m 0755 "$CLI_BINARY" "$bundle/bin/fling-rs"
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
