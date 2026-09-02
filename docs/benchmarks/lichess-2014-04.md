# Lichess April 2014 baseline

This report establishes a reproducible correctness and single-core throughput
baseline before optimizing `gambit-pgn`. The dataset is not stored in this
repository.

## Environment

- Date: 2026-08-31
- Hardware: Apple M3 MacBook Air, 8 cores (4 performance, 4 efficiency), 16 GB
  memory
- Operating system: macOS 26.0.1
- Rust: `rustc 1.93.1 (01f6ddf75 2026-02-11)`, aarch64 Apple Darwin
- Build: default Cargo release profile
- Parser implementation: commit `8046651`

## Dataset

The input is the official Lichess standard-rated export for April 2014:

```text
https://database.lichess.org/standard/lichess_db_standard_rated_2014-04.pgn.zst
```

Lichess publishes the archive as 137 MB containing 810,463 games. Values
observed locally:

```text
compressed bytes:   136,839,236
decompressed bytes: 701,772,510
SHA-256: d795efabb88ead3636b1233ed7e53b9e706b917c60862979fafc8bb7c4864dfe
```

The digest exactly matched Lichess's published `standard/sha256sums.txt` entry.

## Reproduction

Choose a dataset directory outside the repository:

```console
LICHESS_DATA_DIR=/absolute/path/to/lichess-data
mkdir -p "$LICHESS_DATA_DIR"

curl -fL --retry 3 \
  https://database.lichess.org/standard/lichess_db_standard_rated_2014-04.pgn.zst \
  -o "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn.zst"

shasum -a 256 \
  "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn.zst"

pzstd -d \
  "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn.zst" \
  -o "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"
```

Run a strict validation pass:

```console
cargo run --release -p gambit-pgn --example validate -- \
  "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"
```

Run the minimal parser-throughput harness five times:

```console
cargo build --release -p gambit-pgn --example throughput
for run_number in 1 2 3 4 5; do
  target/release/examples/throughput \
    "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"
done
```

Run bounded-memory validation from the decompressed file and directly from the
Zstandard archive:

```console
cargo build --release -p gambit-pgn --example stream-validate

target/release/examples/stream-validate \
  "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"

zstdcat "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn.zst" | \
  target/release/examples/stream-validate -
```

Run the fused incremental parser over the same inputs:

```console
cargo build --release -p gambit-pgn --example incremental-validate

for run_number in 1 2 3 4 5; do
  target/release/examples/incremental-validate \
    "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"
done

for run_number in 1 2 3 4 5; do
  zstdcat "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn.zst" | \
    target/release/examples/incremental-validate -
done
```

## Correctness result

Strict parsing completed without an error and produced:

| Metric | Count |
| --- | ---: |
| Bytes | 701,772,510 |
| Events | 104,757,225 |
| Games | 810,463 |
| Tags | 12,137,048 |
| Move numbers | 29,593,299 |
| SAN tokens | 54,748,499 |
| NAGs | 1,006,520 |
| Comments | 4,030,007 |
| Variations | 0 |
| Outcomes | 810,463 |

The parsed game count exactly matches the count published by Lichess, and every
game produced one outcome.

## Throughput result

The minimal harness parses the already-loaded byte slice repeatedly, consumes
every event through `black_box`, and excludes file loading from its timer.

| Run | Throughput (MiB/s) |
| ---: | ---: |
| 1 | 533.19 |
| 2 | 540.03 |
| 3 | 540.84 |
| 4 | 539.88 |
| 5 | 538.48 |
| **Median** | **539.88** |

The instrumented validation harness, which classifies and increments counters
for every event, measured 528.36, 523.80, 523.33, 522.19, and 520.92 MiB/s. Its
median was 523.33 MiB/s.

A separate `/usr/bin/time -l` validation run reported a maximum resident set
size of 703,283,200 bytes and a peak memory footprint of 703,087,408 bytes.

## Bounded-memory streaming result

`GameReader` reads 64 KiB chunks by default, recognizes outcome markers only at
the top movetext level, and reuses its storage between games. It then passes
each borrowed game slice to the same strict `Parser` used by the whole-file
measurements.

All ten streaming runs reproduced every correctness count in the table above.

| Input | Runs (MiB/s) | Median (MiB/s) |
| --- | --- | ---: |
| Decompressed file | 242.72, 242.53, 240.57, 241.28, 240.19 | **241.28** |
| `zstdcat` pipe | 234.10, 238.11, 238.10, 236.63, 238.44 | **238.10** |

The internal timer includes reading, framing, parsing, and event counters. For
the pipe it also includes time waiting for the separate `zstdcat` process. A
timed file run reported 1,736,704 bytes maximum RSS; a timed pipe run reported
1,769,472 bytes maximum RSS for the Gambit process. The latter does not include
the decompressor's memory.

Compared with the whole-file validator, streaming reduces measured maximum RSS
from approximately 703 MB to 1.7 MB. Its lower throughput is expected because
the current implementation scans the bytes once to find game boundaries and a
second time to emit PGN events.

