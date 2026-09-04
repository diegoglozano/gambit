# Gambit databases

A `.gambit` file is a self-contained chess database for repeated local queries.
It keeps the original PGN, normalized filter metadata, and the first occurrence
of every mainline position in one portable file.

Build one from a file, compressed archive, directory, or standard input:

```console
$ gambit index games.pgn.zst --output games.gambit
index: complete
mode: build
destination: games.gambit
sources written: 1
sources skipped: 0
sources replaced: 0
games written: 1729
positions written: 110388
source PGN bytes scanned: 1311266
source PGN bytes written: 1311266
database bytes: 7929856
elapsed: 0.154s
throughput: 8.14 MiB/s
```

Then use the existing Query contract:

```console
gambit query games.gambit \
  --player diegoglozano \
  --color black \
  --result loss \
  --since 2026-01-01 \
  --format count

gambit query games.gambit \
  --position 'rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2'
```

Count queries use only indexes and metadata. PGN output decompresses only the
matching games, and JSONL reads stored metadata without reparsing movetext. The
same filters therefore work on raw PGN for one-shot streaming jobs and on a
`.gambit` file for repeated interactive analysis.

## Inspect a database

Use Info to see what the file contains without extracting PGN:

```console
$ gambit info games.gambit
database: readable
path: games.gambit
schema version: 2
database bytes: 7929856
sources: 1
fingerprinted sources: 1/1
games: 1729
positions: 110388
mainline plies: 109130
results: 813 white wins, 829 black wins, 87 draws, 0 unfinished
dates: 2025.06.26 to 2026.09.02
stored PGN bytes: 1311263
compressed PGN bytes: 862041 (1.52x)
integrity: not checked (use --check)
elapsed: 0.003s
```

The default read-only summary aggregates database, source, game, position,
result, date, and compression metadata. Stored PGN bytes exclude separators
outside framed games. `fingerprinted sources` shows how many
sources are ready for fast incremental comparison; schema-v1 files report zero
until `index --update` encounters and migrates them.

For a deeper health check, add `--check`:

```console
gambit info --check games.gambit
gambit info --check --format json games.gambit
```

This runs SQLite's bounded `quick_check` diagnostics, validates every
foreign-key relationship, decompresses each stored PGN frame, checks its
declared length, and verifies schema-v2 source fingerprints. It is explicit
because it reads the complete database and can take materially longer than the
normal summary on a large file.
A healthy check prints `integrity: ok` and exits 0. Detected integrity problems
produce an `invalid` report and exit 1; file, schema, database, and output
failures exit 3. Info never modifies the database.

## Build and recovery contract

Indexing is one-pass and bounded-memory with respect to corpus size. Gambit
holds at most the current 16 MiB-limited game plus a fixed SQLite cache. It
executes every standard-chess mainline once and stores one 128-bit position key
per visited game position; variations and explicitly non-standard variants are
excluded from the position index. Repeated positions are collapsed to their
first ply at query time.

The builder writes a uniquely named temporary file beside the destination,
synchronizes the completed database, and publishes it without overwriting an
existing path. A malformed PGN, illegal standard-chess mainline, interruption,
or storage failure leaves no destination claiming to be complete. Retrying the
same command is safe after the temporary file has been removed; Gambit cleans
up temporary files on handled errors. Without `--update`, an existing
destination is always refused.

## Incremental updates

After source files change, update the existing database in place:

```console
gambit sync \
  --lichess-user diegoglozano \
  --output ./synced-games \
  --database games.gambit
```

The first invocation builds `games.gambit`; later invocations update it. The
equivalent manual command remains available as
`gambit index --update ./synced-games --output games.gambit`.

Update works at source-file granularity. Gambit performs a bounded-memory
framing and BLAKE3 fingerprint pass over every supplied source. It skips an
unchanged source before parsing SAN or writing SQLite, appends a new source,
and deletes then reindexes a changed source. A changed source is reopened for
the semantic pass, and its fingerprint is verified again to detect concurrent
modification.

The exact source path string is its identity, so use the same relative or
absolute input paths on the build and subsequent updates. Inputs omitted from
an update remain in the database; this command does not prune them. A monolithic
PGN is one source and must be reindexed in full when it changes. The
one-game-per-file layout produced by `gambit sync` gives the most efficient
incremental path: completed games skip, newly synced games append, and a
refreshed unfinished game replaces only itself.

All sources in one invocation update inside a single immediate transaction.
An invalid or concurrently changed source, interruption, database error, or
storage failure before commit rolls back additions and replacements together.
Readers continue to see the previously committed database. Standard input is
rejected with `--update` because safe fingerprint verification requires a
reopenable source.

## File format

Schema version 2 is a [SQLite application file](https://www.sqlite.org/appfileformat.html)
with a 32 KiB page size, Gambit's `application_id`, and SQLite `user_version` 2.
Gambit links SQLite into the binary, so no system SQLite installation or server
process is required. Each original game is an independent Zstandard frame for
random extraction. Metadata indexes cover player, date, and result filters; a
covering position index maps keys to games and first plies. Version 2 adds a
per-source fingerprint for change detection.

Gambit continues to query schema version 1 databases created by v0.7.0. Their
first update migrates the schema transactionally and lazily reconstructs each
encountered fingerprint from the independently compressed stored games. An
unchanged v0.7 source therefore does not need semantic reindexing.

`.gambit` is the public interchange unit, but its SQL schema is an internal
implementation detail before Gambit 1.0. Query validates the application and
schema identifiers and rejects unrelated or unsupported databases rather than
guessing. Position keys deliberately ignore FEN move counters and normalize
non-capturable en-passant targets, matching raw-PGN position-query semantics.

## HPC tradeoffs

The format is optimized for query amplification: pay the cost of parsing and
legal move execution once, then answer selective queries without scanning or
decompressing the corpus. Construction is currently single-threaded and
SQLite writes dominate large builds. The database also trades disk for direct
access because every searchable position is materialized.

For one query over a large archive, use raw `.pgn.zst`: it avoids the build and
additional storage. Build `.gambit` when the collection is reused, position
search is common, or downstream tools repeatedly extract small subsets. The
large-corpus measurements and reproducible commands live in the
[April 2014 benchmark](benchmarks/lichess-2014-04.md#gambit-database-follow-up).

The next HPC improvements should preserve the file contract while adding
parallel parse/SAN workers, batched writer ingestion, and visible progress.
Measurements—not the choice of SQLite itself—decide when a custom storage
engine becomes worthwhile.

## Reports and exit status

`--format json` returns the mode, schema version, written/skipped/replaced
source counts, written game/position counts, scanned and written source byte
sizes, database size, elapsed time, and scan throughput. Human output contains
the same fields. On update, game and position counts describe work performed in
that invocation rather than totals already stored in the database.

Exit status 0 means a complete database was published. Invalid PGN, FEN, or SAN
exits 1; command-line errors exit 2; destination, input, compression, database,
or output failures exit 3.
