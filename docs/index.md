# Gambit documentation

Gambit validates PGN syntax and chess semantics while streaming plain or
Zstandard-compressed files. Point it at one file, several files, or an entire
corpus directory. It reports the exact game, move, byte, line, and column where
a problem occurs and provides JSON and JSONL output for automated pipelines.

[Install Gambit](https://diegoglozano.github.io/gambit/artifacts/){ .md-button }
[Get started](getting-started.md){ .md-button }

## A quick diagnosis

```console
$ gambit doctor tournament.pgn
valid: tournament.pgn
mode: semantic
bytes: 184320
games: 512
moves: 42117
elapsed: 0.012s
throughput: 14.65 MiB/s
```

The default semantic mode checks both the PGN structure and every move against
the live position. Use `--syntax-only` when you only need structural parsing.

## Choose an output format

Human-readable diagnostics are the default. CI and editor integrations can use
one JSON document per input or a stream of JSONL diagnostic records. GitHub
Actions can render Doctor failures as native annotations:

```console
gambit doctor --format json game.pgn
gambit doctor --keep-going --format jsonl corpus.pgn.zst
gambit doctor --keep-going --format github ./corpus
```

## What is in this repository?

- `gambit` is the command-line PGN doctor.
- `gambit-pgn` is the zero-copy parsing and bounded-memory streaming layer.
- `gambit-chess` provides positions, legal move generation, and SAN execution.

The parser stays lexical by design; position-dependent validation belongs to
the chess core. That separation keeps both layers independently testable and
optimizable.
