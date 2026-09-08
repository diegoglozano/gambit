//! Fixed-position spike, not a six-game workload or a final product budget.
use gambit_engine::{Cancellation, analyze};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args_os().nth(1).ok_or("usage: profile ENGINE")?);
    let positions = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    ];
    let total = Instant::now();
    for (i, fen) in positions.iter().enumerate() {
        let start = Instant::now();
        let result = analyze(
            &path,
            fen,
            100_000,
            &Cancellation::default(),
            Duration::from_secs(60),
        )?;
        println!(
            "position={} elapsed_ms={} result={result:?}",
            i + 1,
            start.elapsed().as_millis()
        );
    }
    println!("total_ms={}", total.elapsed().as_millis());
    Ok(())
}
