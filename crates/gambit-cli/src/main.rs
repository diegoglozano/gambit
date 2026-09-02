use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::process::ExitCode;
use std::time::Instant;

use gambit_chess::Position;
use gambit_pgn::{Event, IncrementalParser};

const USAGE: &str = "Usage: gambit <FILE.pgn|->\n\nValidates PGN syntax and chess semantics. Use - to read from standard input.";

fn main() -> ExitCode {
    match parse_arguments(env::args_os().skip(1)) {
        Ok(Action::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Action::Version) => {
            println!("gambit {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Action::Validate(path)) => validate(&path),
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

enum Action {
    Help,
    Version,
    Validate(OsString),
}

fn parse_arguments(mut arguments: impl Iterator<Item = OsString>) -> Result<Action, String> {
    let Some(argument) = arguments.next() else {
        return Err(String::from("missing PGN file"));
    };
    if arguments.next().is_some() {
        return Err(String::from("too many arguments"));
    }
    match argument.to_str() {
        Some("-h" | "--help") => Ok(Action::Help),
        Some("-V" | "--version") => Ok(Action::Version),
        Some(value) if value.starts_with('-') && value != "-" => {
            Err(format!("unknown option: {value}"))
        }
        _ => Ok(Action::Validate(argument)),
    }
}

fn validate(path: &OsString) -> ExitCode {
    let (input, source): (Box<dyn Read>, String) = if path == "-" {
        (Box::new(io::stdin().lock()), String::from("stdin"))
    } else {
        match File::open(path) {
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

#[derive(Debug)]
struct SemanticError {
    game: u64,
    ply: u64,
    offset: u64,
    kind: &'static str,
    context: Vec<u8>,
    detail: String,
}

struct Validator {
    position: Position,
    before_last_move: Position,
    variation_stack: Vec<(Position, Position)>,
    fen: Option<(Vec<u8>, usize)>,
    games: u64,
    moves: u64,
    game_ply: u64,
    error: Option<SemanticError>,
}

impl Default for Validator {
    fn default() -> Self {
        Self {
            position: Position::initial(),
            before_last_move: Position::initial(),
            variation_stack: Vec::new(),
            fen: None,
            games: 0,
            moves: 0,
            game_ply: 0,
            error: None,
        }
    }
}

impl Validator {
    fn observe(&mut self, event: Event<'_>) {
        if self.error.is_some() {
            return;
        }
        match event {
            Event::GameStart { .. } => {
                self.position = Position::initial();
                self.before_last_move = self.position;
                self.variation_stack.clear();
                self.fen = None;
                self.game_ply = 0;
            }
            Event::Tag(tag) if tag.name() == b"FEN" => {
                self.fen = Some((tag.raw_value().to_vec(), tag.span().start));
            }
            Event::MovetextStart { .. } => {
                if let Some((fen, offset)) = &self.fen {
                    match Position::from_fen(fen) {
                        Ok(position) => self.position = position,
                        Err(error) => {
                            self.error = Some(SemanticError {
                                game: self.games + 1,
                                ply: 0,
                                offset: u64::try_from(*offset).expect("span offset fits in u64"),
                                kind: "FEN",
                                context: fen.clone(),
                                detail: error.to_string(),
                            });
                            return;
                        }
                    }
                }
                self.before_last_move = self.position;
            }
            Event::San(token) => {
                self.before_last_move = self.position;
                self.game_ply += 1;
                match self.position.play_san(token.as_bytes()) {
                    Ok(_) => self.moves += 1,
                    Err(error) => {
                        self.error = Some(SemanticError {
                            game: self.games + 1,
                            ply: self.game_ply,
                            offset: u64::try_from(token.span().start)
                                .expect("span offset fits in u64"),
                            kind: "SAN",
                            context: token.as_bytes().to_vec(),
                            detail: error.to_string(),
                        });
                    }
                }
            }
            Event::VariationStart(_) => {
                self.variation_stack
                    .push((self.position, self.before_last_move));
                let branch_base = self.before_last_move;
                self.position = branch_base;
                self.before_last_move = branch_base;
            }
            Event::VariationEnd(_) => {
                if let Some((position, before_last_move)) = self.variation_stack.pop() {
                    self.position = position;
                    self.before_last_move = before_last_move;
                }
            }
            Event::GameEnd { .. } => self.games += 1,
            Event::Tag(_)
            | Event::MoveNumber { .. }
            | Event::Nag(_)
            | Event::Comment(_)
            | Event::Outcome { .. } => {}
        }
    }
}
