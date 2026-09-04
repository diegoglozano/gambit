# CLI reference

## Synopsis

```text
gambit doctor [OPTIONS] <PATH|->...
gambit stats [OPTIONS] <PATH|->...
gambit index [OPTIONS] --output <FILE> <PATH|->...
gambit info [OPTIONS] <FILE>
gambit query [OPTIONS] <PATH|->...
gambit query [OPTIONS] --lichess-user <NAME>
gambit sync --lichess-user <NAME> --output <DIRECTORY>
gambit <PATH|->
```

The direct path form is a compatibility alias for `gambit doctor` and accepts
exactly one input. Use the `doctor` command for options or batch validation.

## Inputs

A path can name a file or a directory. Files ending in `.zst` are decompressed
automatically. Directory inputs are scanned recursively and include regular
files or file symlinks ending in `.pgn` or `.pgn.zst`, case-insensitively.
Other directory entries are ignored, and directory symlinks are not followed.

Discovered files are processed in deterministic path order. An empty directory
is an input error, as is a directory entry that cannot be read or inspected.
When several paths are supplied, each directory is expanded in its argument
position.

Use `-` alone to read decompressed PGN from standard input. Standard input
cannot be combined with any other input path.

## Doctor options

| Option | Description |
| --- | --- |
| `--format <human|json|jsonl|github>` | Select the report format. The default is `human`. |
| `--syntax-only` | Parse PGN structure without executing moves. |
| `--lenient` | Allow a final game without an outcome marker. |
| `--keep-going` | Continue after errors, up to 100 per input. |
| `--max-errors <N>` | Continue until `N` errors have been reported per input. |
| `-q`, `--quiet` | Print nothing when human-format validation succeeds. |
| `-h`, `--help` | Print help. |
| `-V`, `--version` | Print the version. |

`--quiet` cannot be combined with a machine-readable format.

## Stats options

| Option | Description |
| --- | --- |
| `--format <human|json>` | Select the report format. The default is `human`. |
| `--lenient` | Allow a final game without an outcome marker and count it as unfinished. |
| `-h`, `--help` | Print help. |

Stats accepts the same file, directory, compressed, and standard-input forms as
Doctor. It performs structural parsing without executing chess moves and
reports corpus shape, Seven Tag Roster coverage, date quality/range, and Elo
quality/range. Fixed distributions describe game lengths, rating bands, and
PGN time-control forms. See [Corpus statistics](statistics.md) for metric
definitions, JSON shape, and HPC behavior.

## Index options

| Option | Description |
| --- | --- |
| `-o`, `--output <FILE>` | Select the `.gambit` database. Required. A build refuses an existing path; `--update` requires one. |
| `--update` | Add new sources and replace changed sources in one transaction. Standard input is not supported because inputs must be reopened safely. |
| `--format <human|json>` | Select the completion report format. The default is `human`. |
| `-h`, `--help` | Print help. |

Index accepts the same PGN input forms as Doctor and Stats. It semantically
executes standard-chess mainlines, stores the original PGN and query metadata,
and builds a position lookup in bounded memory. A new database is published
only after the complete build succeeds. Updates fingerprint every named source,
skip unchanged sources before semantic work, and atomically commit the complete
batch. See [Gambit databases](databases.md).

## Info options

| Option | Description |
| --- | --- |
| `--check` | Check SQLite structure, foreign keys, stored PGN frames, and source fingerprints in addition to the summary. |
| `--format <human|json>` | Select the report format. The default is `human`. |
| `-h`, `--help` | Print help. |

