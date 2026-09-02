# Corpus statistics

`gambit stats` answers the first operational questions about a PGN corpus
without loading games into memory:

```console
$ gambit stats lichess_db_standard_rated_2014-04.pgn.zst
stats: valid
source: lichess_db_standard_rated_2014-04.pgn.zst
bytes: 701772510
games: 810463
mainline plies: 54748499
results: 410370 white wins, 372707 black wins, 27386 draws, 0 unfinished
game length (plies): min 0, avg 67.55, max 344
header coverage: Event 810463/810463, Site 810463/810463, Date 0/810463, Round 0/810463, White 810463/810463, Black 810463/810463, Result 810463/810463
dates: 810463 complete (2014.03.31 to 2014.04.30), 0 incomplete/invalid, 0 missing
ratings: 1620815 numeric (min 732, avg 1619.09, max 2734), 111 invalid, 0 missing
elapsed: 1.903s
throughput: 351.67 MiB/s
```

This is the median end-to-end throughput from five runs on the compressed
April 2014 Lichess baseline. See the
[benchmark report](benchmarks/lichess-2014-04.md#gambit-stats-result) for the
environment, reproduction commands, all runs, and memory measurements.

## What is counted

- `bytes` is the number of decompressed bytes read. For `.pgn.zst`, it is the
  PGN size rather than the compressed file size.
- `games` counts structurally complete games.
- `mainline plies` counts SAN tokens on the main line. Moves in recursive
  annotation variations are excluded.
- `results` counts the mainline `1-0`, `0-1`, `1/2-1/2`, and `*` markers.
- game length is the minimum, arithmetic mean, and maximum mainline ply count
  across complete games.
- `header coverage` counts complete games containing each case-sensitive Seven
  Tag Roster name. Values are presence counts, so duplicate tags do not inflate
  them.
- `dates` uses the first `Date` value, falling back to the first `UTCDate` when
  `Date` is absent. Only real `YYYY.MM.DD` calendar dates contribute to the
  range; partial dates such as `????.??.??` are counted as incomplete.
- `ratings` combines the first `WhiteElo` and `BlackElo` values into two player
  slots per game. Unsigned decimal values contribute to the range and
  arithmetic mean; absent and invalid values are counted separately.

Stats intentionally does not retain player names, event names, or other tag
values. Exact distinct-value counts would make memory grow with corpus
cardinality, violating the bounded-memory contract.

Missing, incomplete, or invalid metadata is a quality signal, not a PGN syntax
failure. These counters do not change the scan status or exit code.

Stats performs lexical PGN parsing. It does not execute moves or establish that
SAN is legal in the current chess position. Run `gambit doctor` when semantic
validation is required.

## HPC behavior

Stats uses the fused incremental parser and updates fixed-size counters in the
event callback. Each decompressed byte is parsed once, complete games are not
materialized, and memory use does not grow with the corpus. The reusable input
buffer is 64 KiB; a single token, tag, or comment may grow to the 16 MiB safety
limit.

Files in a batch are currently processed sequentially in deterministic path
order. This makes throughput predictable for one large stream and avoids
oversubscribing storage or Zstandard decompression. Shard a corpus across
processes when the storage system can sustain concurrent readers. Native
multi-file parallelism should be added only with workload measurements that
show a benefit.

Elapsed time includes parsing and decompression reads. Reported throughput is
decompressed MiB divided by that elapsed time. In a multi-file report, elapsed
times are summed, so the aggregate is the effective sequential throughput.

## Batches and compressed input

Stats accepts the same inputs as Doctor: one or more files, recursively scanned
directories, `.pgn.zst` files, or decompressed standard input:

```console
gambit stats january.pgn february.pgn.zst
gambit stats ./sharded-corpus
zstdcat archive.pgn.zst | gambit stats -
```

A batch summary includes valid, invalid, and unreadable input counts. Invalid
or unreadable files do not discard counters from completed games in other
files.

## JSON output

Use JSON for schedulers, data pipelines, and regression tracking:

```console
gambit stats --format json ./corpus
```

One resolved input produces one report. Multiple resolved inputs produce an
aggregate object with a `reports` array. The schema distinguishes
`mainline_plies` from full chess moves and groups outcome and length metrics:

```json
{
  "schema_version": 1,
  "status": "valid",
  "source": "games.pgn",
  "outcome_required": true,
  "bytes": 184320,
  "games": 512,
  "mainline_plies": 42117,
  "results": {
    "white_wins": 249,
    "black_wins": 231,
    "draws": 32,
    "unfinished": 0
  },
  "game_length": {
    "minimum_plies": 7,
    "average_plies": 82.259765625,
    "maximum_plies": 281
  },
  "header_coverage": {
    "event": 512,
    "site": 512,
    "date": 500,
    "round": 512,
    "white": 512,
    "black": 512,
    "result": 512
  },
  "dates": {
    "complete": 500,
    "incomplete_or_invalid": 8,
    "missing": 4,
    "earliest": "2025.01.03",
    "latest": "2025.12.19"
  },
  "ratings": {
    "numeric": 1000,
    "invalid": 4,
    "missing": 20,
    "minimum": 812,
    "average": 1647.218,
    "maximum": 2813
  },
  "elapsed_seconds": 0.012,
  "throughput_mib_per_second": 14.6484375,
  "diagnostic": null
}
```

## Invalid and incomplete data

Strict mode requires every game to end with an outcome. `--lenient` permits a
final game without one and counts its result as unfinished:

```console
gambit stats --lenient fragment.pgn
```

On malformed PGN, Stats exits with status 1 and reports counters for complete
games before the error. Those counters are partial; the `status` and
`diagnostic` fields prevent them from being mistaken for a complete scan.
Unreadable input, corrupt compressed data, or the streaming token limit exits
with status 3.
