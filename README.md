# Gambit

Gambit is the beginning of a high-performance chess analysis stack in Rust. The
first component is `gambit-pgn`, a dependency-free PGN parser designed for bulk
game ingestion.

## Parser design

- Single-pass pull parser over `&[u8]`
- Zero allocation on the hot path; every tag, SAN move, and comment borrows the
  original input
- Structural events for headers, move numbers, SAN, NAGs, comments, recursive
  annotation variations, and outcomes
- Bounded-memory game framing from files, pipes, or decompression streams
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

Stream a decompressed file with bounded memory, or decompress and parse in one
pipeline:

```console
cargo run --release -p gambit-pgn --example stream-validate -- games.pgn
zstdcat games.pgn.zst | \
  cargo run --release -p gambit-pgn --example stream-validate -- -
```

The first real-corpus baseline uses 810,463 standard-rated Lichess games. See
[the April 2014 benchmark report](docs/benchmarks/lichess-2014-04.md) for the
dataset checksum, reproducible commands, python-chess and Scoutfish comparison,
results, and measurement limitations.

The parser is lexical by design. The next layer should decode SAN directly into
a compact board representation, where legality and position-dependent meaning
belong.
