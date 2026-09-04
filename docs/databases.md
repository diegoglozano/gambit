# Gambit databases

A `.gambit` file is a self-contained chess database for repeated local queries.
It keeps the original PGN, normalized filter metadata, and the first occurrence
of every mainline position in one portable file.

Build one from a file, compressed archive, directory, or standard input:

```console
$ gambit index games.pgn.zst --output games.gambit
index: complete
destination: games.gambit
sources: 1
games: 1729
positions: 110388
source PGN bytes: 1311266
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
up temporary files on handled errors.

An existing destination is always refused. This makes a rebuild explicit:

```console
gambit index ./synced-games --output games.next.gambit
gambit query games.next.gambit --format count
mv games.next.gambit games.gambit
```

The source PGN remains the canonical, interoperable input. This first format
does not support in-place append or mutation; rebuilding favors deterministic
layout and simple recovery. Incremental indexing is a future layer over the
same Query contract.

## File format

Schema version 1 is a [SQLite application file](https://www.sqlite.org/appfileformat.html)
with a 32 KiB page size, Gambit's `application_id`, and SQLite `user_version` 1.
Gambit links SQLite into the binary, so no system SQLite installation or server
process is required. Each original game is an independent Zstandard frame for
random extraction. Metadata indexes cover player, date, and result filters; a
covering position index maps keys to games and first plies.

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
parallel parse/SAN workers, batched writer ingestion, visible progress, and
incremental rebuilds. Measurements—not the choice of SQLite itself—decide when
a custom storage engine becomes worthwhile.

## Reports and exit status

`--format json` returns the schema version, source/game/position counts, source
and database byte sizes, elapsed time, and source-byte throughput. Human output
contains the same fields.

Exit status 0 means a complete database was published. Invalid PGN, FEN, or SAN
exits 1; command-line errors exit 2; destination, input, compression, database,
or output failures exit 3.
