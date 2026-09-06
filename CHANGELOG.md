# Changelog

All notable changes to Gambit are documented in this file.

## [Unreleased]

## [0.13.0] - 2026-09-06

### Added

- Desktop filters now update the game list automatically, debounce typed input,
  validate incomplete or conflicting ranges inline, and prevent stale requests
  from replacing newer results.
- Added desktop game sorting by date, rating, result, or White player in either
  direction.
- Added an Explore view for common opening lines, results over time, frequent
  opponents, and recurring positions, with representative-game drill-downs.

## [0.12.0] - 2026-09-06

### Added

- Gambit Desktop now remembers up to 12 recent databases and switches between
  them without replacing or rebuilding either library.
- Added the CLI's indexed player, opponent, color, result, date, rating, and
  position filters to the desktop game browser, with matching-game PGN export.
- Added desktop database integrity verification plus result, storage, and
  position summaries.
- Desktop PGN workflows now build from multiple `.pgn` or `.pgn.zst` files and
  add, skip, or replace sources incrementally using content fingerprints.
- Added an optional one-sync Lichess personal access token for faster account
  exports without persisting the credential.
- Exposed structured full-filter paging and file-based incremental updates from
  the reusable Gambit library shared by the CLI and desktop app.

### Fixed

- Kept macOS Open and Save panels usable with the custom `.gambit` extension by
  validating database paths after selection instead of filtering them as an
  unknown native content type.

## [0.11.0] - 2026-09-06

### Added

- Added live Lichess sync progress to Gambit Desktop with downloaded-game
  counts, the latest history date reached, and a separate indexing phase.
- Gambit Desktop now opens games from the selected player's perspective and
  includes a manual board-flip control.

### Fixed

- Disabled autocapitalization, autocorrection, and spellchecking for Lichess
  usernames and exact player filters.
- Kept arrow-key replay navigation from scrolling the application window while
  still following the active move inside its own list.

## [0.10.0] - 2026-09-06

### Added

- Added native PGN import to Gambit Desktop for building and opening a new
  `.gambit` database from a `.pgn` or `.pgn.zst` file.
- Added signed in-app updates to Gambit Desktop, with automatic update checks,
  release notes, and user-confirmed installation and restart.

### Fixed

- Kept all 64 chessboard cells square regardless of whether a rank contains
  pieces.

## [0.9.0] - 2026-09-05

### Added

- Added the first Gambit Desktop preview: a local-first Tauri app that syncs a
  public Lichess account or opens an existing `.gambit` database, pages and
  filters games, and replays legal mainlines on an interactive chessboard.
- Added a reusable Gambit Rust library surface for collection sync, database
  inspection, paged game metadata, stored PGN, and per-ply board states without
  invoking or parsing the CLI.
- Gambit Desktop now remembers and automatically reopens the last library.
- Versioned releases now attach a universal macOS DMG and SHA-256 checksum,
  with optional Developer ID signing and Apple notarization through CI secrets.

## [0.8.0] - 2026-09-05

### Added

- Added transactional `gambit index --update` for source-granular incremental
  databases: unchanged sources bypass semantic indexing, new sources append,
  changed sources replace their prior games, and failed batches roll back.
- Added automatic schema-v1 database migration with lazy source-fingerprint
  recovery, keeping v0.7 databases queryable and incrementally updatable.
- Added `gambit info` for database size, schema and fingerprint coverage,
  corpus totals, result/date summaries, compression ratio, JSON output, and
  optional SQLite, relationship, stored-PGN, and fingerprint integrity checks.
- Added `gambit sync --database <FILE>` to build a `.gambit` database after the
  first committed sync and update it source-by-source on every later run.

## [0.7.0] - 2026-09-04

### Added

- Added `gambit index` to build self-contained, query-optimized `.gambit`
  databases from streaming PGN, with original-game extraction, metadata
  indexes, exact mainline-position lookup, bounded memory, and atomic
  no-overwrite publication.
