# Gambit Desktop

Gambit Desktop is the local-first graphical client for `.gambit` databases. It
uses the same Rust application services as the CLI and never uploads a player's
database to Gambit.

Run the development app from macOS with:

```console
cargo run --manifest-path apps/gambit-desktop/src-tauri/Cargo.toml
```

The first vertical slice can synchronize a public Lichess account into the
application data directory, open an existing `.gambit` file, page through its
games, and replay standard-chess mainlines on an interactive board. The last
library is reopened automatically on the next launch.