## Fused incremental streaming result

The follow-up `IncrementalParser` combines boundary recognition and event
emission in one lexical state machine. It reads 64 KiB chunks, retains only an
incomplete token across reads, and lends borrowed events to a callback before
the reusable buffer is compacted. No input byte is rescanned by a separate game
framer.

All ten runs reproduced every correctness count above. The largest internal
buffer observed was 65,624 bytes.

| Input | Runs (MiB/s) | Median (MiB/s) |
| --- | --- | ---: |
| Decompressed file | 416.93, 432.53, 433.64, 432.84, 431.86 | **432.53** |
| `zstdcat` pipe | 424.43, 427.02, 427.28, 427.05, 426.46 | **427.02** |

This is a 79.3% improvement over both two-pass medians. It retains approximately
82.6% of the whole-file counter harness's throughput while avoiding its
full-corpus allocation.

A timed file run reported 1,736,704 bytes maximum RSS. A timed pipeline run
reported 1,753,088 bytes maximum RSS for Gambit, excluding the separate
decompressor. The parser therefore keeps essentially the same bounded memory
footprint as `GameReader` while eliminating its duplicate scan.

## Gambit Stats result

`gambit stats` exposes the fused incremental path as a product workflow. Its
callback keeps only corpus counters: complete games, mainline plies, outcomes,
and game-length extrema and sum. The `.pgn.zst` path also performs Zstandard
decompression in the Gambit process.

Build Stats and run it five times against both representations:

```console
cargo build --release -p gambit

for run_number in 1 2 3 4 5; do
  target/release/gambit stats --format json \
    "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"
done

for run_number in 1 2 3 4 5; do
  target/release/gambit stats --format json \
    "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn.zst"
done
```

Both paths reproduced the baseline's 701,772,510 decompressed bytes, 810,463
games, and 54,748,499 mainline SAN tokens. Stats additionally reported 410,370
white wins, 372,707 black wins, 27,386 draws, and no unfinished games. Game
lengths ranged from 0 to 344 plies with an average of 67.5521.

| Input | Runs (MiB/s) | Median (MiB/s) | Maximum RSS |
| --- | --- | ---: | ---: |
| Decompressed file | 402.02, 443.30, 482.23, 489.95, 475.62 | **475.62** | 1,884,160 bytes |
| In-process `.pgn.zst` | 359.79, 351.54, 362.47, 339.50, 340.05 | **351.54** | 10,911,744 bytes |

The Stats timer includes file reads, event parsing, aggregation, and—in the
compressed row—in-process decompression. The first decompressed-file run also
includes a cold page-cache effect; all five values remain visible rather than
discarding it. Maximum RSS comes from a separate `/usr/bin/time -l` run.

The decompressed path retains a sub-2 MB resident footprint independent of the
669 MiB corpus. Direct compressed input trades roughly 9 MB of additional
resident memory and lower byte throughput for eliminating a separate
decompression process and the 701 MB decompressed file.

## External tool comparison

These are not interchangeable operations, so the work performed by every row
is part of the result:

| Tool and operation | Correct games | Median throughput | Maximum RSS | Semantics |
| --- | ---: | ---: | ---: | --- |
| Gambit whole-file | 810,463 | 539.88 MiB/s | 703.3 MB | Lexes and emits every structural event |
| Gambit fused streaming file | 810,463 | 432.53 MiB/s | 1.74 MB | Reads once, lexes, and counts every event |
| Gambit fused `zstdcat` pipeline | 810,463 | 427.02 MiB/s | 1.75 MB | Decompresses, reads once, lexes, and counts every event |
| Gambit streaming file | 810,463 | 241.28 MiB/s | 1.74 MB | Frames games, then lexes and counts every event |
| Gambit `zstdcat` pipeline | 810,463 | 238.10 MiB/s | 1.77 MB | Decompresses, frames, lexes, and counts every event |
| python-chess `skip_game()` | 810,463 | 84.60 MiB/s | 20.25 MB | Finds and skips games without fully parsing them |
| Scoutfish `make` | Not completed | Not valid | Not valid | Parses legal positions and writes a query index |

## Semantic chess result

`gambit-chess` resolves each SAN token against a 104-byte bitboard position,
tests legality, and applies it. The hot path targets only source pieces that can
geometrically reach the SAN destination; it does not generate every legal move
unless it must verify castling or checkmate.

Run the combined incremental PGN and semantic validator:

```console
cargo build --release -p gambit-pgn --example semantic-validate

for run_number in 1 2 3 4 5; do
  target/release/examples/semantic-validate \
    "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"
done
```

Every run completed all 810,463 games and applied all 54,748,499 SAN moves with
zero errors.

| Run | Elapsed | Throughput | Move rate |
| ---: | ---: | ---: | ---: |
| 1 | 8.995s | 74.40 MiB/s | 6.09 million/s |
| 2 | 8.843s | 75.69 MiB/s | 6.19 million/s |
| 3 | 8.939s | 74.87 MiB/s | 6.12 million/s |
| 4 | 8.805s | 76.01 MiB/s | 6.22 million/s |
| 5 | 8.796s | 76.09 MiB/s | 6.22 million/s |
| **Median** | **8.843s** | **75.69 MiB/s** | **6.19 million/s** |

