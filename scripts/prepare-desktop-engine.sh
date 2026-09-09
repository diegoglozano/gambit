#!/usr/bin/env bash
# Pinned upstream binaries; no Homebrew or runtime download required.
set -euo pipefail
repository_root=$(git rev-parse --show-toplevel)
cache="$repository_root/target/stockfish-spike"
desktop="$repository_root/apps/gambit-desktop/src-tauri"
mkdir -p "$cache" "$desktop/binaries" "$desktop/engine-resources"

fetch() {
  local name=$1 checksum=$2 url=$3
  if [[ ! -f "$cache/$name" ]]; then
    curl --fail --location --retry 3 --output "$cache/$name.partial" "$url"
    mv "$cache/$name.partial" "$cache/$name"
  fi
  printf '%s  %s\n' "$checksum" "$cache/$name" | shasum -a 256 --check
}
base=https://github.com/official-stockfish/Stockfish/releases/download/sf_17.1
fetch arm64.tar 4e23165eb8f353c221ff7ab6716f0a160c3993dadf90d0c0ad982a7ade4091c9 "$base/stockfish-macos-m1-apple-silicon.tar"
fetch x86_64.tar 067f100a31d3d6f0e45826e6495513c3e0518044e006caa3477993689049d658 "$base/stockfish-macos-x86-64.tar"
fetch nn-1c0000000000.nnue 1c0000000000a67d629999d932d0c373f7450ce43cd12d0562868f4eaf9ae2ad https://raw.githubusercontent.com/official-stockfish/networks/master/nn-1c0000000000.nnue
fetch nn-37f18f62d772.nnue 37f18f62d772f3107e1d6aaca3898c130c3c86f2ab63e6555fbbca20635a899d https://raw.githubusercontent.com/official-stockfish/networks/master/nn-37f18f62d772.nnue

mkdir -p "$cache/arm64" "$cache/x86_64"
tar -xf "$cache/arm64.tar" -C "$cache/arm64"
tar -xf "$cache/x86_64.tar" -C "$cache/x86_64"
cp "$cache/arm64/stockfish/stockfish-macos-m1-apple-silicon" "$desktop/binaries/gambit-stockfish-aarch64-apple-darwin"
cp "$cache/x86_64/stockfish/stockfish-macos-x86-64" "$desktop/binaries/gambit-stockfish-x86_64-apple-darwin"
lipo -create "$desktop/binaries/gambit-stockfish-aarch64-apple-darwin" \
  "$desktop/binaries/gambit-stockfish-x86_64-apple-darwin" \
  -output "$desktop/binaries/gambit-stockfish-universal-apple-darwin"
lipo "$desktop/binaries/gambit-stockfish-universal-apple-darwin" -verify_arch arm64 x86_64
chmod +x "$desktop"/binaries/gambit-stockfish-*

# Ship corresponding source, build scripts and both network files alongside the
# executable, rather than relying only on a third-party download/source offer.
source_stage=$(mktemp -d "$cache/source.XXXXXX")
trap 'rm -rf -- "$source_stage"' EXIT
cp -R "$cache/arm64/stockfish/src" "$source_stage/src"
cp -R "$cache/arm64/stockfish/scripts" "$source_stage/scripts"
cp "$cache/nn-1c0000000000.nnue" "$cache/nn-37f18f62d772.nnue" "$source_stage/src/"
cp "$cache/arm64/stockfish/Copying.txt" "$cache/arm64/stockfish/AUTHORS" \
  "$cache/arm64/stockfish/README.md" "$source_stage/"
cp "$repository_root/docs/stockfish-engine.md" "$source_stage/GAMBIT-BUILD.md"
tar -czf "$desktop/engine-resources/stockfish-17.1-source.tar.gz" -C "$source_stage" .
cp "$cache/arm64/stockfish/Copying.txt" "$desktop/engine-resources/Stockfish-COPYING.txt"
cp "$cache/arm64/stockfish/AUTHORS" "$desktop/engine-resources/Stockfish-AUTHORS.txt"
cp "$repository_root/docs/stockfish-engine.md" "$desktop/engine-resources/Stockfish-NOTICE.md"
shasum -a 256 "$desktop"/binaries/gambit-stockfish-* "$desktop/engine-resources/stockfish-17.1-source.tar.gz"
