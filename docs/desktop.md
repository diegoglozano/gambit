# Gambit Desktop preview

Gambit Desktop is a local-first graphical client for Gambit databases. It is an
early development preview rather than part of the signed binary release.

## Player workflow

The first vertical slice supports two ways to enter the library:

- Enter a public Lichess username and an optional first-sync date. Gambit stores
  the PGNs and resulting database in the operating system's application-data
  directory. **Sync now** later fetches and indexes only new or changed games.
- Choose an existing `.gambit` file from the native file picker.

Once loaded, the app shows corpus totals and date coverage, pages through games
newest-first, filters by an exact player name case-insensitively, and replays a
selected standard-chess mainline. Arrow keys and board controls move through
the game. Raw PGN remains available for inspection.

## Privacy

The app reads databases locally and does not upload them to Gambit. Public game
sync communicates directly with the Lichess API. Authentication and OS
credential storage are intentionally deferred until the public-game experience
is stable.

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

To build a macOS preview bundle with the pinned Tauri CLI:

```console
cd apps/gambit-desktop
npx --yes @tauri-apps/cli@2.11.4 build --debug --bundles app
```

Pull requests build an unsigned `gambit-desktop-macos-preview` artifact. Code
signing, notarization, automatic updates, Windows packaging, and public desktop
distribution remain later release milestones.
