#!/usr/bin/env bash
set -euo pipefail

: "${WOT_CI_CACHE:?}"
: "${GITHUB_OUTPUT:?}"
: "${GITHUB_STEP_SUMMARY:?}"

game_dir="$WOT_CI_CACHE/eu-sd-en"
bundle_root="$WOT_CI_CACHE/extracted-client"
mkdir -p "$game_dir" "$bundle_root"

# Hold the lock until the bundle has been published for this installed client.
exec 9>"$WOT_CI_CACHE/game-client.lock"
flock 9

if [[ ! -f "$game_dir/.wg-launcher/state.json" ]]; then
  wg-launcher install --game wot --region eu --client-type sd \
    --language EN --output "$game_dir"
fi
wg-launcher update --path "$game_dir"

version=$(python3 - "$game_dir/version.xml" <<'PY'
import re
import sys
from xml.etree import ElementTree

version = (ElementTree.parse(sys.argv[1]).getroot().findtext("version") or "").strip()
if not re.fullmatch(r"(?:v\.)?[0-9]+(?:\.[0-9]+){3}(?:[ \t]+#[0-9]+)?", version):
    sys.exit("Missing or invalid game client version in version.xml")
print(version)
PY
)
version_key=$(printf '%s' "$version" | sha256sum | cut -d ' ' -f 1)
resources="$bundle_root/$version_key"
executable_sha=$(sha256sum "$game_dir/WorldOfTanks.exe" | cut -d ' ' -f 1)
status=reused
if [[ ! -e "$resources" ]]; then
  staging=$(mktemp -d "$bundle_root/.extract.XXXXXX")
  trap 'rm -rf -- "$staging"' EXIT
  cargo +stable run --locked -p wot-extract -- "$game_dir" --output "$staging/resources"
  jq -e --arg version "$version" '.game_version == $version' "$staging/resources/manifest.json" >/dev/null
  mv -- "$staging/resources" "$resources"
  status=created
fi

jq -e --arg version "$version" '.game_version == $version' "$resources/manifest.json" >/dev/null
{
  echo "game-version=$version"
  echo "resources=$resources"
} >> "$GITHUB_OUTPUT"
printf 'EU client %s (%s), resource bundle %s.\n' "$version" "$executable_sha" "$status" |
  tee -a "$GITHUB_STEP_SUMMARY"