A separate timed run reported 1,769,472 bytes maximum RSS. The semantic work
reduces byte throughput relative to lexical-only parsing, but it keeps the same
bounded-memory profile.

The comparable python-chess visitor validates SAN and updates positions without
retaining game trees. A fresh run was interrupted after 324.90 seconds without
finishing; the earlier attempt was stopped after 401.90 seconds. Neither partial
run is reported as completed throughput. Gambit completes the full semantic
workload more than 36 times within the shorter interrupted duration, but an
exact completed speedup remains unavailable.

### python-chess 1.11.2

The committed `benchmarks/python_chess_scan.py` script supports three modes.
`skip` uses the API documented for quickly skimming games, `visitor` validates
SAN and counts moves without retaining game trees, and `model` calls the normal
game-model parser.

The measured structural comparison used Python 3.9.6 and python-chess 1.11.2:

```console
python3 -m venv /tmp/gambit-python-chess
/tmp/gambit-python-chess/bin/python -m pip install \
  -r benchmarks/python-chess-requirements.txt

for run_number in 1 2 3 4 5; do
  /tmp/gambit-python-chess/bin/python benchmarks/python_chess_scan.py skip \
    "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"
done
```

The five results were 85.69, 85.06, 84.26, 84.60, and 82.96 MiB/s. Each found
exactly 810,463 games. A timed run reported 20,250,624 bytes maximum RSS.

The original exploratory `visitor` run was stopped after 401.90 seconds without
completing the corpus. The semantic section above now provides the comparable
Gambit SAN/board result and records a second interrupted python-chess attempt.

### Scoutfish commit `00cec1339f97114a32c30080dbad5e3a500634f2`

Scoutfish has no native Apple ARM64 build target. The unmodified source was
compiled as a generic 64-bit host binary with Apple Clang and `-O3`:

```console
git clone https://github.com/mcostalba/scoutfish.git /tmp/scoutfish
git -C /tmp/scoutfish checkout 00cec1339f97114a32c30080dbad5e3a500634f2
make -C /tmp/scoutfish/src build \
  ARCH=general-64 COMP=clang KERNEL=Other

/tmp/scoutfish/src/scoutfish make \
  "$LICHESS_DATA_DIR/lichess_db_standard_rated_2014-04.pgn"
```

The indexing pass repeatedly rejected legal disambiguated SAN such as `Rfe1`,
`Rac8`, and `Rfd1`, then terminated with `SIGBUS` after writing an incomplete
60 KiB index. Because it neither completed nor accepted the corpus, no Scoutfish
throughput figure is valid on this platform/build. This result should be retried
on a supported x86-64 environment before drawing a broader conclusion about
Scoutfish.

## Interpretation and limitations

- This validates the lexical parser against a large real-world corpus. It does
  not validate whether each SAN token is legal in its position.
- The original `validate` and `throughput` examples use `fs::read`, so their peak
  memory is approximately the full decompressed input size. `stream-validate`
  removes that limitation.
- `GameReader` currently requires a top-level outcome marker and has a default
  16 MiB maximum game size. It intentionally rejects incomplete final records.
- The legacy `GameReader` path still scans input twice and remains useful when
  callers specifically need complete borrowed game slices. `IncrementalParser`
  is the faster path for event consumers.
- The April 2014 corpus contains no recursive annotation variations, so the
  unit tests and synthetic corpus remain responsible for that grammar path.
- These runs were made on a local fanless laptop without CPU pinning, a fixed
  performance governor, or thermal stabilization. Treat the result as a
  development baseline, not a portable hardware comparison.
- No compiler flags such as `-C target-cpu=native`, LTO, or PGO were enabled.

The parallel follow-up below adds batch-level concurrency around the compact
SAN/board layer. Games are independent work units, so a bounded producer/worker
pipeline keeps memory bounded while legality checking and position updates
scale across cores. A completed Scoutfish comparison still requires a supported
x86-64 environment.

## Bounded parallel semantic result

The first parallel prototype uses `GameReader` as a serial producer, copies
complete games into packed byte batches, and sends those batches through a
bounded queue to worker-local parsers and chess positions. Results are reduced
in input order so error reporting remains deterministic.

This follow-up measurement used a separate host and is not directly comparable
to the Apple M3 results above:

- Date: 2026-09-01
- Hardware: Intel N95, 4 physical cores, no SMT, single NUMA node
- Rust: `rustc 1.85.0 (4d91de4e 2025-02-17)`
- Target: `x86_64-unknown-linux-musl`
- Build: default Cargo release profile, no native CPU flags
- Input: the same checksum-verified decompressed April 2014 corpus
- Batch target: 4 MiB

Every run reproduced 810,463 games and 54,748,499 legal SAN moves. Five runs
were made for each worker count:

