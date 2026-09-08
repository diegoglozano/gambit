fn main() {
    // Packaging diagnostic: no library access, UI, or user-facing chess claims.
    if std::env::args().any(|arg| arg == "--engine-smoke-test") {
        let result = std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|exe| {
                let engine = exe
                    .parent()
                    .ok_or("missing executable directory")?
                    .join("gambit-stockfish");
                gambit_engine::analyze(
                    &engine,
                    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                    100_000,
                    &gambit_engine::Cancellation::default(),
                    std::time::Duration::from_secs(30),
                )
                .map_err(|e| e.to_string())
            });
        match result {
            Ok(analysis) => println!("{analysis:?}"),
            Err(error) => {
                eprintln!("Local engine smoke test failed: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    gambit_desktop::run();
}