Info accepts exactly one `.gambit` file. The normal summary is read-only and
reports storage and chess-corpus totals. `--check` can scan the complete
database and exits 1 if it finds an integrity problem. See
[Gambit databases](databases.md#inspect-a-database).

## Query options

| Option | Description |
| --- | --- |
| `--lichess-user <NAME>` | Stream this user's public games from the Lichess API and select that player implicitly. Cannot be combined with a path or `--player`. |
| `--max-games <N>` | Request at most `N` of the newest games. Requires `--lichess-user`. |
| `--player <NAME>` | Match games containing this player, case-insensitively. |
| `--opponent <NAME>` | Match the selected player's opponent. Requires `--player`. |
| `--color <white|black>` | Match the selected player's color. Requires `--player`. |
| `--result <win|loss|draw|unfinished>` | Match the selected player's result. `win` and `loss` require `--player`. |
| `--since <YYYY-MM-DD>` | Match games on or after this inclusive date. |
| `--until <YYYY-MM-DD>` | Match games on or before this inclusive date. |
| `--min-rating <ELO>` | Match the selected player's minimum rating. Requires `--player`. |
| `--max-rating <ELO>` | Match the selected player's maximum rating. Requires `--player`. |
| `--position <FEN>` | Match standard-chess games reaching this six-field FEN position. |
| `--format <pgn|jsonl|count>` | Select output. The default is `pgn`. |
| `-h`, `--help` | Print help. |

Query accepts the same file input forms as Doctor and Stats, an explicit
`.gambit` database, or one Lichess user as a remote input. Set the optional
`LICHESS_TOKEN` environment variable for authenticated Lichess access. See
[Query games](query.md) for missing-metadata behavior, remote-source semantics,
output contracts, and bounded-memory details.

## Sync options

| Option | Description |
| --- | --- |
| `--lichess-user <NAME>` | Select the Lichess account to synchronize. Required. |
| `--output <DIRECTORY>` | Select an empty or existing Gambit sync destination. Required. |
| `--database <FILE>` | Build this `.gambit` database after the first successful sync and incrementally update it after later syncs. |
| `--since <YYYY-MM-DD>` | Set an inclusive history boundary when initializing a new destination. |
| `--format <human|json>` | Select the report format. The default is `human`. |
| `-h`, `--help` | Print help. |

Sync stores one PGN per Lichess game ID and advances its cursor only after the
complete stream succeeds. Later runs fetch an overlapping incremental window
and refresh previously unfinished games. `--database` then maintains a
query-optimized database from that local canonical store. See
[Sync Lichess games](sync.md) for the storage, recovery, and automation
contracts.

## Validation modes

Semantic validation is the default. It reports malformed PGN, invalid FEN
starting positions, malformed or illegal SAN, ambiguous moves, and incorrect
check or mate suffixes. It also verifies that:

- the `Result` header agrees with the movetext outcome;
- `SetUp` and `FEN` occur together correctly;
- explicit move numbers match the live position and side to move;
- those rules continue to hold inside recursive variations and FEN starts.

`--syntax-only` intentionally skips position-dependent and cross-field checks.

## Machine-readable reports

JSON emits one report when exactly one file is resolved. For a multi-file or
directory batch, it wraps the per-file reports in a batch summary. The first
diagnostic remains in `diagnostic`, while later diagnostics appear in
`additional_diagnostics`.

```console
gambit doctor --format json games.pgn
```

JSONL emits one diagnostic record per line followed by a summary record. A
batch invocation ends with an additional `batch_summary` record, making it
suitable for incremental consumers:

```console
gambit doctor --keep-going --format jsonl games.pgn.zst
```

The `github` format writes one GitHub Actions `error` workflow command per
diagnostic, including the source path, line, and column when available. It ends
with one plain-text summary and retains the standard exit status, so invalid
PGN fails the workflow step automatically:

```console
gambit doctor --keep-going --format github ./corpus
```

Workflow-command data and properties are escaped according to GitHub's command
protocol. Outside GitHub Actions the command records are printed literally;
use `human`, `json`, or `jsonl` for other consumers.

## Diagnostic locations

Human diagnostics include the game number and identifying headers when
available, the ply, byte offset, line, column, and a source-line excerpt. Lines
and byte columns are one-based.
