# Query games

`gambit query` selects games from PGN without first importing them into a
database. It is the query contract for Gambit's future persistent index and is
already fast enough for personal archives and one-shot corpus searches.

## Start with your own games

Count every game belonging to a player:

```console
gambit query lichess-diegoglozano.pgn \
  --player diegoglozano \
  --format count
```

Export losses as Black from 2026 onward:

```console
gambit query lichess-diegoglozano.pgn \
  --player diegoglozano \
  --color black \
  --result loss \
  --since 2026-01-01 \
  > black-losses-2026.pgn
```

The default `pgn` output contains complete matching games and no status text,
so it can be redirected, compressed, validated, or queried again:

```console
gambit doctor black-losses-2026.pgn
gambit stats black-losses-2026.pgn
gambit query black-losses-2026.pgn --opponent example --player diegoglozano
```

## Filter semantics

All supplied filters must match. `--since` and `--until` are inclusive and
accept real `YYYY-MM-DD` dates. A game's first `Date` tag is used, falling back
to its first `UTCDate` only when `Date` is absent. Games with missing, partial,
or invalid effective dates do not match a date filter.

`--player` matches the first `White` or `Black` value using ASCII
case-insensitive comparison. The following predicates are evaluated from that
player's side of the board:

- `--color white|black`;
- `--result win|loss`;
- `--opponent NAME`;
- `--min-rating ELO` and `--max-rating ELO`.

These filters require `--player`. A missing or non-numeric rating does not
match a rating bound. `--result draw|unfinished` may be used without a player
because those outcomes are not side-dependent.

Options can occur before or after input paths and accept both `--option value`
and `--option=value` forms.

## Output formats

### PGN

`--format pgn` is the default. It writes complete matching games to standard
output, separated by blank lines. Diagnostics go to standard error. If a later
game is malformed, previously emitted matches remain valid streaming output
and Query exits non-zero.

### Count

`--format count` writes one unsigned decimal match count followed by a newline.
It is intentionally minimal for shell scripts:

```console
losses=$(gambit query games.pgn --player Ada --result loss --format count)
```

### JSONL

`--format jsonl` writes one object per matching game. No summary record is
added, so each line has the same schema and can be consumed incrementally:

```json
{"schema_version":1,"source":"games.pgn","game":42,"event":"rated blitz game","site":"https://lichess.org/example","date":"2026.09.02","white":"Ada","black":"Grace","white_elo":1512,"black_elo":1498,"result":"white_win","mainline_plies":67}
```

Optional or invalid metadata fields are omitted. `result` is one of
`white_win`, `black_win`, `draw`, or `unfinished`; it remains absolute even
when the filter was player-relative. `game` is the one-based game number within
the source file.

## Inputs and resource bounds

Query accepts plain PGN, `.pgn.zst`, multiple files, recursive directories, or
decompressed standard input. Directory discovery is deterministic. Output from
all resolved files is concatenated, and `count` reports the aggregate.

The corpus is never materialized. Query retains only the current game, using a
64 KiB read buffer and a 16 MiB maximum game-size safety limit. Memory use is
therefore bounded independently of corpus size. Each framed game is parsed
lexically; Query checks PGN structure but does not execute SAN against a chess
position. Use Doctor when legality matters. On the 810,463-game Lichess April
2014 baseline, a player-filtered count sustains 242.49 MiB/s from decompressed
PGN and 202.81 MiB/s with in-process Zstandard decompression; see the
[benchmark report](benchmarks/lichess-2014-04.md#metadata-query-follow-up).

Exit status 0 means the query completed, including when it matched no games.
Malformed PGN or a game exceeding the safety limit exits 1. Input, decompression,
or output failures exit 3.