- Extended `gambit query` to use `.gambit` metadata and position indexes for
  counts and JSONL, decompressing stored PGN only for matching PGN output.

## [0.6.1] - 2026-09-04

### Fixed

- Removed the fixed 30-second response deadline from streamed Lichess exports,
  which truncated healthy queries and syncs for larger collections.
- Prevented `gambit query --format count` from printing an incomplete total
  when any input fails.

## [0.6.0] - 2026-09-04

### Added

- Added `gambit sync` for resumable Lichess collections, with per-game PGN
  storage, idempotent overlap, unfinished-game refresh, committed cursors, and
  human or JSON reports.
- Added direct bounded-memory Lichess user queries with `--lichess-user`,
  optional `LICHESS_TOKEN` authentication, upstream date/opponent/color
  filtering, and a `--max-games` request limit.
- Added exact standard-chess position filtering to `gambit query` with
  `--position <FEN>`, including FEN starts, first-match ply reporting in JSONL,
  and composition with every metadata filter.

## [0.5.0] - 2026-09-03

### Added

- Added `gambit query` for bounded-memory metadata filtering with player-relative
  color, result, opponent, date, and rating predicates, emitting PGN, JSONL, or
  a match count.

## [0.4.0] - 2026-09-03

### Added

- Added `gambit stats` for single-pass corpus summaries: decompressed bytes,
  complete games, mainline plies, result distribution, and game-length range
  and average.
- Added human and JSON Stats reports for files, recursive directories,
  `.pgn.zst` streams, and standard input, including aggregate batch metrics and
  partial counters when an input is malformed.
- Added Seven Tag Roster coverage, complete `Date`/`UTCDate` ranges, and
  `WhiteElo`/`BlackElo` coverage and summary statistics without retaining
  high-cardinality tag values.
- Added fixed, exactly mergeable distributions for game length and Elo, plus
  structural categories for PGN `TimeControl` values.

### Changed

- Exposed incremental-parser I/O statistics after an error so streaming callers
  can report partial progress without a second pass.

## [0.3.0] - 2026-09-02

### Added

- Added recursive directory inputs to `gambit doctor`, with deterministic
  discovery of `.pgn` and `.pgn.zst` files and explicit empty-directory errors.
- Added a `github` output format that emits native GitHub Actions error
  annotations and a concise validation summary.

## [0.2.0] - 2026-09-02

### Added

- Added `gambit doctor` for PGN syntax and chess-semantic validation, with
  stable exit codes and human or JSON reports.
- Added actionable diagnostics with game headers, ply, byte offset, line,
  column, source excerpt, and machine-readable categories.
- Added complete corpus scans with `--keep-going`, `--max-errors`, and JSONL
  output.
- Added direct streaming validation of `.pgn.zst` files and ordered multi-file
  scans with aggregate JSON and JSONL summaries.
- Added consistency checks for `Result` outcomes, `SetUp`/`FEN` metadata, and
  explicit move numbers, including FEN starts and recursive variations.

### Changed

- Added CI coverage for the minimum supported Rust version and CLI behavior on
  Linux, macOS, and Windows.
- Kept the original `gambit <FILE>` invocation as a compatibility alias for
  `gambit doctor <FILE>`.

## [0.1.0] - 2026-09-02

- First binary release for Linux, macOS, and Windows, with shell and PowerShell
  installers.

[Unreleased]: https://github.com/diegoglozano/gambit/compare/v0.13.0...HEAD
[0.13.0]: https://github.com/diegoglozano/gambit/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/diegoglozano/gambit/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/diegoglozano/gambit/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/diegoglozano/gambit/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/diegoglozano/gambit/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/diegoglozano/gambit/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/diegoglozano/gambit/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/diegoglozano/gambit/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/diegoglozano/gambit/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/diegoglozano/gambit/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/diegoglozano/gambit/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/diegoglozano/gambit/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/diegoglozano/gambit/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/diegoglozano/gambit/releases/tag/v0.1.0
