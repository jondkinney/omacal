#!/usr/bin/env bash
# Fetch a tzdata release's ICU resource files (little-endian, format 44)
# from unicode-org/icu-data into this directory. No argument: the newest.
set -euo pipefail
cd "$(dirname "$0")"

repo=unicode-org/icu-data
version=${1:-}
if [ -z "$version" ]; then
  version=$(curl -fsSL "https://api.github.com/repos/$repo/contents/tzdata/icunew" \
    | sed -n 's/.*"name": *"\(20[0-9][0-9][a-z]\)".*/\1/p' | sort | tail -1)
fi
[ -n "$version" ] || { echo "could not determine a tzdata version" >&2; exit 1; }

for f in metaZones.res timezoneTypes.res windowsZones.res zoneinfo64.res; do
  echo "fetching $version/44/le/$f"
  curl -fsSL -o "$f" "https://raw.githubusercontent.com/$repo/main/tzdata/icunew/$version/44/le/$f"
done
printf '%s\n' "$version" > VERSION
echo "ICU time zone data is now $version"
