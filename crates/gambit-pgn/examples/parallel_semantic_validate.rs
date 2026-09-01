use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use gambit_pgn::{FrameError, GameReader, Parser};

mod support;

use support::{SemanticError, Validator};

const DEFAULT_BATCH_MIB: usize = 1;

#[derive(Debug)]
struct Batch {
    sequence: u64,
    first_game: u64,
    base_offset: u64,
    bytes: Vec<u8>,
    game_ends: Vec<usize>,
}

#[derive(Debug)]
struct BatchResult {
    sequence: u64,
    games: u64,
    moves: u64,
    error: Option<ValidationError>,
}

#[derive(Debug)]
enum ValidationError {
    Parse {
        game: u64,
        offset: u64,
        detail: String,
    },
    Semantic(SemanticError),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse {
                game,
                offset,
                detail,
            } => write!(f, "game {game}, byte {offset}: {detail}"),
            Self::Semantic(error) => write!(
                f,
                "game {}, ply {}, byte {}: {} {}: {}",
                error.game,
                error.ply,
                error.offset,
                error.kind,
                String::from_utf8_lossy(&error.context),
                error.detail
            ),
        }
    }
}

#[derive(Debug)]
enum PipelineError {
    Frame(FrameError),
    WorkersUnavailable,
    WorkerPanicked,
    Validation(ValidationError),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(error) => error.fmt(f),
            Self::WorkersUnavailable => f.write_str("all semantic workers stopped unexpectedly"),
            Self::WorkerPanicked => f.write_str("a semantic worker panicked"),
            Self::Validation(error) => error.fmt(f),
        }
    }
}

impl Error for PipelineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Frame(error) => Some(error),
            Self::WorkersUnavailable | Self::WorkerPanicked | Self::Validation(_) => None,
        }
    }
}

impl From<FrameError> for PipelineError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Summary {
    bytes: u64,
    batches: u64,
    games: u64,
    moves: u64,
}

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1);
    let Some(path) = arguments.next() else {
        print_usage();
        return ExitCode::FAILURE;
    };
    let default_workers = thread::available_parallelism().map_or(1, usize::from);
    let workers = match positive_usize(arguments.next(), default_workers, "worker count") {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let batch_mib = match positive_usize(arguments.next(), DEFAULT_BATCH_MIB, "batch MiB") {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    if arguments.next().is_some() {
        eprintln!("too many arguments");
        print_usage();
        return ExitCode::FAILURE;
    }
    let Some(batch_bytes) = batch_mib.checked_mul(1024 * 1024) else {
        eprintln!("batch size is too large");
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
    let summary = match run_parallel(input, workers, batch_bytes) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let elapsed = started.elapsed();
    #[allow(clippy::cast_precision_loss)]
    let mib = summary.bytes as f64 / (1024.0 * 1024.0);
    #[allow(clippy::cast_precision_loss)]
    let million_moves = summary.moves as f64 / 1_000_000.0;

    println!("source: {source}");
    println!("workers: {workers}");
    println!("batch target: {batch_mib} MiB");
    println!("batches: {}", summary.batches);
    println!("bytes: {}", summary.bytes);
    println!("games: {}", summary.games);
    println!("legal SAN moves: {}", summary.moves);
    println!("elapsed: {:.3}s", elapsed.as_secs_f64());
    println!("throughput: {:.2} MiB/s", mib / elapsed.as_secs_f64());
    println!(
        "move rate: {:.2} million moves/s",
        million_moves / elapsed.as_secs_f64()
    );
    ExitCode::SUCCESS
}

fn print_usage() {
    eprintln!(
        "usage: cargo run --release -p gambit-pgn --example parallel-semantic-validate -- FILE.pgn|- [WORKERS] [BATCH_MIB]"
    );
}

fn positive_usize(argument: Option<OsString>, default: usize, name: &str) -> Result<usize, String> {
    let Some(argument) = argument else {
        return Ok(default);
    };
    let value = argument
        .to_str()
        .ok_or_else(|| format!("{name} must be valid UTF-8"))?
        .parse::<usize>()
        .map_err(|error| format!("invalid {name}: {error}"))?;
    if value == 0 {
        Err(format!("{name} must be greater than zero"))
    } else {
        Ok(value)
    }
}

fn run_parallel<R: Read>(
    input: R,
    workers: usize,
    batch_bytes: usize,
) -> Result<Summary, PipelineError> {
    assert!(workers > 0, "worker count must be positive");
    assert!(batch_bytes > 0, "batch size must be positive");

    let queue_capacity = workers.saturating_mul(2).max(1);
    let (job_sender, job_receiver) = mpsc::sync_channel(queue_capacity);
    let job_receiver = Arc::new(Mutex::new(job_receiver));
    let (result_sender, result_receiver) = mpsc::channel();
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let receiver = Arc::clone(&job_receiver);
            let sender = result_sender.clone();
            thread::spawn(move || worker_loop(&receiver, &sender))
        })
        .collect();
    drop(job_receiver);
    drop(result_sender);

    let mut reader = GameReader::new(input);
    let production = produce_batches(&mut reader, &job_sender, batch_bytes);
    drop(job_sender);

    let mut worker_panicked = false;
    for handle in handles {
        if handle.join().is_err() {
            worker_panicked = true;
        }
    }
    if worker_panicked {
        return Err(PipelineError::WorkerPanicked);
    }
    let batches = production?;

    let mut results: Vec<_> = result_receiver.into_iter().collect();
    results.sort_unstable_by_key(|result| result.sequence);
    if let Some(index) = results.iter().position(|result| result.error.is_some()) {
        let error = results
            .swap_remove(index)
            .error
            .expect("result was selected because it contains an error");
        return Err(PipelineError::Validation(error));
    }

    let games = results.iter().map(|result| result.games).sum();
    let moves = results.iter().map(|result| result.moves).sum();
    Ok(Summary {
        bytes: reader.bytes_read(),
        batches,
        games,
        moves,
    })
}

