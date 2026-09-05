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

rustup target add aarch64-apple-darwin x86_64-apple-darwin

cd "$desktop_root"
npx --yes @tauri-apps/cli@2.11.4 build \
  --bundles dmg \
  --target universal-apple-darwin \
  --config "{\"version\":\"$version\"}"

dmg_path=$(find src-tauri/target/universal-apple-darwin/release/bundle/dmg \
  -maxdepth 1 -type f -name '*.dmg' -print -quit)
if [[ -z "$dmg_path" ]]; then
  echo "Tauri did not produce a DMG" >&2
  exit 1
fi

mkdir -p "$asset_directory"
cp "$dmg_path" "$asset_directory/$asset_name"
(
  cd "$asset_directory"
  shasum -a 256 "$asset_name" > "$asset_name.sha256"
)

echo "desktop release assets:"
echo "  $asset_directory/$asset_name"
echo "  $asset_directory/$asset_name.sha256"
