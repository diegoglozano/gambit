# Changelog

All notable changes to Gambit are documented in this file.

## [Unreleased]

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

[Unreleased]: https://github.com/diegoglozano/gambit/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/diegoglozano/gambit/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/diegoglozano/gambit/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/diegoglozano/gambit/releases/tag/v0.1.0