| Workers | Runs (MiB/s) | Median | Median move rate | Speedup vs. fused baseline |
| ---: | --- | ---: | ---: | ---: |
| Fused single-thread baseline | 34.66, 35.07, 34.56, 35.06, 34.77 | **34.77** | 2.84 million/s | 1.00x |
| 1 | 35.26, 34.84, 35.70, 35.34, 35.34 | **35.34** | 2.89 million/s | 1.02x |
| 2 | 63.53, 63.88, 63.38, 64.02, 64.33 | **63.88** | 5.23 million/s | 1.84x |
| 4 | 91.82, 92.65, 90.33, 89.31, 91.99 | **91.82** | 7.51 million/s | 2.64x |
| 8 | 92.48, 85.09, 91.77, 90.95, 91.70 | **91.70** | 7.50 million/s | 2.64x |

Four workers use all physical cores and are the saturation point on this host.
Eight workers add scheduling variability without increasing the median. A
separately timed four-worker, 4 MiB run consumed approximately 360% CPU and
54,816 KiB maximum RSS.

A smaller batch-size sweep found no meaningful throughput benefit from larger
batches:

| Target batch | Batches | Median throughput | Sample maximum RSS |
| ---: | ---: | ---: | ---: |
| 1 MiB | 670 | **91.59 MiB/s** | 14,424 KiB |
| 4 MiB | 168 | **91.82 MiB/s** | 54,816 KiB |
| 16 MiB | 42 | **90.50 MiB/s** | 215,796 KiB |

The 1 and 16 MiB medians use three runs; the 4 MiB median uses the five-run
scaling series. Maximum RSS is one sample per size. Because 1 MiB retains
99.7% of the 4 MiB median throughput while using substantially less memory, it
is the new default.

The achieved 2.64x strong-scaling speedup is useful but only 66% efficiency at
four cores. The following experiment tests whether direct file partitioning can
remove the serial framer, packed-batch copy, and producer/worker
oversubscription. The bounded producer remains appropriate for pipes and
non-seekable decompression streams.

## Partitioned file result

The partitioned prototype first makes a bounded serial `GameReader` pass to
choose approximately byte-balanced game boundaries. Each worker then seeks to
its assigned boundary and independently frames, parses, and semantically
validates that range. It uses no central producer or packed-game copies during
the validation phase. Partitioning and validation are timed separately so a
reusable external index can be evaluated independently from a one-shot run.

The same Intel N95 host, Rust target, build configuration, and checksum-verified
corpus were used. Every run again produced exactly 810,463 games and 54,748,499
legal SAN moves.

| Workers | Validation runs (MiB/s) | Validation median | End-to-end median | Median partition time |
| ---: | --- | ---: | ---: | ---: |
| 1 | 31.57, 31.00, 31.39, 31.56, 31.44 | **31.44 MiB/s** | **28.08 MiB/s** | 2.598s |
| 2 | 58.07, 58.37, 57.85, 59.19, 57.80 | **58.07 MiB/s** | **47.58 MiB/s** | 2.586s |
| 4 | 86.31, 89.05, 94.50, 95.84, 96.03 | **94.50 MiB/s** | **69.82 MiB/s** | 2.579s |
| 8 | 91.76, 93.83, 92.10, 96.15, 97.44 | **93.83 MiB/s** | **68.79 MiB/s** | 2.596s |

Four ranges are again the saturation point. Their validation-only median is
2.9% above the bounded queue's 91.82 MiB/s median, but the boundary pass makes
one-shot end-to-end throughput 24.0% lower. A separately timed four-range run
used 1,144 KiB maximum RSS, compared with 14,424 KiB for a 1 MiB queue batch.

Using the observed median times, a reusable partition index would need roughly
13 validation passes over the same file to amortize its initial discovery cost
against the queue pipeline. Persisting such an index is therefore useful only
for repeated analytics, not one-shot validation. The result also shows that the
producer and batch copies are not the main scaling limit on this host. The next
optimization should profile the SAN/board kernel, especially repeated
occupancy, piece lookup, attack generation, and `Position` copies.

## Precomputed non-sliding attacks

Hardware counters were unavailable on the Intel N95 host because Linux exposed
`perf_event_paranoid=4`. Source-guided candidate changes were therefore accepted
only after five-run corpus measurements. Hoisting occupancy and replacing
target-square piece lookup with bit tests regressed the single-thread median
from 34.10 to 33.81 MiB/s and was reverted.

The successful change generates pawn, knight, and king attack tables at compile
time. Runtime check detection now performs indexed lookups instead of repeatedly
walking coordinate deltas. The four 64-entry tables occupy 2 KiB of static data;
the 104-byte `Position` layout and runtime memory bounds are unchanged.

Fresh single-thread fused-parser measurements before and after the final table
change were:

| Version | Runs (MiB/s) | Median | Move rate |
| --- | --- | ---: | ---: |
| Before | 34.29, 34.64, 33.87, 34.10, 33.91 | **34.10 MiB/s** | 2.79 million/s |
| Attack tables | 37.59, 37.96, 38.28, 38.36, 38.33 | **38.28 MiB/s** | 3.13 million/s |

