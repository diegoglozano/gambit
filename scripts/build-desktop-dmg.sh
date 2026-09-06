#!/usr/bin/env bash
set -euo pipefail

version=${1:?usage: build-desktop-dmg.sh VERSION}
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid desktop version: $version" >&2
  exit 2
fi

repository_root=$(git rev-parse --show-toplevel)
desktop_root="$repository_root/apps/gambit-desktop"
asset_directory="$repository_root/target/desktop-release"
asset_name="gambit-desktop-universal-apple-darwin.dmg"
updater_name="gambit-desktop-universal-apple-darwin.app.tar.gz"

mkdir -p "$asset_directory"
rm -f \
  "$asset_directory/$asset_name" \
  "$asset_directory/$asset_name.sha256" \
  "$asset_directory/$updater_name" \
  "$asset_directory/$updater_name.sig" \
  "$asset_directory/latest.json"

rustup target add aarch64-apple-darwin x86_64-apple-darwin

cd "$desktop_root"
build_config="{\"version\":\"$version\"}"
bundle_targets="dmg"
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  build_config="{\"version\":\"$version\",\"bundle\":{\"createUpdaterArtifacts\":false}}"
else
  bundle_targets="dmg,app"
fi
npx --yes @tauri-apps/cli@2.11.4 build \
  --bundles "$bundle_targets" \
  --target universal-apple-darwin \
  --config "$build_config"

dmg_path=$(find src-tauri/target/universal-apple-darwin/release/bundle/dmg \
  -maxdepth 1 -type f -name '*.dmg' -print -quit)
if [[ -z "$dmg_path" ]]; then
  echo "Tauri did not produce a DMG" >&2
  exit 1
fi

cp "$dmg_path" "$asset_directory/$asset_name"
(
  cd "$asset_directory"
  shasum -a 256 "$asset_name" > "$asset_name.sha256"
)

if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  updater_path=$(find src-tauri/target/universal-apple-darwin/release/bundle/macos \
    -maxdepth 1 -type f -name '*.app.tar.gz' -print -quit)
  if [[ -z "$updater_path" || ! -f "$updater_path.sig" ]]; then
    echo "Tauri did not produce signed updater artifacts" >&2
    exit 1
  fi
  cp "$updater_path" "$asset_directory/$updater_name"
  cp "$updater_path.sig" "$asset_directory/$updater_name.sig"
  signature=$(tr -d '\r\n' < "$updater_path.sig")
  download_url="https://github.com/diegoglozano/gambit/releases/latest/download/$updater_name"
  jq -n \
    --arg version "$version" \
    --arg notes "Gambit $version is available. Download and restart to update." \
    --arg url "$download_url" \
    --arg signature "$signature" \
    '{
      version: $version,
      notes: $notes,
      platforms: {
        "darwin-aarch64": { url: $url, signature: $signature },
        "darwin-x86_64": { url: $url, signature: $signature }
      }
    }' > "$asset_directory/latest.json"
fi

echo "desktop release assets:"
echo "  $asset_directory/$asset_name"
echo "  $asset_directory/$asset_name.sha256"
if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo "  $asset_directory/$updater_name"
  echo "  $asset_directory/$updater_name.sig"
  echo "  $asset_directory/latest.json"
fi
