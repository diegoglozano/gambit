use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::process::ExitCode;
use std::time::Instant;

use gambit_pgn::IncrementalParser;

mod support;

use support::Validator;

fn main() -> ExitCode {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!(
            "usage: cargo run --release -p gambit-pgn --example semantic-validate -- FILE.pgn|-"
        );
        return ExitCode::FAILURE;
    };
    let (input, source): (Box<dyn Read>, String) = if path == "-" {
        (Box::new(io::stdin().lock()), String::from("stdin"))
    } else {
        match File::open(&path) {
            Ok(file) => (Box::new(file), path.to_string_lossy().into_owned()),
            Err(error) => {
                eprintln!("failed to open {}: {error}", path.to_string_lossy());
                return ExitCode::FAILURE;
            }
        }
    };

    let started = Instant::now();
    let mut validator = Validator::default();
    let mut parser = IncrementalParser::new(input);
    let stream = match parser.parse(|event| validator.observe(event)) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("after {} complete game(s): {error}", validator.games);
            return ExitCode::FAILURE;
        }
    };
    if let Some(error) = validator.error {
        eprintln!(
            "game {}, ply {}, byte {}: {} {}: {}",
            error.game,
            error.ply,
            error.offset,
            error.kind,
            String::from_utf8_lossy(&error.context),
            error.detail
        );
        return ExitCode::FAILURE;
    }

    let elapsed = started.elapsed();
    #[allow(clippy::cast_precision_loss)]
    let mib = stream.bytes_read as f64 / (1024.0 * 1024.0);
    #[allow(clippy::cast_precision_loss)]
    let million_moves = validator.moves as f64 / 1_000_000.0;
    println!("source: {source}");
    println!("bytes: {}", stream.bytes_read);
    println!("games: {}", validator.games);
    println!("legal SAN moves: {}", validator.moves);
    println!("elapsed: {:.3}s", elapsed.as_secs_f64());
    println!("throughput: {:.2} MiB/s", mib / elapsed.as_secs_f64());
    println!(
        "move rate: {:.2} million moves/s",
        million_moves / elapsed.as_secs_f64()
    );
    ExitCode::SUCCESS
}