This is a 12.3% single-thread throughput improvement. The final bounded queue
scaling series with 1 MiB batches was:

| Workers | Runs (MiB/s) | Median | Median move rate |
| ---: | --- | ---: | ---: |
| 1 | 39.72, 39.33, 38.86, 39.68, 39.58 | **39.58 MiB/s** | 3.24 million/s |
| 2 | 72.57, 71.24, 71.55, 71.70, 67.78 | **71.55 MiB/s** | 5.85 million/s |
| 4 | 105.92, 105.06, 101.41, 104.69, 96.25 | **104.69 MiB/s** | 8.56 million/s |
| 8 | 102.24, 104.87, 100.72, 105.26, 100.67 | **102.24 MiB/s** | 8.36 million/s |

The four-worker median is 14.3% above the earlier 1 MiB batch result of 91.59
MiB/s. A sampled four-worker run used 14,304 KiB maximum RSS, effectively
unchanged from the previous 14,424 KiB sample. Four physical workers remain the
saturation point. The next kernel candidate is sliding-ray attack lookup, which
should likewise be evaluated independently before changing the position layout.

## Precomputed sliding rays

The sliding-ray candidate replaces the eight coordinate walks in check
detection with per-square ray masks. The nearest occupied square on an
increasing ray is isolated with its least significant bit; decreasing rays use
the most significant bit. The eight compile-time 64-entry tables add 4 KiB of
static data. The 104-byte `Position` layout and the bounded queue are unchanged.

Fresh five-run measurements compared the retained merged attack-table binary
with the sliding-ray candidate on the same decompressed corpus and host:

| Version | Runs (MiB/s) | Median | Move rate |
| --- | --- | ---: | ---: |
| Attack-table baseline | 36.29, 38.33, 38.62, 38.76, 38.60 | **38.60 MiB/s** | 3.16 million/s |
| Sliding rays | 51.00, 52.78, 53.15, 52.99, 52.57 | **52.78 MiB/s** | 4.32 million/s |

This is a 36.7% single-thread throughput improvement. Both binaries validated
all 810,463 games and 54,748,499 legal SAN moves on every run. A fresh
four-worker comparison measured 104.30, 105.96, 103.95, 104.00, and 103.25
MiB/s for the merged baseline, a 104.00 MiB/s median. The final candidate
scaling series was:

| Workers | Runs (MiB/s) | Median | Median move rate |
| ---: | --- | ---: | ---: |
| 1 | 55.53, 56.13, 55.45, 55.59, 53.23 | **55.53 MiB/s** | 4.54 million/s |
| 2 | 94.49, 98.26, 97.27, 93.92, 94.43 | **94.49 MiB/s** | 7.73 million/s |
| 4 | 133.02, 131.13, 119.38, 122.43, 135.30 | **131.13 MiB/s** | 10.73 million/s |
| 8 | 126.31, 126.14, 124.25, 129.09, 121.76 | **126.14 MiB/s** | 10.32 million/s |

The four-worker median improves by 26.1% over the fresh merged baseline. A
sampled four-worker run used 14,368 KiB maximum RSS, only 64 KiB above the prior
sample. Four physical workers remain the saturation point. The next isolated
kernel candidate was applying the ray masks to pseudo-legal sliding move
generation; that experiment is reported after the Apple M3 baseline below.

## Final merged version on Apple M3

The final version through the sliding-ray optimization was rerun on the Apple
M3 host so the Intel N95 measurements above have a current macOS comparison:

- Date: 2026-09-01
- Commit: `6068928`
- Hardware: Apple M3 MacBook Air, 4 performance and 4 efficiency cores, 16 GB
  memory
- Operating system: macOS 26.0.1, arm64
- Rust: `rustc 1.93.1 (01f6ddf75 2026-02-11)`
- Build: default Cargo release profile, no native CPU flags
- Input: the same checksum-verified 701,772,510-byte April 2014 corpus
- Bounded queue batch target: 1 MiB

Commands launched by the benchmark automation inherited macOS background
scheduling, producing unusable 28–114 MiB/s single-thread variance. Each
published process was moved out of `PRIO_DARWIN_BG` immediately after launch
with `taskpolicy -B -p PID`. Five subsequent fused runs stayed within 1.1% of
each other. A process launched normally from an interactive terminal does not
need this adjustment.

Every published run reproduced exactly 810,463 games and 54,748,499 legal SAN
moves.

### Fused single-thread result

| Runs (MiB/s) | Median | Median elapsed | Median move rate |
| --- | ---: | ---: | ---: |
| 116.99, 116.22, 116.78, 117.04, 117.46 | **116.99 MiB/s** | **5.721s** | **9.57 million/s** |

A separately timed run used 1,785,856 bytes maximum RSS.

### Bounded queue scaling

