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
the managed Lichess user by default). Opening and position cards open a
representative game at the relevant ply.

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
application-data directory.

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
./scripts/build-desktop-dmg.sh 0.13.0
```

Pull requests exercise that universal packaging path. After the main Release
workflow publishes a version tag, the Desktop release workflow builds the DMG
and signed updater files from the same commit and attaches them to the existing
GitHub Release. A manual workflow dispatch can rebuild an existing tag.

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
