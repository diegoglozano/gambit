# Gambit Desktop

Gambit Desktop is a local-first graphical client for Gambit databases. It is an
early product release for macOS.

## Install on macOS

Download the universal DMG from the
[latest GitHub Release](https://github.com/diegoglozano/gambit/releases/latest/download/gambit-desktop-universal-apple-darwin.dmg),
open it, and drag **Gambit** into **Applications**. The same installer supports
Apple Silicon and Intel Macs. A
[SHA-256 checksum](https://github.com/diegoglozano/gambit/releases/latest/download/gambit-desktop-universal-apple-darwin.dmg.sha256)
is published beside every installer.

Until Apple Developer credentials are configured for the project, release DMGs
use an ad-hoc signature and macOS may require you to right-click Gambit and
choose **Open** on first launch. Once the release secrets described below are
present, the same pipeline applies a Developer ID signature and submits the app
to Apple for notarization automatically.

## Player workflow

The first vertical slice supports three ways to enter the library:

- Enter a public Lichess username and an optional first-sync date. Gambit stores
  the PGNs and resulting database in the operating system's application-data
  directory. During the first import, live progress reports the number of games
  downloaded and the latest history date reached before local indexing begins.
  **Sync now** later fetches and indexes only new or changed games. For a faster
  initial export, expand **Speed up with a Lichess token** and provide a
  [personal access token](https://lichess.org/account/oauth/token). The token is
  sent directly to Lichess for that sync and is not stored.
- Choose one or more `.pgn` or `.pgn.zst` files, then save and immediately open
  a new `.gambit` database. From an open library, **Add / update PGN** adds new
  sources, skips unchanged sources, and replaces games from changed sources.
- Choose an existing `.gambit` file from the native file picker.

Once loaded, the app shows corpus, result, storage, and date totals; can verify
database integrity; and pages through games. Search uses the same indexed
filters as the CLI: player, opponent, player color, player-relative result, date
bounds, player rating bounds, and a complete six-field position FEN. Results
update automatically as filters change, with a short debounce while typing.
Games can be sorted by date, rating, result, or White player, and matching games
can be exported as PGN.

**Explore** summarizes common opening lines, results over the latest 12 active
months, frequent opponents, and recurring positions for the Player filter (or
the managed Lichess user by default). When a player has at least four completed
games in one of the common opening lines, Explore highlights the lowest-scoring
line that includes a loss as a review candidate. It shows the supporting sample
and opens a representative loss at the relevant ply. Other opening and position
cards open a representative game at the relevant ply.

The longer-term player workflow and the evidence rules for recommendations are
described in [Player experience direction](player-experience.md).

Managed Lichess libraries open on **Today**, the learning home. Gambit prepares
up to six completed games from the latest 24 player games, independently of
opening results, keeping up to three unfinished lessons from the last entered
sample when available. Valid cached evidence loads before new engine searches.
Each local pass uses one engine thread, the existing fixed node budget and a
three-minute elapsed-time limit. A partially analyzed game stays incomplete;
it is never reported as a completed no-result diagnosis.

Today offers one supported mistake with a position preview and source game.
**Learn this move** enters the lesson directly; the player does not select games,
build a queue or start diagnosis. Concrete recurrence within this small sample,
consequence, recency and evidence bounds guide selection. A single game is not
called a habit. Games stay available while evidence arrives progressively;
when no lesson is ready, **Browse my games** offers a useful alternative.
**Local analysis and sync** contains pause/resume and quiet sync context.
Interrupted preparation does not restart silently on relaunch.

Successful attempts and reveal save progress automatically. **Continue** moves
to another supported exercise when one is available, or returns to Today.
**Exit** returns immediately with progress preserved. No-result or unsupported
games are not inserted between lessons. Entering a ready lesson pauses its
preparation pass so the player can practice without competing searches.

The local database is available immediately while Gambit checks Lichess in the
background, no more than once every 15 minutes. **Sync now** checks explicitly.
The latest successful check and new-game record survive relaunch; a failed
background check preserves local games and lessons. Background arrivals retain
the current lesson and the Library's game, move and board orientation.

Explore's opening recommendation remains an investigation rather than an
error claim, and retains its manual review-set tools:

Starting the recommendation opens a review set containing up to six recent
losses that reached the highlighted opening position. Review mode shows only
those queued games and makes its result, opening position, and “latest six of
the matching losses” scope visible. It keeps the position aligned across games
and provides previous, next, reviewed, defer, open-on-Lichess, and exit
controls. Reviewed and deferred games remain visible in the set with their
status, while the next untouched game opens automatically. Progress is stored
on the Mac for that library, appears on Today, and resumes after relaunching the
app. When every game has been reviewed or deferred, a session summary returns
the player to Today (or Explore for an unmanaged library). Exiting early still
restores the player's previous filters, sort, page, selection, and originating
view without changing the database.

**Analyze review set** finds supported turning points locally with Stockfish.
Practice shows the position before your decision and which color you play.
Click a piece to see its legal destinations, then click a destination or drag
there. Completing a legal move checks it automatically; selecting a piece alone
is not an attempt. For promotion, choose the new piece after choosing a
destination. Coordinate entry remains available with Enter or **Play move**. **See the move I played** previews the
original decision without recording an attempt. Move descriptions name the
piece, and arrows show its path. **Show me a better move** reveals the answer;
use **See next move**, **Back**, and **Starting position** to follow it on the
board. Successful attempts and revealed answers save completion automatically,
with independent solves kept separate from help. The board stays available for
inspection until **Continue**; answer playback never submits an attempt. If a
check fails or is cancelled, play the move again to retry. Completed practice
and positions saved for later remain on this Mac.

The app replays a selected standard-chess mainline. The board automatically
faces the selected player and can be flipped manually. Arrow keys and board
controls move through the game without moving the surrounding window. Raw PGN
remains available for inspection. Gambit remembers up to 12 recent libraries;
**Libraries** switches among them, and the active library reopens automatically
on the next launch.

## Updates

Gambit checks for updates shortly after launch without interrupting normal use.
When a newer release exists, the app shows its release notes and asks before it
downloads or installs anything. **Check for updates** in the sidebar runs the
same check manually. After installation, Gambit restarts into the new version.

Updater archives are cryptographically signed independently from Apple's app
signature. The app rejects an archive that does not match the updater public
key embedded in the installed version. Because v0.9.0 did not include this
updater, it cannot discover the first updater-enabled release; install that DMG
manually once, and later releases can update from inside the app.

## Privacy

The app reads databases locally and does not upload them to Gambit. Game sync
communicates directly with the Lichess API. An optional personal access token is
kept only for the duration of one sync; Gambit does not write it to disk or to
the saved session. To resume and switch libraries, Gambit stores recent local
database paths and, when applicable, public Lichess usernames in the
application-data directory. Managed-library entries also store the counts and
timestamp from their latest successful check so Today remains useful offline.
Review progress is stored in the same local session file and is scoped to its
source library.

## Architecture

The `gambit` Rust package now exposes its indexing, query, sync, and structured
library modules as a reusable library. Tauri commands call those APIs in the
desktop process. The CLI uses the same modules, so the graphical client does not
execute a subprocess or depend on human-readable terminal output.

The static frontend is embedded in the native application. It has no remote
runtime dependencies and uses a restrictive content security policy.

## Run locally

On macOS, install the Tauri prerequisites and run:

```console
cargo run --manifest-path apps/gambit-desktop/src-tauri/Cargo.toml
```

To build a local app bundle with the pinned Tauri CLI:

```console
cd apps/gambit-desktop
npx --yes @tauri-apps/cli@2.11.4 build --debug --bundles app
```

Build the universal release DMG and its SHA-256 checksum with:

```console
./scripts/build-desktop-dmg.sh 0.17.0
```

Pull requests exercise that universal packaging path. After the main Release
workflow publishes a version tag, the Desktop release workflow builds the DMG
and signed updater files from the same commit and attaches them to the existing
GitHub Release. A manual workflow dispatch can rebuild an existing tag.

## Frontend checks

Run the controller tests and browser regression checks from `apps/gambit-desktop`:

```console
npm ci
npm test
npx playwright install chromium webkit
npm run test:browser
```

The browser suite checks visual practice in Chromium and WebKit, including
answer visibility, SVG geometry, move previews, and layout at four window sizes.
CI runs both browsers on macOS and saves traces and screenshots on failures.

## Signing and notarization

Without Apple credentials, the release workflow uses Tauri's ad-hoc identity.
Configure all of these GitHub Actions secrets to enable Developer ID signing
and notarization:

- `APPLE_CERTIFICATE`: base64-encoded Developer ID Application `.p12`
- `APPLE_CERTIFICATE_PASSWORD`: password used when exporting the `.p12`
- `KEYCHAIN_PASSWORD`: temporary CI keychain password
- `APPLE_ID`: Apple Developer account email
- `APPLE_PASSWORD`: app-specific Apple ID password
- `APPLE_TEAM_ID`: Apple Developer Team ID

The workflow uses ad-hoc signing until the complete secret set is present.

Updater signing is mandatory for every desktop release and uses two additional
secrets:

- `TAURI_SIGNING_PRIVATE_KEY`: encrypted Tauri updater private key
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: password for that key

The updater private key must be backed up permanently. Existing installations
trust its matching embedded public key and cannot migrate automatically if the
private key is lost. Windows packaging remains a later release milestone.
