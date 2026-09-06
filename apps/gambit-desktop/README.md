# Gambit Desktop

Gambit Desktop is the local-first graphical client for `.gambit` databases. It
uses the same Rust application services as the CLI and never uploads a player's
database to Gambit.

Run the development app from macOS with:

```console
cargo run --manifest-path apps/gambit-desktop/src-tauri/Cargo.toml
```

The first vertical slice can synchronize a public Lichess account into the
application data directory, build a new `.gambit` database from PGN files, open
an existing database, page through its games, and replay standard-chess
mainlines on an interactive board. Lichess sync reports streaming game/date
progress, and the board faces the selected player with a manual flip control.
The last library is reopened automatically on the next launch. Installed builds
check GitHub Releases for signed updates and ask before downloading, installing,
and restarting. Because v0.9.0 predates the updater, users must install the
first updater-enabled release manually.

Build a universal macOS DMG from the repository root with:

```console
./scripts/build-desktop-dmg.sh 0.11.0
```

Release builds require `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. They produce the DMG plus a signed
universal `.app.tar.gz`, its signature, and `latest.json` for Tauri's updater.
