#!/usr/bin/env bash
# Point the Flatpak manifest at a release. With no argument it follows
# whatever GitHub currently calls latest.
#
# Flathub runs flatpak-external-data-checker against the manifest's
# x-checker-data block and opens this same bump as a pull request by itself
# once the app is published there. This script is what does the job before
# that, and when a release needs to go out faster than the checker's cycle.
set -euo pipefail

cd "$(dirname "$0")"

repo=x3me/omacal
manifest=io.extremelabs.omacal.yml
metainfo=io.extremelabs.omacal.metainfo.xml

version=${1:-}
if [ -z "$version" ]; then
  version=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" \
    | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -1)
fi
[ -n "$version" ] || { echo "could not determine a version" >&2; exit 1; }

url="https://github.com/$repo/releases/download/v$version/omacal_${version}_amd64.deb"
echo "fetching $url"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fL --progress-bar "$url" -o "$tmp/omacal.deb"
sha=$(sha256sum "$tmp/omacal.deb" | cut -d' ' -f1)
date=$(curl -fsSL "https://api.github.com/repos/$repo/releases/tags/v$version" \
  | sed -n 's/.*"published_at": *"\([0-9-]*\)T.*/\1/p' | head -1)

# Every module but omacal itself comes from shared-modules, so the manifest
# carries exactly one sha256 and a blind rewrite is safe. Check that rather
# than assume it: a second checksum appearing here would otherwise be
# silently overwritten with the .deb's.
count=$(grep -c '^ *sha256: [0-9a-f]\{64\}$' "$manifest")
[ "$count" = 1 ] || { echo "expected 1 sha256 in $manifest, found $count" >&2; exit 1; }

sed -i \
  -e "s|^\( *url: \)https://github.com/$repo/releases/download/.*\.deb$|\1$url|" \
  -e "s|^\( *sha256: \)[0-9a-f]\{64\}$|\1$sha|" \
  "$manifest"

sed -i -e "s|<release version=\"[^\"]*\" date=\"[^\"]*\"/>|<release version=\"$version\" date=\"${date:-$(date -u +%F)}\"/>|" \
  "$metainfo"

echo "manifest now at $version"
grep -n "url: https://github.com/$repo/releases\|sha256:" "$manifest"
grep -n "<release " "$metainfo"
