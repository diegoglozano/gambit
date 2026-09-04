# Gambit documentation

Gambit inspects and validates PGN while streaming plain or Zstandard-compressed
files. Point it at one file, several files, or an entire corpus directory.
Query selects reusable subsets, Stats summarizes corpus shape, and Doctor
pinpoints syntax and chess-semantic errors.

[Install Gambit](https://diegoglozano.github.io/gambit/artifacts/){ .md-button }
[Get started](getting-started.md){ .md-button }

## A quick query

```console
gambit query games.pgn --player diegoglozano --result loss --format count
```

Query can emit matching PGN, one JSONL metadata record per match, or only the
count. It can also find games that reach an exact FEN position. Player-relative
filters make questions such as wins, losses, color, and rating unambiguous. See
[Query games](query.md) for the complete contract.

## A quick corpus summary

```console
$ gambit stats tournament.pgn.zst
stats: valid
source: tournament.pgn.zst
bytes: 184320
games: 512
mainline plies: 42117
results: 249 white wins, 231 black wins, 32 draws, 0 unfinished
game length (plies): min 7, avg 82.26, max 281
game-length buckets: 0=1, 1-20=20, 21-40=70, 41-60=120, 61-80=130, 81-120=120, 121-160=40, 161+=11
header coverage: Event 512/512, Site 512/512, Date 500/512, Round 512/512, White 512/512, Black 512/512, Result 512/512
dates: 500 complete (2025.01.03 to 2025.12.19), 8 incomplete/invalid, 4 missing
ratings: 1000 numeric (min 812, avg 1647.22, max 2813), 4 invalid, 20 missing
rating bands: <1000=10, 1000-1199=30, 1200-1399=120, 1400-1599=280, 1600-1799=300, 1800-1999=180, 2000-2199=60, 2200-2399=15, 2400+=5
time controls: sudden death=10, increment=400, moves/period=20, multi-stage=30, hourglass=2, unknown=5, unlimited=5, invalid=4, missing=36
elapsed: 0.001s
throughput: 175.78 MiB/s
```

Stats performs a single lexical pass and keeps memory bounded independently of
corpus size. See [Corpus statistics](statistics.md) for metric definitions and
HPC behavior.

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

- `gambit` provides the Query, Stats, and Doctor command-line workflows.
- `gambit-pgn` is the zero-copy parsing and bounded-memory streaming layer.
- `gambit-chess` provides positions, legal move generation, and SAN execution.

The parser stays lexical by design; position-dependent validation belongs to
the chess core. That separation keeps both layers independently testable and
optimizable.