| Workers | Runs (MiB/s) | Median | Median move rate | Speedup vs. fused |
| ---: | --- | ---: | ---: | ---: |
| 1 | 123.19, 123.75, 123.90, 123.55, 123.24 | **123.55 MiB/s** | 10.11 million/s | 1.06x |
| 2 | 242.40, 241.44, 242.22, 242.18, 240.89 | **242.18 MiB/s** | 19.81 million/s | 2.07x |
| 4 | 393.89, 393.81, 391.62, 371.18, 386.08 | **391.62 MiB/s** | 32.04 million/s | 3.35x |
| 8 | 378.57, 378.49, 379.68, 365.85, 364.47 | **378.49 MiB/s** | 30.96 million/s | 3.24x |

Four workers are the queue saturation point and reach 79.2% scaling efficiency
relative to the one-worker queue. A sampled four-worker run used 10,715,136
bytes maximum RSS.

The final merged N95 and M3 series are directly comparable because both use the
same code, corpus, 1 MiB batches, and default release profile:

| Path | Intel N95 median | Apple M3 median | M3/N95 |
| --- | ---: | ---: | ---: |
| Fused single-thread | 52.78 MiB/s | 116.99 MiB/s | 2.22x |
| Bounded, 1 worker | 55.53 MiB/s | 123.55 MiB/s | 2.22x |
| Bounded, 2 workers | 94.49 MiB/s | 242.18 MiB/s | 2.56x |
| Bounded, 4 workers | 131.13 MiB/s | 391.62 MiB/s | 2.99x |
| Bounded, 8 workers | 126.14 MiB/s | 378.49 MiB/s | 3.00x |

### Partitioned file scaling

| Ranges | Validation runs (MiB/s) | Validation median | End-to-end median | Median partition time | Median move rate |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1 | 95.27, 95.94, 95.99, 95.77, 95.59 | **95.77 MiB/s** | **78.88 MiB/s** | 1.496s | 7.83 million/s |
| 2 | 185.95, 186.27, 185.81, 186.54, 185.23 | **185.95 MiB/s** | **131.30 MiB/s** | 1.498s | 15.21 million/s |
| 4 | 354.71, 343.20, 347.47, 347.04, 340.91 | **347.04 MiB/s** | **195.40 MiB/s** | 1.497s | 28.39 million/s |
| 8 | 513.03, 515.88, 508.26, 500.00, 512.73 | **512.73 MiB/s** | **238.77 MiB/s** | 1.496s | 41.94 million/s |

Unlike the bounded queue, validation-only throughput uses all eight M3 cores
and is 31% above the best queue median. The serial boundary pass still makes
one-shot partitioning 39% slower than the queue. With persisted boundaries,
the approximately 0.40s validation advantage amortizes the 1.50s discovery
cost on the fourth repeated analysis. A sampled eight-range run used 3,244,032
bytes maximum RSS.

The older N95 partition table predates the two attack-kernel optimizations, so
it should not be used for a direct M3/N95 hardware ratio. A new N95 partition
run at `6068928` would be required for that comparison.

## Ray-based sliding move generation on Apple M3

The next candidate reuses the existing eight per-square ray tables for bishop,
rook, and queen move generation. It isolates the nearest blocker on each ray
with a least- or most-significant-bit operation, includes that blocker as a
possible capture, and masks friendly pieces from the result. SAN candidate
selection now uses the same kernel instead of a separate coordinate walk. This
adds no tables and leaves the 104-byte `Position` layout unchanged.

The new `perft` example makes the move-generation benchmark reproducible:

```console
cargo build --release -p gambit-chess --example perft
target/release/examples/perft 6
target/release/examples/perft 5 \
  'r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1'
```

Five foreground runs of binaries built from the main-branch baseline
`b581215` and the candidate produced:

| Position | Depth | Nodes | Baseline (Mnodes/s) | Ray candidate (Mnodes/s) | Median change |
| --- | ---: | ---: | --- | --- | ---: |
| Initial | 6 | 119,060,324 | 51.65, 52.41, 52.65, 52.26, 52.64 | 54.83, 54.55, 54.69, 54.46, 54.81 | **+4.35%** |
| Kiwipete | 5 | 193,690,690 | 58.37, 56.14, 57.92, 58.52, 58.09 | 57.50, 58.35, 59.93, 60.22, 59.44 | **+2.32%** |

Correctness checks compare the ray kernel with the old coordinate walk for all
64 source squares, all three sliding pieces, fixed dense occupancy patterns,
and every possible single-square blocker. The existing initial, Kiwipete, and
endgame perft assertions also pass.

The semantic benchmark verifies whether the isolated kernel win survives the
full PGN-to-position path. Every candidate run again produced exactly 810,463
games and 54,748,499 legal SAN moves:

| Path | Main baseline median | Candidate runs (MiB/s) | Candidate median | Change |
| --- | ---: | --- | ---: | ---: |
| Fused single-thread | 116.99 MiB/s | 116.10, 119.98, 120.30, 117.64, 120.45 | **119.98 MiB/s** | **+2.56%** |
| Bounded, 4 workers | 391.62 MiB/s | 395.86, 390.31, 386.73, 386.42, 374.18 | **386.73 MiB/s** | -1.25% |
| Partitioned, 8 ranges | 512.73 MiB/s | 551.02, 541.53, 537.78, 512.68, 553.80 | **541.53 MiB/s** | +5.62% |

