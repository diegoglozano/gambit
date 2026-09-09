#!/usr/bin/env bash
set -euo pipefail
app=${1:?usage: smoke-desktop-engine.sh PATH_TO_GAMBIT_APP}
engine="$app/Contents/MacOS/gambit-stockfish"
lipo "$engine" -verify_arch arm64 x86_64
codesign --verify --strict "$engine"
test -s "$app/Contents/Resources/engine/Stockfish-COPYING.txt"
test -s "$app/Contents/Resources/engine/Stockfish-AUTHORS.txt"
tar -tzf "$app/Contents/Resources/engine/stockfish-17.1-source.tar.gz" \
  ./src/Makefile ./src/nn-1c0000000000.nnue ./src/nn-37f18f62d772.nnue \
  ./Copying.txt ./AUTHORS ./GAMBIT-BUILD.md > /dev/null
"$app/Contents/MacOS/gambit-desktop" --engine-smoke-test
