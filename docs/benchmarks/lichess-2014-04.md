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

## External tool comparison

These are not interchangeable operations, so the work performed by every row
is part of the result:

| Tool and operation | Correct games | Median throughput | Maximum RSS | Semantics |
| --- | ---: | ---: | ---: | --- |
| Gambit whole-file | 810,463 | 539.88 MiB/s | 703.3 MB | Lexes and emits every structural event |
| Gambit streaming file | 810,463 | 241.28 MiB/s | 1.74 MB | Frames games, then lexes and counts every event |
| Gambit `zstdcat` pipeline | 810,463 | 238.10 MiB/s | 1.77 MB | Decompresses, frames, lexes, and counts every event |
| python-chess `skip_game()` | 810,463 | 84.60 MiB/s | 20.25 MB | Finds and skips games without fully parsing them |
| Scoutfish `make` | Not completed | Not valid | Not valid | Parses legal positions and writes a query index |

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

An exploratory `visitor` run was stopped after 401.90 seconds without completing
the corpus. This is not reported as a throughput result: unlike Gambit's current
lexical parser, it decodes SAN, validates moves against board state, and updates
positions. That work belongs in a future comparison against Gambit's SAN/board
layer.

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
- The streaming framer and parser both inspect the input. A fused incremental
  parser could remove the duplicate scan while retaining bounded memory.
- The April 2014 corpus contains no recursive annotation variations, so the
  unit tests and synthetic corpus remain responsible for that grammar path.
- These runs were made on a local fanless laptop without CPU pinning, a fixed
  performance governor, or thermal stabilization. Treat the result as a
  development baseline, not a portable hardware comparison.
- No compiler flags such as `-C target-cpu=native`, LTO, or PGO were enabled.

The next performance milestone should fuse game framing and event parsing into
one incremental state machine. After that, add batch-level parallelism and the
SAN/board layer, where semantic comparisons with python-chess visitor mode and
Scoutfish indexing become meaningful.
