# Local turning-point evidence

The backend can replay one standard game for an explicitly selected player and
find the earliest supported deterioration after a review's shared ply. It is
wired into a work-in-progress desktop diagnosis and practice panel. It is not
yet a complete coaching release.

## Desktop background service

The desktop now exposes `start_coaching`, `coaching_status`, and
`cancel_coaching`. Requests require the current library path, one to six distinct
game IDs, an explicit player and shared ply. A read-only request loads cached
results without launching Stockfish; analysis requires `analyze: true`.
All cached games are loaded before missing games are searched. Game-level
events carry a job generation and library path so the UI can reject stale
updates. Cancellation likewise requires the matching generation and path.

A shared engine gate serializes searches across cancellable worker sessions.
The queue runs on an owned background thread, preserves completed records,
isolates unsupported/failed games and resolves the packaged adjacent engine.
Library switching cancels old work without blocking navigation; normal game
navigation does not cancel it. App exit cancels and joins workers so active
engine children are reaped. No queue command writes indexed evidence or legacy
session metadata. A new request waits for a cancelling worker to finish.

The practice command accepts only move attempts and named actions, not client
verdicts or solved flags. It rechecks current game inputs and the actual engine
hash before saving. Practice shares the engine gate with diagnosis. Snapshots
include monotonic revisions and factual summaries.

The UI now has an explicit local-analysis explanation, progressive results,
cancel/retry, a FEN-based practice board with pointer/arrow-key selection and
text move input, promotion selection, and explicit reveal/done/later actions.
Preferred moves and lines are absent from the visible answer until success or
reveal. Revealed lines now support manual board stepping through up to six legal
plies, preserving the player's orientation; exercise input is disabled away from
the root position. Exit opens the saved-evidence session summary with a return
to practice action. Native QA has verified basic keyboard entry, cancellation,
reveal, accepted/rejected attempts and cached restoration in an isolated build.
Screen-reader and final release-artifact checks remain pending.

Remaining release work includes native sleep notifications, diagnosis-aware
recommendation labels, full queue-state reconciliation, final release-artifact
and screen-reader verification.

Before searching, the queue atomically saves intent markers keyed by the full
analysis inputs. Cache-only reopening visibly marks unfinished work even if no
game completed; a valid completed record supersedes its marker. Unreadable
records expose an explicit recovery action: preserve that one record under a
unique backup name, then retry local analysis. Valid records cannot be reset by
this action, and recovered corrupt practice cannot be silently treated as valid.

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
separately from the diagnosis within each record. Practice-policy version is
also part of the key so a changed tolerance cannot silently retain old outcomes.

Saves use a private same-directory temporary file, sync its contents, atomically
replace the one record, and sync the directory on Unix. Read/modify/write
operations are serialized within the desktop process. An identical diagnosis
retry preserves practice; a different result under the same key is rejected.
Cache reads do not create directories or alter library files.

## Practice and factual summaries

The backend verifies a submitted legal move against the stored preferred move,
searching both continuations with the same complete root history and node
budget. This avoids comparing an old root estimate to only one newly searched
child. A different move can be strong if it is within 30 centipawns; a submitted
preferred move is still searched, sharing its reference search. Illegal moves
do not launch an engine. Unsupported score inequalities are **inconclusive**,
not “try again.” Forced mates retain their type; preserving a forced mate is
accepted without translating mate distances into centipawns.

Attempt feedback contains only the submitted move and its evaluations/verdict,
not the preferred move or PV. Reveal is an explicit separate action. The cache
retains the latest 100 attempts, total attempt count, first solve outcome
(without reveal or after hint), reveal state, and active/completed/practice-later
disposition. Engine failure and cancellation return no completed attempt.

Summary calculations accept at most six distinct game records. They count
analyzed/no-result games and practice outcomes, report the actual FEN move-number
range, and group identical playable positions and identical played choices.
Move counters are excluded from repeated-position identity; castling, side to
move and effective en passant remain relevant. Centipawn mean/median exclude
mate transitions, and remain labeled as minimum estimates when any contributing
loss was bounded. No theme classifier or opening-quality claim is introduced.

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
are saved individually by the desktop queue/cache layer.

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
