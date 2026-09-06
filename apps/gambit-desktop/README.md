# Gambit Desktop

Gambit Desktop is the local-first graphical client for `.gambit` databases. It
uses the same Rust application services as the CLI and never uploads a player's
database to Gambit.

Run the development app from macOS with:

```console
cargo run --manifest-path apps/gambit-desktop/src-tauri/Cargo.toml
```

The app can synchronize a public Lichess account into the application data
directory, build or incrementally update a `.gambit` database from multiple PGN
files, switch among recent databases, and replay standard-chess mainlines on an
interactive board. Its indexed filters cover player, opponent, color, result,
date, rating, and position; they update results live, and matching games can be
sorted or exported to PGN. Explore summarizes opening lines, results over time,
frequent opponents, and recurring positions, with representative-game
drill-downs. Database summaries expose corpus coverage and an explicit integrity
check. Lichess sync reports streaming game/date progress, and the board faces
the selected player with a manual flip control. The active library is reopened
automatically on the next launch. Installed builds check GitHub Releases for
signed updates and ask before downloading, installing, and restarting. Because
v0.9.0 predates the updater, users must install the first updater-enabled release
manually.

Build a universal macOS DMG from the repository root with:

```console
./scripts/build-desktop-dmg.sh 0.12.0
```

Release builds require `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. They produce the DMG plus a signed
universal `.app.tar.gz`, its signature, and `latest.json` for Tauri's updater.
