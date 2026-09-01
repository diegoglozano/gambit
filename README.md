# Gambit

Gambit is the beginning of a high-performance chess analysis stack in Rust. It
currently contains a dependency-free PGN ingestion layer and a compact semantic
chess core.

## Parser design

- Single-pass pull parser over `&[u8]`
- Zero allocation on the hot path; every tag, SAN move, and comment borrows the
  original input
- Structural events for headers, move numbers, SAN, NAGs, comments, recursive
  annotation variations, and outcomes
- Fused single-pass parsing from files, pipes, or decompression streams with
  bounded memory
- Borrowed streaming events delivered through a callback, without rescanning
  framed games
- Byte-accurate spans and errors
- Strict and lenient modes
- No chess-position work in the parsing layer

```rust
use gambit_pgn::{Event, Parser};

let pgn = b"[Event \"Example\"]\n\n1. e4 e5 2. Nf3 *";
for event in Parser::new(pgn) {
    match event? {
        Event::San(token) => println!("SAN: {}", token.as_str()?),
        _ => {}
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Run the test suite:

```console
cargo test --workspace
```

Measure parser throughput on the built-in synthetic corpus, or pass a real PGN
file:

```console
cargo run --release -p gambit-pgn --example throughput
cargo run --release -p gambit-pgn --example throughput -- games.pgn
```

Validate a corpus and print structural counts:

```console
cargo run --release -p gambit-pgn --example validate -- games.pgn
```

Parse a decompressed file with bounded memory, or decompress and parse in one
pipeline. This is the preferred high-throughput streaming path:

```console
cargo run --release -p gambit-pgn --example incremental-validate -- games.pgn
zstdcat games.pgn.zst | \
  cargo run --release -p gambit-pgn --example incremental-validate -- -
```

The older two-pass game-framing benchmark remains available for comparison:

```console
cargo run --release -p gambit-pgn --example stream-validate -- games.pgn
zstdcat games.pgn.zst | \
  cargo run --release -p gambit-pgn --example stream-validate -- -
```

The first real-corpus baseline uses 810,463 standard-rated Lichess games. See
[the April 2014 benchmark report](docs/benchmarks/lichess-2014-04.md) for the
dataset checksum, reproducible commands, python-chess and Scoutfish comparison,
results, and measurement limitations.

The parser remains lexical by design. `gambit-chess` owns legality and
position-dependent meaning so ingestion and chess-state work stay independently
optimizable.

## Semantic chess core

`gambit-chess` adds 104-byte copyable bitboard positions, 32-bit moves, FEN
loading, legal move generation, and SAN execution. It handles pins, checks,
castling, en passant, promotion, and file/rank disambiguation without external
dependencies.

Run a reproducible legal-move-generation benchmark from the initial position;
the optional arguments are depth followed by a FEN:

```console
cargo run --release -p gambit-chess --example perft -- 6
cargo run --release -p gambit-chess --example perft -- 5 \
  'r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1'
```

Validate every SAN move while incrementally reading a PGN corpus:

```console
cargo run --release -p gambit-pgn --example semantic-validate -- games.pgn
zstdcat games.pgn.zst | \
  cargo run --release -p gambit-pgn --example semantic-validate -- -
```

On the April 2014 Lichess corpus, the current single-threaded semantic path
validates 54,748,499 moves at a median 11.43 million moves/s on an Apple M3, with
approximately 1.79 MB maximum RSS.

An experimental bounded parallel path frames complete games into packed byte
batches and validates each batch on worker-local chess state. The optional
arguments select the worker count and target batch size in MiB; defaults are
the available hardware parallelism and 1 MiB:

```console
cargo run --release -p gambit-pgn \
  --example parallel-semantic-validate -- games.pgn 4 1
```

Sweep worker counts on the same decompressed corpus to measure strong scaling:

```console
cargo build --release -p gambit-pgn --example parallel-semantic-validate
for workers in 1 2 4 8; do
  target/release/examples/parallel-semantic-validate games.pgn "$workers" 1
done
```

The queue holds at most twice the worker count in pending batches. Games larger
than the batch target remain intact, so the existing 16 MiB `GameReader` limit
is still the per-game upper bound. This path intentionally uses the lightweight
game framer before worker-local parsing. The file-oriented experiment below
tests direct game-aligned range partitioning without that serial validation
stage.

The experimental partitioned path performs a bounded boundary-discovery pass,
then lets each worker seek to and validate an independent game-aligned range:

```console
cargo run --release -p gambit-pgn \
  --example partitioned-semantic-validate -- games.pgn 4
```

It reports partitioning and validation time separately. On the Intel N95,
direct validation was only slightly faster than the queue path. On the Apple M3,
eight direct ranges validate faster than the four-worker queue, but the discovery
pass still makes a one-shot run slower. Persisted boundaries amortize after
approximately four repeated M3 analyses.
