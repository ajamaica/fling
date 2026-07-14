#!/usr/bin/env bash
# Standalone installer bootstrap for Fling release bundles.
set -euo pipefail

readonly REPOSITORY="ajamaica/fling"
readonly ARCHIVE="fling-linux-x86_64.tar.gz"
readonly VERSION="${FLING_VERSION:-latest}"

die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
say() { printf '>>> %s\n' "$*"; }

[ "$(uname -s)" = Linux ] || die "Fling supports Linux only"
case "$(uname -m)" in x86_64|amd64) ;; *) die "Fling requires x86_64 Linux" ;; esac
for tool in curl tar sha256sum mktemp awk; do
    command -v "$tool" >/dev/null 2>&1 || die "required tool not found: $tool"
done

case "$VERSION" in
    latest) base="https://github.com/$REPOSITORY/releases/latest/download" ;;
    *[!A-Za-z0-9._-]*|'') die "FLING_VERSION must be a release tag containing only letters, numbers, dot, underscore, or hyphen" ;;
    *) base="https://github.com/$REPOSITORY/releases/download/$VERSION" ;;
esac

work="$(mktemp -d "${TMPDIR:-/tmp}/fling-installer.XXXXXXXX")" || die "could not create temporary directory"
chmod 0700 "$work"
cleanup() { rm -rf -- "$work"; }
trap cleanup EXIT HUP INT TERM

say "Downloading Fling ${VERSION}..."
curl -fL --proto '=https' --tlsv1.2 --retry 3 --connect-timeout 15 \
    -o "$work/$ARCHIVE" "$base/$ARCHIVE"
curl -fL --proto '=https' --tlsv1.2 --retry 3 --connect-timeout 15 \
    -o "$work/SHA256SUMS" "$base/SHA256SUMS"

checksum="$(awk -v name="$ARCHIVE" '$2 == name || $2 == "*" name { print $1 }' "$work/SHA256SUMS")"
[ "$(printf '%s\n' "$checksum" | awk 'NF { count++; if ($0 !~ /^[0-9a-fA-F]{64}$/) bad=1 } END { print count ":" bad }')" = "1:" ] \
    || die "SHA256SUMS must contain exactly one valid checksum for $ARCHIVE"
printf '%s  %s\n' "$checksum" "$ARCHIVE" > "$work/verify.sha256"
(cd "$work" && sha256sum -c verify.sha256) || die "release checksum verification failed"

# Refuse paths that could escape the private extraction directory and links.
tar -tzf "$work/$ARCHIVE" | awk '
    /^\// { exit 1 }
    { n=split($0,p,"/"); for (i=1;i<=n;i++) if (p[i] == "..") exit 1 }
' || die "release archive contains an unsafe path"
tar -tvzf "$work/$ARCHIVE" | awk 'substr($1,1,1) ~ /[lh]/ { exit 1 }' \
    || die "release archive contains links"
tar -xzf "$work/$ARCHIVE" --no-same-owner --no-same-permissions -C "$work"
installer="$work/fling-linux-x86_64/install-bundle.sh"
[ -f "$installer" ] && [ ! -L "$installer" ] || die "release bundle is missing its installer"
say "Installing Fling UI and CLI for the current user..."
/bin/bash "$installer"
