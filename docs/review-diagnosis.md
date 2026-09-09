# Local turning-point evidence

The backend can replay one standard game for an explicitly selected player and
find the earliest supported deterioration after a review's shared ply. It is
not yet wired into the desktop UI, and is not a complete
coaching release.

## Durable local records

The cache lives under the app-data `coaching` directory, in a namespace derived
from the canonical library path. It never edits indexed PGN, session/sync
metadata, or legacy review progress. Each key includes stable game identity,
canonical initial FEN/mainline hash, selected player **and color**, shared ply,
the actual engine binary hash and name, architecture, fixed settings, and
cache/selection/evidence-policy versions. Changing any of these inputs cannot
reuse the old result; comments and variations do not invalidate an unchanged
mainline. Old-key files are retained rather than deleted during invalidation.

Records are limited to 1 MiB, parsed strictly, checked against the complete key,
and legally replayed before reuse. Truncated, oversized, inconsistent or corrupt
data is an error, not a successful diagnosis. Corrupt existing practice is not
silently overwritten. Completed/again-later/revealed/solution state is stored
separately from the diagnosis within each record; attempt handling follows in
the practice service.

Saves use a private same-directory temporary file, sync its contents, atomically
replace the one record, and sync the directory on Unix. Read/modify/write
operations are serialized within the desktop process. An identical diagnosis
retry preserves practice; a different result under the same key is rejected.
Cache reads do not create directories or alter library files.

## Input and evidence

`gambit-coaching::ReviewGame` validates at most 256 KiB of PGN and 1,024 mainline
plies. It rejects missing/ambiguous players, unsupported variants, inconsistent
setup tags, illegal mainlines and multiple games. Comments and variations are
not part of the engine history. Empty/short games can honestly have no result.
Shared ply counts half-moves from the supplied game's start, not its FEN move
number. Each player decision preserves the full known history.

`diagnose` is synchronous and must run on a background worker. The caller
supplies a serialized local engine, cancellation and a fixed node budget. It
checks cancellation between searches and never returns partial-game success
after a crash or changed engine identity/settings. Completed earlier games
remain the responsibility of the future queue/cache layer.

## Provisional selection policy (version 1)

- Require a loss of at least 100 centipawns, a newly forced mate against the
  player, or loss of a forced mate for the player.
- Exclude a position already below -300 centipawns or already forced-mated.
- Select the earliest supported candidate, not the largest late error.
- Require a distinct legal preferred move; inconsistent fixed-budget scores
  must not label the preferred move itself an actionable mistake.
- Treat uncertain scores as uncertain. A lower-bound before score and an
  upper-bound after score can establish a **minimum** loss, not an exact loss.
  Unknown eligibility or insufficient inequalities do not establish a candidate.
- Evidence policy 2 retains the immediately previous completed exact iteration
  when a node-limited final report is bounded, but only if that iteration chose
  the same root move. Depth and source stay explicit. A changed root move or
  larger depth gap retains the final bound. “Exact” means an unbounded engine
  estimate at that recorded depth, not a claim of perfect chess knowledge.
- Preserve mate direction explicitly, including terminal mate zero: checkmating
  the opponent must not become “mate against the player” during normalization.

The result records the number of analyzed and inconclusive decisions. A no-result
state means no supported turning point at this budget, not proof that every move
was good. Factual presentation must retain “at least” for bounded losses and
must not infer positional/tactical themes from these numbers.

## Budget and native gate

The engine-packaging milestone passed on both native architectures in
[run 34319662506](https://github.com/diegoglozano/gambit/actions/runs/34319662506).
Both jobs tested and rebuilt source from the same DMG, SHA-256
`7590d1693bc64cbe216a2fd8dd8f0c5ede680c538b3ac8a8d4e815572e9172ae`.

| Public six-game workload | 100,000 nodes | 300,000 nodes |
| --- | ---: | ---: |
| Native Apple Silicon hosted runner | 254.158 s | 358.979 s |
| Native Intel hosted runner | 490.276 s | 913.336 s |

Every run completed 470 searches with zero failed games. These were FEN-only
baseline workloads; history-aware measurements are identified separately by
report schema 2. The initial development budget is 100,000 nodes, one thread,
16 MiB hash, fresh process per search, with a 30-second watchdog. Diagnosis
stops once it finds a supported candidate. This choice favors the lower measured
latency; it is not proof of coaching quality. Representative review-set quality,
interactive responsiveness and full release acceptance still need validation.

## Local diagnostic

```sh
cargo run --release -p gambit-coaching --example diagnose_review -- \
  ENGINE PGN 'Player name' SHARED_PLY
```

The example accepts one to six games and emits detailed JSONL locally. Do not
upload private PGNs, positions, evaluations or reports to CI or PRs. It does not
write a cache or modify a library. Unit tests inject deterministic engine
evidence; real engine runs are separate from those policy tests.
