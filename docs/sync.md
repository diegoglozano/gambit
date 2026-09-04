# Sync Lichess games

`gambit sync` maintains a local, resumable copy of one Lichess user's games.
The result is an ordinary directory of PGN files that Query, Stats, Doctor, and
other chess tools can read without another network request.

## Create a local store

Choose an empty destination and run the first sync:

```console
gambit sync \
  --lichess-user diegoglozano \
  --output ./diegoglozano-games
```

Add `--database` when repeated queries should use a maintained `.gambit` file:

```console
gambit sync \
  --lichess-user diegoglozano \
  --output ./diegoglozano-games \
  --database diegoglozano.gambit
```

The first successful sync builds the database. Running the same command later
updates it: unchanged game files skip semantic indexing, new games append, and
refreshed unfinished games replace their previous records. The PGN directory
remains the canonical, interoperable store.

The first run downloads the full public history. For a smaller initial store,
set an inclusive starting date:

```console
gambit sync \
  --lichess-user diegoglozano \
  --since 2026-01-01 \
  --output ./diegoglozano-games
```

`--since` establishes the history boundary and can only be used while
initializing a store. Later runs use the persisted cursor:

```console
gambit sync --lichess-user diegoglozano --output ./diegoglozano-games
```

Public games need no credentials. Set `LICHESS_TOKEN` in the process
environment to authenticate, using the same security model as direct
[Lichess queries](query.md#start-with-your-own-games).

## Use the synchronized games

The destination is already a valid Gambit corpus:

```console
gambit stats ./diegoglozano-games

gambit query ./diegoglozano-games \
  --player diegoglozano \
  --result loss \
  --format count

gambit doctor ./diegoglozano-games
```

Because the store is local, repeated metadata and position queries run at
Gambit's normal filesystem throughput instead of the Lichess export rate.
When `--database` is enabled, point Query at that file for indexed lookups:

```console
gambit query diegoglozano.gambit \
  --player diegoglozano \
  --result loss \
  --format count
```

## Storage contract

Each game is stored under `games/`, keyed by Lichess's stable eight-character
game ID. Filenames use a lowercase hexadecimal encoding of that ID so distinct
mixed-case IDs remain distinct on case-insensitive macOS and Windows
filesystems. The original ID and URL remain visible in the PGN `Site` tag. A
private `.gambit-sync.json` file records the source, user, cursor, and unfinished
games. Do not edit that state file manually.

The destination must either be empty or already contain a compatible Gambit
sync state for the same Lichess user. Gambit will not claim a nonempty unmanaged
directory or reuse a store belonging to another account. It never deletes
finished game files, even if a game later becomes unavailable remotely.

Writes are idempotent. Repeated games replace the same path, unchanged PGN is
not rewritten, and the cursor advances only after the complete API response and
all game writes succeed. If a process or network failure interrupts a run, use
the same command again.

Database maintenance starts only after the game files and cursor commit. Its
build uses atomic no-overwrite publication; later updates use one SQLite
transaction. If database maintenance fails, Sync exits 3 and says explicitly
that the files and cursor were already committed. Rerun the same command to
repair or catch up the database. The database path is a command option rather
than persisted sync state, so include `--database` on every run that should
maintain it. Adding the option to an existing sync store builds its first
database normally.

## Incremental behavior

Every successful run records its request boundary in Unix milliseconds. The
next run starts five minutes before that cursor. This small overlap handles
clock skew and boundary races without creating duplicate games because storage
is keyed by game ID.

The user export includes ongoing games. Their IDs remain in sync state and are
refreshed individually on later runs until Lichess reports a final outcome.
That prevents a long-running correspondence game created before the cursor from
remaining permanently unfinished in the local store.

Lichess serves exports as a throttled stream, so the initial run can take time
for accounts with large histories. Gambit issues requests sequentially, keeps
only the current game in memory, and reports HTTP 429 with the same one-minute
backoff guidance as direct Query. Do not run two sync processes against the same
destination simultaneously.

## Machine-readable report

Use `--format json` for automation:

```console
gambit sync \
  --lichess-user diegoglozano \
  --output ./diegoglozano-games \
  --format json
```

The report includes `received`, `created`, `updated`, and `unchanged` counts;
the number of unfinished games refreshed and still tracked; and the committed
cursor. `received` counts PGN records received over the network, so an
overlapped or individually refreshed game may be included even when no local
file changes. With `--database`, JSON adds a nested `database` object containing
the same build/update report as `gambit index --format json`; human output adds
the Index report after the Sync report.

Exit status 0 means the cursor and files were committed successfully. Invalid
arguments exit 2. Filesystem, state, Lichess, parsing, and report failures exit
3. A network, parsing, or game-storage failure before the commit leaves the
previous cursor unchanged and is safe to retry. Database maintenance and report
output happen after that commit, so either can return 3 even though the new
cursor was saved.
