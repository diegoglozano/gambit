# Six-game engine workload

The fixture contains the six factual game scores from the 1997 Kasparov–Deep
Blue match, with no commentary. Imported from the
[python-chess PGN fixtures](https://github.com/niklasf/python-chess/blob/master/data/pgn/kasparov-deep-blue-1997.pgn)
on September 7, 2026. It has 519 plies and exercises both player perspectives,
castling, middlegames, endgames, short and long games.

This is a reproducible **performance workload**, not six losses sharing an
opening, and not a turning-point quality test. Production budget selection still
requires representative review sets from online players on both supported CPU
architectures. Never infer a chess lesson from this benchmark.

The default player is Garry Kasparov, starting after ply 8. The profiler performs
two fixed-node searches for each of 235 player decisions: before the move and
after the played move, 470 searches in total. It searches every decision, without
stopping at a proposed turning point. Each search uses a fresh process, one
thread and 16 MiB hash, with a 30-second watchdog. This measures the current
boundary's full traversal cost, including startup. Schema 2 preserves the
starting FEN and every legal mainline move before each search, including
repetition history. Setup games cannot recover history preceding their FEN.
It deliberately performs no threshold selection or cache writes.

```sh
bash scripts/prepare-desktop-engine.sh
bash scripts/profile-review-engine.sh 100000
bash scripts/profile-review-engine.sh 300000
```

Reports are written under `target/engine-profile-NODES/`. `machine.json` records
CPU, OS, architecture, translation status, commit, dirty-tree status, and
engine/fixture/harness SHA-256.
`review.jsonl` contains a start record, progressive game records, and a final
summary. Each completed decision retains its FEN, played UCI move, before/after
scores, best moves and PVs; each game also retains its initial FEN and full
mainline so the search position can be reconstructed by ply. Console progress
omits this position evidence. Raw root scores and player-normalized scores both
retain mate type, search depth, and exact/lower/upper-bound provenance. Bounds
reverse with player perspective. On an engine failure the affected game's remaining moves are
skipped, later games are attempted, the summary is incomplete, and the process
exits nonzero. An interrupted run has no complete summary and must not be
reported as a successful benchmark.

For a private real review set, use exactly six complete standard games with the
same named player (on either side):

```sh
cargo run --release -p gambit-engine --example review_profile -- \
  ENGINE SIX_GAME_PGN 'Player name' SHARED_PLY NODES
```

To include machine metadata for that private run, use
`bash scripts/profile-review-engine.sh NODES REPORT_DIRECTORY PGN 'Player name' SHARED_PLY`.
The read-only `gambit` example `export_review LIBRARY ID1 ID2 ID3 ID4 ID5 ID6`
can export an existing saved queue to stdout; direct it to an ignored local
path and keep the result out of CI.

This runs locally. Do not upload reports or PGNs from private review sets; CI
uses only the public fixture. File framing and replay are bounded; unsupported
variants, illegal SAN, missing/ambiguous players, or incomplete six-game inputs
fail before an engine is launched. Shared ply counts mainline half-moves from
the PGN start, including games with a setup FEN; variation moves are ignored.

`scripts/validate-desktop-engine.sh DMG REPORT_DIRECTORY` mounts a release DMG,
checks its engine and source contents, runs the real-engine tests and six-game
profiles at 100,000 and 300,000 nodes against the packaged executable, and rebuilds/tests its corresponding
source. The desktop-release workflow runs it on native Apple Silicon and Intel
runners against the same DMG. Both jobs must pass before desktop assets can be
published. CI completion proves executable compatibility and records timings;
it does not automatically approve a production node budget or validate chess
thresholds.

## Development measurements

The measurements below used the original FEN-only schema 1 harness. They are
historical baseline costs, not history-aware diagnosis evidence. Do not compare
their scores directly with schema 2; repetition changes the engine input.

Apple M3, macOS 26.0.1, native arm64, one thread, 16 MiB hash, fresh engine
process per search, including startup; node budgets are listed per run:

| Workload | Nodes/search | Decisions | Searches | Total | Failed games |
| --- | ---: | ---: | ---: | ---: | ---: |
| Public historical six-game fixture, shared ply 8 | 100,000 | 235 | 470 | 177.027 s | 0 |
| Local saved six-loss queue, shared ply 4 | 100,000 | 209 | 418 | 145.708 s | 0 |
| Local saved six-loss queue, shared ply 4 | 300,000 | 209 | 418 | 204.334 s | 0 |

The historical run retained 331 bounded scores and 139 exact scores. This distinction
must survive future diagnosis and practice logic: a bound is not an exact
centipawn estimate. Raw game/position evidence for the local queue is not
committed or uploaded. These development timings are not a finalized production
budget and do not substitute for the native Intel measurements.

## Native CI measurements

[Validation run 34269084469](https://github.com/diegoglozano/gambit/actions/runs/34269084469)
tested the same packaged engine SHA-256 on both native architectures
(`c3e16f4854502924492857e547185e7f500326bf17e9e085dcb48192681ceaae`).
The public fixture and fresh-process settings above were unchanged:

| Runner CPU | Nodes/search | Completed games | Total | Status |
| --- | ---: | ---: | ---: | --- |
| Apple M1 (Virtual), macOS 14.8.9 | 100,000 | 6 | 253.223 s | Complete, no failures |
| Apple M1 (Virtual), macOS 14.8.9 | 300,000 | 6 | 371.299 s | Complete, no failures |
| Intel i7-8700B, macOS 15.7.9 | 100,000 | 6 | 659.657 s | Complete, no failures |
| Intel i7-8700B, macOS 15.7.9 | 300,000 | 5 | — | Interrupted by job timeout |

Packaged-engine integration tests passed on both architectures. The Apple
Silicon source rebuild and tests also passed; the Intel job never reached its
source rebuild. The 30-minute job limit interrupted the second sequential
workload after five completed games (1,007.843 seconds), not a per-search
watchdog failure. Validation now allows 60 minutes for both workloads and the
source rebuild; the 30-second per-search watchdog remains unchanged. The
interrupted run is not a passing Intel validation or a completed 300,000-node
measurement. Hosted runner timings are not a user-session latency guarantee;
the full-traversal cost needs further work before a production budget is fixed.