fn produce_batches<R: Read>(
    reader: &mut GameReader<R>,
    sender: &SyncSender<Batch>,
    target_bytes: usize,
) -> Result<u64, PipelineError> {
    let mut sequence = 0_u64;
    let mut game_number = 1_u64;
    let mut next_offset = 0_u64;
    let mut first_game = game_number;
    let mut base_offset = next_offset;
    let mut bytes = Vec::with_capacity(target_bytes);
    let mut game_ends = Vec::new();

    while let Some(game) = reader.read_game()? {
        if !game_ends.is_empty() && bytes.len().saturating_add(game.len()) > target_bytes {
            send_batch(
                sender,
                Batch {
                    sequence,
                    first_game,
                    base_offset,
                    bytes,
                    game_ends,
                },
            )?;
            sequence += 1;
            first_game = game_number;
            base_offset = next_offset;
            bytes = Vec::with_capacity(target_bytes);
            game_ends = Vec::new();
        }
        bytes.extend_from_slice(game);
        game_ends.push(bytes.len());
        game_number += 1;
        next_offset = next_offset
            .checked_add(u64::try_from(game.len()).expect("game length fits in u64"))
            .expect("PGN stream offset overflowed u64");
    }
    if !game_ends.is_empty() {
        send_batch(
            sender,
            Batch {
                sequence,
                first_game,
                base_offset,
                bytes,
                game_ends,
            },
        )?;
        sequence += 1;
    }
    Ok(sequence)
}

fn send_batch(sender: &SyncSender<Batch>, batch: Batch) -> Result<(), PipelineError> {
    sender
        .send(batch)
        .map_err(|_| PipelineError::WorkersUnavailable)
}

fn worker_loop(receiver: &Arc<Mutex<Receiver<Batch>>>, sender: &mpsc::Sender<BatchResult>) {
    loop {
        let batch = {
            let guard = receiver.lock().expect("job receiver mutex poisoned");
            guard.recv()
        };
        let Ok(batch) = batch else {
            break;
        };
        if sender.send(validate_batch(&batch)).is_err() {
            break;
        }
    }
}

fn validate_batch(batch: &Batch) -> BatchResult {
    let mut start = 0_usize;
    let mut games = 0_u64;
    let mut moves = 0_u64;
    for (index, &end) in batch.game_ends.iter().enumerate() {
        let game_number =
            batch.first_game + u64::try_from(index).expect("batch game index fits in u64");
        let game_offset =
            batch.base_offset + u64::try_from(start).expect("batch byte offset fits in u64");
        let mut validator = Validator::with_origin(game_number - 1, game_offset);
        for event in Parser::new(&batch.bytes[start..end]) {
            match event {
                Ok(event) => validator.observe(event),
                Err(error) => {
                    return BatchResult {
                        sequence: batch.sequence,
                        games,
                        moves,
                        error: Some(ValidationError::Parse {
                            game: game_number,
                            offset: game_offset
                                + u64::try_from(error.offset)
                                    .expect("parse error offset fits in u64"),
                            detail: error.to_string(),
                        }),
                    };
                }
            }
            if let Some(error) = validator.error.take() {
                return BatchResult {
                    sequence: batch.sequence,
                    games,
                    moves,
                    error: Some(ValidationError::Semantic(error)),
                };
            }
        }
        debug_assert_eq!(validator.games, game_number);
        games += 1;
        moves += validator.moves;
        start = end;
    }
    BatchResult {
        sequence: batch.sequence,
        games,
        moves,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn validates_packed_batches_in_parallel() {
        let input = b"[Event \"A\"]\n\n1. e4 e5 2. Nf3 *\n[Event \"B\"]\n\n1. d4 d5 *\n";
        let summary = run_parallel(Cursor::new(input), 2, 24).unwrap();

        assert_eq!(summary.bytes, input.len() as u64);
        assert_eq!(summary.games, 2);
        assert_eq!(summary.moves, 5);
        assert_eq!(summary.batches, 2);
    }

    #[test]
    fn reports_the_earliest_error_independent_of_worker_order() {
        let input = b"1. e4 *\n1. e5 *\n1. d5 *\n";
        let error = run_parallel(Cursor::new(input), 2, 1).unwrap_err();

        match error {
            PipelineError::Validation(ValidationError::Semantic(error)) => {
                assert_eq!(error.game, 2);
                assert_eq!(error.ply, 1);
                assert_eq!(error.context, b"e5");
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
