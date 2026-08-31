use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::Instant;

use gambit_pgn::{Event, Parser};

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
        eprintln!("usage: cargo run --release -p gambit-pgn --example validate -- FILE.pgn");
        return ExitCode::FAILURE;
    };

    let load_started = Instant::now();
    let input = match fs::read(&path) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("failed to read {}: {error}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    let load_elapsed = load_started.elapsed();

    let parse_started = Instant::now();
    let mut statistics = Statistics::default();
    for event in Parser::new(&input) {
        match event {
            Ok(event) => statistics.observe(event),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let parse_elapsed = parse_started.elapsed();
    #[allow(clippy::cast_precision_loss)] // Human-readable measurement output is approximate.
    let mib = input.len() as f64 / (1024.0 * 1024.0);

    println!("file: {}", path.to_string_lossy());
    println!("bytes: {}", input.len());
    println!("events: {}", statistics.events);
    println!("games: {}", statistics.games);
    println!("tags: {}", statistics.tags);
    println!("move numbers: {}", statistics.move_numbers);
    println!("SAN tokens: {}", statistics.san_tokens);
    println!("NAGs: {}", statistics.nags);
    println!("comments: {}", statistics.comments);
    println!("variations: {}", statistics.variations);
    println!("outcomes: {}", statistics.outcomes);
    println!("load time: {:.3}s", load_elapsed.as_secs_f64());
    println!("parse time: {:.3}s", parse_elapsed.as_secs_f64());
    println!(
        "parse throughput: {:.2} MiB/s",
        mib / parse_elapsed.as_secs_f64()
    );
    ExitCode::SUCCESS
}