The fused result is the cleanest end-to-end signal and raises median move rate
from 9.57 to 9.81 million moves/s. The bounded result is effectively flat amid
parallel-run variance. Eight-range validation improved, but it is sensitive to
workstation scheduling; its median end-to-end throughput moved only from
238.77 to 243.27 MiB/s because the serial partition pass is unchanged.

As with the preceding Mac measurements, the automation initially launched
processes with macOS background priority. Each measured process was moved out
of `PRIO_DARWIN_BG` immediately after launch with `taskpolicy -B -p PID`.

## Move-application metadata on Apple M3

A four-second sample of the fused validator at the merged ray baseline
`fca38e5` showed that semantic validation remained dominated by SAN resolution
and move application. Three redundant operations were isolated:

- a selected non-castling SAN move was applied once to verify king safety and
  then applied again to commit it;
- `play_unchecked` searched all piece bitboards to rediscover the moving piece,
  even though both the SAN resolver and move generator already knew its type;
- `play_unchecked` searched for a captured piece on quiet moves despite the
  existing capture flag proving that no capture could occur.

SAN resolution now returns the already-validated successor position. The
moving piece occupies three previously unused high bits in the existing 32-bit
`Move`; zero remains an unspecified-piece sentinel so default moves retain the
old lookup fallback. Quiet moves bypass captured-piece lookup entirely. `Move`
therefore remains 4 bytes and `Position` remains 104 bytes.

The previous section's ray candidate is the perft baseline. Five foreground
runs of the final candidate produced:

| Position | Depth | Nodes | Baseline median | Candidate runs (Mnodes/s) | Candidate median | Change |
| --- | ---: | ---: | ---: | --- | ---: | ---: |
| Initial | 6 | 119,060,324 | 54.69 Mnodes/s | 72.60, 72.98, 73.34, 73.81, 73.78 | **73.34 Mnodes/s** | **+34.10%** |
| Kiwipete | 5 | 193,690,690 | 59.44 Mnodes/s | 70.72, 70.75, 70.74, 70.87, 70.44 | **70.74 Mnodes/s** | **+19.01%** |

A fresh paired fused baseline produced 120.00, 116.22, 120.18, 120.55, and
120.36 MiB/s, a 120.18 MiB/s median. The final candidate produced 130.63,
132.35, 132.82, 132.37, and 130.55 MiB/s, a **132.35 MiB/s median** and a
**10.13% improvement**. Median move rate increased from 9.83 to 10.83 million
moves/s. Every run reproduced exactly 810,463 games and 54,748,499 legal SAN
moves.

The legal-move tests retain the canonical initial, Kiwipete, and endgame perft
counts, and now also verify that generated moves carry the piece found on their
source square. Debug and release workspace tests cover SAN ambiguity, castling,
en passant, promotion, check, and mate.

Four-worker queue and eight-range runs also reproduced the exact corpus counts,
but are omitted as throughput comparisons: concurrent WindowServer and UI load
produced 231–320 MiB/s queue results and similarly unstable range results. The
single-process perft and fused series remained stable enough to publish.

## Token dispatch and SIMD experiment on Apple M3

A source-line profile of the merged move-metadata version `496d388` attributed
substantial parser time to SAN token boundaries, outcome probing, tag parsing,
and whitespace. Brace and line comment scans accounted for less than 1% of
samples despite 4,030,007 comment events.

An explicit SIMD-dispatched experiment replaced brace and line-comment loops
with the safe `memchr` and `memchr2` APIs. It reduced the incremental parser
median from 430.30 to 414.97 MiB/s, a 3.56% regression. Lichess comments are
generally too short to amortize the wider search setup, so the experiment and
dependency were removed.

The retained optimization instead avoids testing all four outcome strings for
ordinary tokens: only tokens beginning with `0`, `1`, or `*` enter outcome
matching. An intermediate five-run incremental median reached 450.05 MiB/s.
SAN scanning now classifies each byte through a shared 256-byte boundary table
instead of repeating whitespace and punctuation branches. The table is checked
exhaustively against the grammar for all byte values. This requires no unsafe
code or dependencies and adds only 256 bytes of static data.

Fresh baseline binaries at `496d388` and final candidate binaries were measured
on the same host and corpus:

| Parser path | Baseline runs (MiB/s) | Baseline median | Candidate runs (MiB/s) | Candidate median | Change |
| --- | --- | ---: | --- | ---: | ---: |
| Incremental file parser | 424.41, 433.53, 434.16, 434.37, 434.87 | **434.16 MiB/s** | 437.75, 472.92, 487.94, 487.64, 485.16 | **485.16 MiB/s** | **+11.75%** |
| In-memory slice parser | 532.93, 539.57, 536.14, 541.42, 529.17 | **536.14 MiB/s** | 636.68, 642.49, 645.62, 639.51, 639.92 | **639.92 MiB/s** | **+19.36%** |

