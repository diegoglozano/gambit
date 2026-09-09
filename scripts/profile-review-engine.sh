#!/usr/bin/env bash
set -euo pipefail
repository_root=$(git rev-parse --show-toplevel)
cd "$repository_root"
nodes=${1:-100000}
if [[ ! "$nodes" =~ ^[1-9][0-9]*$ ]]; then
  echo 'node budget must be a positive integer' >&2
  exit 2
fi
report_directory=${2:-"$repository_root/target/engine-profile-$nodes"}
engine=${GAMBIT_ENGINE_PATH:-"$repository_root/apps/gambit-desktop/src-tauri/binaries/gambit-stockfish-universal-apple-darwin"}
fixture=${3:-"$repository_root/benchmarks/engine/kasparov-deep-blue-1997.pgn"}
player=${4:-'Garry Kasparov'}
shared_ply=${5:-8}
if [[ $# -gt 5 || ! "$shared_ply" =~ ^[0-9]+$ ]]; then
  echo 'usage: profile-review-engine.sh [NODES [REPORT_DIRECTORY [PGN [PLAYER [SHARED_PLY]]]]]' >&2
  exit 2
fi
mkdir -p "$report_directory"
test -x "$engine"
jq -n --arg cpu "$(sysctl -n machdep.cpu.brand_string)" \
  --arg architecture "$(uname -m)" --arg os "$(sw_vers -productVersion)" \
  --arg translated "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg commit "$(git rev-parse HEAD)" \
  --argjson dirty "$(if [[ -n "$(git status --porcelain)" ]]; then echo true; else echo false; fi)" \
  --arg harness_sha256 "$(shasum -a 256 crates/gambit-engine/examples/review_profile.rs crates/gambit-engine/examples/support/mod.rs crates/gambit-engine/src/lib.rs crates/gambit-engine/src/position.rs crates/gambit-chess/src/position.rs | shasum -a 256 | awk '{print $1}')" \
  --arg engine_sha256 "$(shasum -a 256 "$engine" | awk '{print $1}')" \
  --arg fixture_sha256 "$(shasum -a 256 "$fixture" | awk '{print $1}')" \
  --argjson nodes "$nodes" --argjson shared_ply "$shared_ply" \
  '{cpu:$cpu, architecture:$architecture, macos:$os, translated:$translated,
    date:$date, commit:$commit, working_tree_dirty:$dirty, harness_sha256:$harness_sha256,
    engine_sha256:$engine_sha256, fixture_sha256:$fixture_sha256,
    nodes_per_search:$nodes, shared_ply:$shared_ply}' > "$report_directory/machine.json"
cargo run --release -p gambit-engine --example review_profile -- \
  "$engine" "$fixture" "$player" "$shared_ply" "$nodes" \
  | tee "$report_directory/review.jsonl" | jq --unbuffered -c 'del(.samples, .initial_fen, .mainline)'
