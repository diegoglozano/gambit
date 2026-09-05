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

The first vertical slice supports two ways to enter the library:

- Enter a public Lichess username and an optional first-sync date. Gambit stores
  the PGNs and resulting database in the operating system's application-data
  directory. **Sync now** later fetches and indexes only new or changed games.
- Choose an existing `.gambit` file from the native file picker.

Once loaded, the app shows corpus totals and date coverage, pages through games
newest-first, filters by an exact player name case-insensitively, and replays a
selected standard-chess mainline. Arrow keys and board controls move through
the game. Raw PGN remains available for inspection. Gambit remembers the last
library and reopens it automatically on the next launch.

## Privacy

The app reads databases locally and does not upload them to Gambit. Public game
sync communicates directly with the Lichess API. Authentication and OS
credential storage are intentionally deferred until the public-game experience
is stable. To resume the previous library, Gambit stores its local database path
and, when applicable, public Lichess username in the application-data directory.

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
./scripts/build-desktop-dmg.sh 0.8.0
```

Pull requests exercise that universal packaging path. After the main Release
workflow publishes a version tag, the Desktop release workflow builds the DMG
from the same commit and attaches it to the existing GitHub Release. A manual
workflow dispatch can rebuild an existing tag.

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

The workflow rejects a partially configured secret set. Automatic updates and
Windows packaging remain later release milestones.
