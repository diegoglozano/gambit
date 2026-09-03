# Getting started

## Install

On Linux and macOS, the cargo-dist installer selects the correct release archive
for your platform:

```console
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/diegoglozano/gambit/releases/latest/download/gambit-installer.sh | sh
```

On Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/diegoglozano/gambit/releases/latest/download/gambit-installer.ps1 | iex"
```

Prebuilt archives and checksums are also available on the
[install page](https://diegoglozano.github.io/gambit/artifacts/).

## Query games

Select games from a corpus and write a new PGN:

```console
gambit query lichess.pgn \
  --player diegoglozano \
  --color black \
  --result loss \
  --since 2026-01-01 \
  > black-losses.pgn
```

Count matches without writing the games, or emit metadata as JSONL:

```console
gambit query lichess.pgn --player diegoglozano --format count
gambit query lichess.pgn --result draw --format jsonl
```

See [Query games](query.md) for filter semantics and the JSONL schema.

## Summarize a corpus

Use Stats for a fast structural inventory before deeper validation or analysis:

```console
gambit stats ./corpus
gambit stats --format json games.pgn.zst
```

Stats counts complete games, mainline plies, outcomes, game lengths, header
coverage, dates, ratings, and fixed distributions in one bounded-memory pass.
See [Corpus statistics](statistics.md) for exact metric semantics and
performance behavior.

## Validate a file

```console
gambit doctor games.pgn
```

Files ending in `.zst` are decompressed automatically, without an external
`zstd` process:

```console
gambit doctor games.pgn.zst
```

Pass several files to validate them as a batch. The error limit applies to each
file independently, and Gambit exits with the most severe status it encounters:

```console
gambit doctor january.pgn february.pgn.zst
```

## Validate a corpus directory

Pass a directory to discover every `.pgn` and `.pgn.zst` file below it:

```console
gambit doctor ./corpus
```

Discovery is recursive and case-insensitive. Gambit ignores unrelated files and
processes the matches in deterministic path order, making the output stable for
scripts and CI. Each discovered file keeps its own report and error limit. A
directory with no matching PGN files exits with status 3 instead of silently
succeeding.

## Read from standard input

Use `-` as the only input to read decompressed PGN from a pipe:

```console
curl -sS https://example.com/game.pgn | gambit doctor -
```

Standard input cannot be mixed with file paths in the same invocation.

## Run Doctor in CI

The `github` output format turns diagnostics into annotations on files and
lines in a pull request while retaining Doctor's normal failure exit status.
See [GitHub Actions](github-actions.md) for a complete workflow.

## Find more than the first problem

The default stops at the first error. `--keep-going` scans later
outcome-delimited games and reports up to 100 diagnostics. Set a smaller or
larger positive limit explicitly with `--max-errors`:

```console
gambit doctor --keep-going games.pgn
gambit doctor --max-errors 20 games.pgn
```

## Exit statuses

| Status | Meaning |
| ---: | --- |
| `0` | Every input is valid. |
| `1` | Invalid PGN or chess data was found. |
| `2` | The command line is invalid. |
| `3` | An input could not be read or a report could not be written. |
