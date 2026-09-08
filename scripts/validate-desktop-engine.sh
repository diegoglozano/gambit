#!/usr/bin/env bash
# Run on native Intel and Apple Silicon machines against the SAME release DMG.
set -euo pipefail
repository_root=$(git rev-parse --show-toplevel)
dmg=${1:?usage: validate-desktop-engine.sh DMG REPORT_DIRECTORY}
report_directory=${2:?usage: validate-desktop-engine.sh DMG REPORT_DIRECTORY}
mkdir -p "$report_directory" "$repository_root/target"
dmg="$(cd "$(dirname "$dmg")" && pwd)/$(basename "$dmg")"
report_directory=$(cd "$report_directory" && pwd)
cd "$repository_root"
mount_directory=$(mktemp -d "$repository_root/target/engine-mount.XXXXXX")
source_directory=$(mktemp -d "$repository_root/target/engine-rebuild.XXXXXX")
trap 'hdiutil detach "$mount_directory" >/dev/null 2>&1 || true; rmdir "$mount_directory" 2>/dev/null || true; rm -rf -- "$source_directory"' EXIT
hdiutil attach "$dmg" -readonly -nobrowse -mountpoint "$mount_directory"
app="$mount_directory/Gambit.app"
bash "$repository_root/scripts/smoke-desktop-engine.sh" "$app" | tee "$report_directory/smoke.txt"
export GAMBIT_ENGINE_PATH="$app/Contents/MacOS/gambit-stockfish"
cargo test -p gambit-engine --test process packaged_stockfish -- --ignored
bash "$repository_root/scripts/profile-review-engine.sh" 100000 "$report_directory"
tar -xzf "$app/Contents/Resources/engine/stockfish-17.1-source.tar.gz" -C "$source_directory"
case "$(uname -m)" in
  arm64) build_arch=apple-silicon ;;
  x86_64) build_arch=x86-64 ;;
  *) echo 'unsupported validation architecture' >&2; exit 2 ;;
esac
make -C "$source_directory/src" -j2 build "ARCH=$build_arch" COMP=clang > "$report_directory/source-build.txt" 2>&1
GAMBIT_ENGINE_PATH="$source_directory/src/stockfish" \
  cargo test -p gambit-engine --test process packaged_stockfish -- --ignored
cargo run --release -p gambit-engine --example profile -- "$source_directory/src/stockfish" > "$report_directory/source-probe.txt"
shasum -a 256 "$dmg" > "$report_directory/dmg.sha256"
