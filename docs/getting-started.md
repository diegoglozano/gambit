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