Every incremental run produced the same 104,757,225 events, including
54,748,499 SAN tokens. The final full semantic series was 139.12, 139.97,
139.87, 139.71, and 139.64 MiB/s, a **139.71 MiB/s median** and a **5.56%
improvement** over the preceding 132.35 MiB/s median. It reproduced exactly
810,463 games and 54,748,499 legal SAN moves on every run; median move rate rose
from 10.83 to 11.43 million moves/s.

Direct SIMD remains interesting for a future batched lexer that classifies long
input blocks before emitting events. It is not a good fit for the current inner
SAN loop, where tokens are commonly only two to seven bytes and branch/table
setup dominates useful vector work.

## Destination-directed SAN sources on Intel N95

After the ray move-generation, move-metadata, and token-dispatch changes were
merged, the SAN resolver still began each non-castling move with every piece of
the requested type. A quiet pawn move could therefore test all eight pawns even
though its destination has at most two possible source squares. Each rejected
source also repeated destination occupancy and geometry work.

The candidate intersects the piece bitboard with attacks projected backward
from the SAN destination. Pawn captures use the existing color-specific attack
tables, quiet pawns use one- and two-rank shifts, knights and kings use their
existing lookup tables, and sliding pieces reuse the ray kernel. Destination
occupancy is resolved once per SAN token. No new tables, dependencies, or
position state are required.

Fresh five-run measurements compared binaries from merged `main` at `8b145ef`
with the candidate on the same Intel N95 host and decompressed corpus:

| Path | Baseline runs (MiB/s) | Baseline median | Candidate runs (MiB/s) | Candidate median | Change |
| --- | --- | ---: | --- | ---: | ---: |
| Fused single-thread | 56.96, 61.13, 56.35, 59.37, 60.72 | **59.37 MiB/s** | 64.68, 65.57, 64.80, 64.77, 63.08 | **64.77 MiB/s** | **+9.10%** |
| Bounded, 4 workers | 149.75, 149.34, 155.71, 157.96, 154.53 | **154.53 MiB/s** | 164.53, 152.58, 161.95, 159.30, 164.39 | **161.95 MiB/s** | **+4.80%** |

Every run validated exactly 810,463 games and 54,748,499 legal SAN moves. The
single-thread candidate median corresponds to 5.30 million moves/s; the
four-worker median reaches 13.25 million moves/s. A sampled candidate
four-worker run used 14,228 KiB maximum RSS. The smaller bounded-path gain is
consistent with the serial framer and queue becoming a larger share of total
time as worker-local semantic validation gets faster.

## Captured-piece metadata experiment on Apple M3

The next isolated candidate stored the captured piece type in three unused high
bits of `Move`. SAN resolution already discovers that type, so annotated
captures could bypass the remaining captured-piece bitboard scan in
`play_unchecked` without widening the 4-byte move representation. Zero retained
the metadata-free fallback for generated and default moves.

A broad variant also discovered and stored capture types during generic move
generation. It improved capture-heavy Kiwipete perft by 4.14%, but regressed
initial-position perft by 3.41%; its fused semantic median improved by only
0.57%. Moving the bitboard search into generation was therefore workload
dependent rather than a general reduction.

The final narrowed candidate annotated only SAN captures and en passant, where
the type was already available without another search. Five paired foreground
runs against merged `main` at `6602e08` produced:

| Path | Baseline runs | Baseline median | Candidate runs | Candidate median | Change |
| --- | --- | ---: | --- | ---: | ---: |
| Initial perft, depth 6 (Mnodes/s) | 73.62, 73.28, 73.21, 73.55, 73.63 | **73.55** | 70.07, 71.84, 71.73, 71.80, 71.87 | **71.80** | **-2.38%** |
| Kiwipete perft, depth 5 (Mnodes/s) | 70.78, 70.74, 70.85, 66.89, 70.01 | **70.74** | 70.63, 70.69, 70.52, 70.62, 70.36 | **70.62** | **-0.17%** |
| Fused semantic (MiB/s) | 142.28, 142.76, 145.09, 146.90, 146.52 | **145.09** | 143.64, 142.30, 145.35, 144.49, 144.15 | **144.15** | **-0.65%** |

Every semantic run reproduced exactly 810,463 games and 54,748,499 legal SAN
moves, while perft reproduced the canonical node totals. The extra decode and
fallback branch outweighed the saved scan at this corpus's capture frequency,
so the experiment was removed. Captured-piece metadata should not be revisited
without a separate SAN-specialized application path or a deployment profile
showing capture lookup as a material hotspot.

Before changing I/O backends, profile on the deployment platform. `io_uring` is
Linux-only and is unlikely to improve this cached, sequential workload unless
storage latency or syscall overhead appears in a Linux profile. Parallel
decompression, persisted partitions, native CPU tuning, and PGO remain
candidate experiments. Further SIMD work should begin with a separate batched
lexer design and retain a scalar path for short tokens.
