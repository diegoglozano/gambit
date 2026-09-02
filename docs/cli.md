# CLI reference

## Synopsis

```text
gambit doctor [OPTIONS] <PATH|->...
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

## Options

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
