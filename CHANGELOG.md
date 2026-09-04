# Changelog

All notable changes to Gambit are documented in this file.

## [Unreleased]

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

[Unreleased]: https://github.com/diegoglozano/gambit/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/diegoglozano/gambit/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/diegoglozano/gambit/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/diegoglozano/gambit/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/diegoglozano/gambit/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/diegoglozano/gambit/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/diegoglozano/gambit/releases/tag/v0.1.0
