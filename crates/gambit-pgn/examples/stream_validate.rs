use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::process::ExitCode;
use std::time::Instant;

use gambit_pgn::{Event, GameReader, Parser};

#[derive(Default)]
struct Statistics {
    events: u64,
    games: u64,
    tags: u64,
    move_numbers: u64,
    san_tokens: u64,
    nags: u64,
    comments: u64,
    variations: u64,
    outcomes: u64,
}

impl Statistics {
    fn observe(&mut self, event: Event<'_>) {
        self.events += 1;
        match event {
            Event::GameEnd { .. } => self.games += 1,
            Event::Tag(_) => self.tags += 1,
            Event::MoveNumber { .. } => self.move_numbers += 1,
            Event::San(_) => self.san_tokens += 1,
            Event::Nag(_) => self.nags += 1,
            Event::Comment(_) => self.comments += 1,
            Event::VariationStart(_) => self.variations += 1,
            Event::Outcome { .. } => self.outcomes += 1,
            Event::GameStart { .. } | Event::MovetextStart { .. } | Event::VariationEnd(_) => {}
        }
    }
}

fn main() -> ExitCode {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!(
            "usage: cargo run --release -p gambit-pgn --example stream-validate -- FILE.pgn|-"
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
    let mut reader = GameReader::new(input);
    let mut framed_games = 0_u64;
    let mut statistics = Statistics::default();
    loop {
        let game = match reader.read_game() {
            Ok(Some(game)) => game,
            Ok(None) => break,
            Err(error) => {
                eprintln!("after {framed_games} game(s): {error}");
                return ExitCode::FAILURE;
            }
        };
        framed_games += 1;
        for event in Parser::new(game) {
            match event {
                Ok(event) => statistics.observe(event),
                Err(error) => {
                    eprintln!("in framed game {framed_games}: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    let bytes = reader.bytes_read();
    let elapsed = started.elapsed();
    #[allow(clippy::cast_precision_loss)] // Human-readable measurement output is approximate.
    let mib = bytes as f64 / (1024.0 * 1024.0);

    println!("source: {source}");
    println!("bytes: {bytes}");
    println!("events: {}", statistics.events);
    println!("framed games: {framed_games}");
    println!("parsed games: {}", statistics.games);
    println!("tags: {}", statistics.tags);
    println!("move numbers: {}", statistics.move_numbers);
    println!("SAN tokens: {}", statistics.san_tokens);
    println!("NAGs: {}", statistics.nags);
    println!("comments: {}", statistics.comments);
    println!("variations: {}", statistics.variations);
    println!("outcomes: {}", statistics.outcomes);
    println!("elapsed: {:.3}s", elapsed.as_secs_f64());
    println!(
        "end-to-end throughput: {:.2} MiB/s",
        mib / elapsed.as_secs_f64()
    );
    ExitCode::SUCCESS
}
