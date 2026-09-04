# Query games

`gambit query` selects games from PGN without first importing them into a
database. It is the query contract for Gambit's future persistent index and is
already fast enough for personal archives and one-shot corpus searches.

## Start with your own games

Read public games directly from Lichess and count losses as Black from 2026
onward:

```console
gambit query --lichess-user diegoglozano \
  --color black \
  --result loss \
  --since 2026-01-01 \
  --format count
```

`--lichess-user` is both the input source and the selected player, so it cannot
be combined with a file path or `--player`. Add `--max-games 25` to inspect at
most the newest 25 games returned by Lichess. With PGN output, the same command
can create an archive explicitly:

```console
gambit query --lichess-user diegoglozano > lichess-diegoglozano.pgn
```

Public games need no credentials. To authenticate, create a
[Lichess personal access token](https://lichess.org/account/oauth/token) and
expose it only to the Gambit process:

```console
LICHESS_TOKEN=lip_example gambit query --lichess-user diegoglozano --format count
```

Gambit reads `LICHESS_TOKEN` from the process environment. It does not silently
load `.env` files; a shell, secret manager, or environment loader may provide
the variable from one. The token is sent only in the Lichess authorization
header and is never included in output or diagnostics. Authentication raises
Lichess's export rate for your own games, but is optional for public data.

The response is processed as it arrives and is not cached. Date, opponent, and
color constraints are sent to Lichess to reduce transfer, then every predicate
is evaluated locally for the same behavior as file queries. `--max-games`
limits the games returned after those server-side constraints and before local
result, rating, or position filtering. Lichess serves the newest games first.

Lichess may rate-limit clients. Gambit makes one request at a time and reports
an actionable input error on HTTP 429 rather than silently retrying and risking
duplicate streaming output. Wait at least one minute before trying again.

For an existing local export, use a file exactly as before:

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

## Search by position

Pass a complete six-field FEN to find every standard-chess game whose mainline
reaches that position:

```console
gambit query lichess-diegoglozano.pgn \
  --position 'rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2' \
  --format count
```

Shell quotes are important because a FEN contains spaces. The starting
position is tested as ply 0, followed by the position after every mainline SAN
move. Recursive variations are excluded. A game is emitted once even if it
reaches the target repeatedly; JSONL adds `position_ply` with the first
matching zero-based ply.

Position identity compares piece placement, side to move, castling rights, and
effective en-passant availability. The halfmove clock and fullmove number are
accepted but ignored, so a FEN copied from another tool still finds a position
reached at a different move number. Non-capturable en-passant target squares
are normalized to `-` for compatibility with common FEN exporters.

Games with no `Variant` tag and games tagged `Variant "Standard"` are searched.
Explicit non-standard variants such as Crazyhouse, Atomic, and Chess960 are
valid PGN inputs but cannot represent the same standard-chess position, so
they are skipped as non-matches. Games starting from a valid `FEN` tag are
supported.

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
the source file. Position-filtered records also contain `position_ply`, where
0 is the game's initial position and 1 is the position after White's first
move.

## Inputs and resource bounds

Query accepts plain PGN, `.pgn.zst`, multiple files, recursive directories,
decompressed standard input, or one Lichess user. Directory discovery is
deterministic. Output from all resolved files is concatenated, and `count`
reports the aggregate. A Lichess source is streamed over HTTPS and cannot be
combined with another input.

The corpus is never materialized. Query retains only the current game, using a
64 KiB read buffer and a 16 MiB maximum game-size safety limit. Memory use is
therefore bounded independently of corpus size. Metadata-only filters stay on
the lexical path and do not execute SAN. `--position` activates legal SAN
execution only for standard-chess games, retaining a single 104-byte live
position in addition to the current game.

On the 810,463-game Lichess April 2014 baseline, a player-filtered count
sustains 242.49 MiB/s from decompressed PGN and 202.81 MiB/s with in-process
Zstandard decompression; see the
[metadata benchmark](benchmarks/lichess-2014-04.md#metadata-query-follow-up).
Exact-position search executes 54.7 million moves from the same corpus at
103.60 MiB/s from plain PGN and 96.30 MiB/s from `.pgn.zst`, with maximum RSS
of 1.90 MB and 10.93 MB respectively; see the
[position-query benchmark](benchmarks/lichess-2014-04.md#position-query-follow-up).

The [public monthly archives](https://database.lichess.org/) can be piped through
Gambit, but they are not indexed by username. Avoiding local disk is possible;
avoiding the network transfer and full scan is not. Per-user lookups should
therefore use `--lichess-user`. A resumable local user cache is planned
separately.

Exit status 0 means the query completed, including when it matched no games.
Malformed PGN, an invalid standard-game FEN start, illegal mainline SAN during
position search, or a game exceeding the safety limit exits 1. An invalid
`--position` value is a usage error and exits 2. Input, decompression, or output
failures—including Lichess transport, authentication, not-found, and rate-limit
errors—exit 3.
