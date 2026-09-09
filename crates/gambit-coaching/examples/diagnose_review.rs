//! Local diagnostic only; detailed evidence must not be uploaded from private PGNs.
use gambit_coaching::{DEFAULT_NODES, ReviewGame, diagnose};
use gambit_engine::{Cancellation, analyze_game};
use gambit_pgn::{GameReader, GameReaderOptions};
use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: diagnose_review ENGINE PGN PLAYER SHARED_PLY".into());
    }
    let engine = PathBuf::from(&args[0]);
    let player = args[2].to_str().ok_or("invalid player")?;
    let shared_ply: usize = args[3].to_str().ok_or("invalid shared ply")?.parse()?;
    let mut reader = GameReader::with_options(
        File::open(&args[1])?,
        GameReaderOptions {
            max_game_bytes: ReviewGame::MAX_PGN_BYTES,
            ..GameReaderOptions::default()
        },
    );
    let mut games = Vec::new();
    while let Some(pgn) = reader.read_game()? {
        if games.len() == 6 {
            return Err("review sets contain at most six games".into());
        }
        games.push(ReviewGame::parse(pgn, player)?);
    }
    if games.is_empty() {
        return Err("no games".into());
    }
    let cancel = Cancellation::default();
    for (index, game) in games.iter().enumerate() {
        let started = Instant::now();
        let diagnosis = diagnose(game, shared_ply, DEFAULT_NODES, &cancel, |position| {
            analyze_game(
                &engine,
                position,
                DEFAULT_NODES,
                &cancel,
                Duration::from_secs(30),
            )
        })?;
        let report = serde_json::json!({"game": index + 1, "elapsed_ms": started.elapsed().as_millis(), "diagnosis": diagnosis});
        let mut output = io::stdout().lock();
        serde_json::to_writer(&mut output, &report)?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}
