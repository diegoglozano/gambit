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
game-length buckets: 0=188, 1-20=44878, 21-40=110628, 41-60=207384, 61-80=199448, 81-120=196634, 121-160=45085, 161+=6218
header coverage: Event 810463/810463, Site 810463/810463, Date 0/810463, Round 0/810463, White 810463/810463, Black 810463/810463, Result 810463/810463
dates: 810463 complete (2014.03.31 to 2014.04.30), 0 incomplete/invalid, 0 missing
ratings: 1620815 numeric (min 732, avg 1619.09, max 2734), 111 invalid, 0 missing
rating bands: <1000=4569, 1000-1199=44145, 1200-1399=218466, 1400-1599=493775, 1600-1799=517927, 1800-1999=261806, 2000-2199=69170, 2200-2399=9769, 2400+=1188
time controls: sudden death=0, increment=808167, moves/period=0, multi-stage=0, hourglass=0, unknown=0, unlimited=2296, invalid=0, missing=0
elapsed: 1.915s
throughput: 349.57 MiB/s
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
- game-length buckets count `0`, `1–20`, `21–40`, `41–60`, `61–80`, `81–120`,
  `121–160`, and `161+` mainline plies. The buckets sum to `games`.
- `header coverage` counts complete games containing each case-sensitive Seven
  Tag Roster name. Values are presence counts, so duplicate tags do not inflate
  them.
- `dates` uses the first `Date` value, falling back to the first `UTCDate` when
  `Date` is absent. Only real `YYYY.MM.DD` calendar dates contribute to the
  range; partial dates such as `????.??.??` are counted as incomplete.
- `ratings` combines the first `WhiteElo` and `BlackElo` values into two player
  slots per game. Unsigned decimal values contribute to the range and
  arithmetic mean; absent and invalid values are counted separately.
- rating bands count `<1000`, each 200-point interval from `1000–1199` through
  `2200–2399`, and `2400+`. They sum to the numeric rating count.
- `time controls` classifies the first `TimeControl` value by PGN form:
  sudden-death seconds (`N`), increment (`N+N`), moves per period (`N/N`),
  colon-separated multi-stage, or hourglass (`*N`). `?`, `-`, malformed, and
  absent values have separate counters. The categories sum to `games`.

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
    "maximum_plies": 281,
    "distribution": {
      "zero": 1,
      "from_1_to_20": 20,
      "from_21_to_40": 70,
      "from_41_to_60": 120,
      "from_61_to_80": 130,
      "from_81_to_120": 120,
      "from_121_to_160": 40,
      "at_least_161": 11
    }
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
    "maximum": 2813,
    "distribution": {
      "under_1000": 10,
      "from_1000_to_1199": 30,
      "from_1200_to_1399": 120,
      "from_1400_to_1599": 280,
      "from_1600_to_1799": 300,
      "from_1800_to_1999": 180,
      "from_2000_to_2199": 60,
      "from_2200_to_2399": 15,
      "at_least_2400": 5
    }
  },
  "time_controls": {
    "sudden_death": 10,
    "increment": 400,
    "moves_per_period": 20,
    "multi_stage": 30,
    "hourglass": 2,
    "unknown": 5,
    "unlimited": 5,
    "invalid": 4,
    "missing": 36
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
