use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Instant;

use gambit_pgn::{FrameError, GameReader};

mod support;

use support::{GameValidationError, validate_game};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileRange {
    sequence: usize,
    start: u64,
    end: u64,
    first_game: u64,
    expected_games: u64,
}

#[derive(Debug)]
struct RangeResult {
    sequence: usize,
    bytes: u64,
    games: u64,
    moves: u64,
    error: Option<RangeError>,
}

#[derive(Debug)]
enum RangeError {
    Io {
        sequence: usize,
        source: io::Error,
    },
    Frame {
        sequence: usize,
        source: FrameError,
    },
    Validation(GameValidationError),
    GameCount {
        sequence: usize,
        expected: u64,
        actual: u64,
    },
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { sequence, source } => {
                write!(f, "file range {}: {source}", sequence + 1)
            }
            Self::Frame { sequence, source } => {
                write!(f, "file range {}: {source}", sequence + 1)
            }
            Self::Validation(error) => error.fmt(f),
            Self::GameCount {
                sequence,
                expected,
                actual,
            } => write!(
                f,
                "file range {} completed {actual} games instead of {expected}",
                sequence + 1
            ),
        }
    }
}

impl Error for RangeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Frame { source, .. } => Some(source),
            Self::Validation(_) | Self::GameCount { .. } => None,
        }
    }
}

#[derive(Debug)]
enum PipelineError {
    Io(io::Error),
    Frame(FrameError),
    WorkerPanicked,
    Range(RangeError),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Frame(error) => error.fmt(f),
            Self::WorkerPanicked => f.write_str("a file-range worker panicked"),
            Self::Range(error) => error.fmt(f),
        }
    }
}

impl Error for PipelineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::Range(error) => Some(error),
            Self::WorkerPanicked => None,
        }
    }
}

impl From<io::Error> for PipelineError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
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
    ranges: usize,
    games: u64,
    moves: u64,
}

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1);
    let Some(path) = arguments.next() else {
        print_usage();
        return ExitCode::FAILURE;
    };
    if path == "-" {
        eprintln!("partitioned validation requires a seekable file");
        return ExitCode::FAILURE;
    }
    let default_workers = thread::available_parallelism().map_or(1, usize::from);
    let workers = match positive_usize(arguments.next(), default_workers, "worker count") {
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
    let path = PathBuf::from(path);

    let total_started = Instant::now();
    let input_bytes = match path.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            eprintln!("failed to inspect {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let partition_started = Instant::now();
    let ranges = match File::open(&path)
        .map_err(PipelineError::from)
        .and_then(|file| discover_ranges(file, input_bytes, workers).map_err(PipelineError::from))
    {
        Ok(ranges) => ranges,
        Err(error) => {
            eprintln!("failed to partition {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let partition_elapsed = partition_started.elapsed();

    let validation_started = Instant::now();
    let summary = match validate_ranges(&path, &ranges) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let validation_elapsed = validation_started.elapsed();
    let total_elapsed = total_started.elapsed();

    #[allow(clippy::cast_precision_loss)]
    let mib = summary.bytes as f64 / (1024.0 * 1024.0);
    #[allow(clippy::cast_precision_loss)]
    let million_moves = summary.moves as f64 / 1_000_000.0;
    println!("source: {}", path.display());
    println!("workers requested: {workers}");
    println!("file ranges: {}", summary.ranges);
    println!("bytes: {}", summary.bytes);
    println!("games: {}", summary.games);
    println!("legal SAN moves: {}", summary.moves);
    println!("partition elapsed: {:.3}s", partition_elapsed.as_secs_f64());
    println!(
        "validation elapsed: {:.3}s",
        validation_elapsed.as_secs_f64()
    );
    println!("total elapsed: {:.3}s", total_elapsed.as_secs_f64());
    println!(
        "validation throughput: {:.2} MiB/s",
        mib / validation_elapsed.as_secs_f64()
    );
    println!(
        "end-to-end throughput: {:.2} MiB/s",
        mib / total_elapsed.as_secs_f64()
    );
    println!(
        "validation move rate: {:.2} million moves/s",
        million_moves / validation_elapsed.as_secs_f64()
    );
    ExitCode::SUCCESS
}

fn print_usage() {
    eprintln!(
        "usage: cargo run --release -p gambit-pgn --example partitioned-semantic-validate -- FILE.pgn [WORKERS]"
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

fn discover_ranges<R: Read>(
    input: R,
    input_bytes: u64,
    workers: usize,
) -> Result<Vec<FileRange>, FrameError> {
    assert!(workers > 0, "worker count must be positive");
    if input_bytes == 0 {
        return Ok(vec![FileRange {
            sequence: 0,
            start: 0,
            end: 0,
            first_game: 0,
            expected_games: 0,
        }]);
    }

    let mut reader = GameReader::new(input);
    let mut boundaries = vec![(0_u64, 0_u64)];
    let mut offset = 0_u64;
    let mut games = 0_u64;
    let mut cut_index = 1_usize;
    while let Some(game) = reader.read_game()? {
        offset += u64::try_from(game.len()).expect("game length fits in u64");
        games += 1;
        while cut_index < workers && offset >= split_offset(input_bytes, cut_index, workers) {
            if boundaries
                .last()
                .is_none_or(|(previous, _)| *previous != offset)
            {
                boundaries.push((offset, games));
            }
            cut_index += 1;
        }
    }
    if boundaries
        .last()
        .is_some_and(|(end, _)| *end == input_bytes)
    {
        if let Some((_, completed_games)) = boundaries.last_mut() {
            *completed_games = games;
        }
    } else {
        boundaries.push((input_bytes, games));
    }

    Ok(boundaries
        .windows(2)
        .enumerate()
        .map(|(sequence, pair)| FileRange {
            sequence,
            start: pair[0].0,
            end: pair[1].0,
            first_game: pair[0].1,
            expected_games: pair[1].1 - pair[0].1,
        })
        .collect())
}

fn split_offset(bytes: u64, index: usize, parts: usize) -> u64 {
    let index = u128::try_from(index).expect("partition index fits in u128");
    let parts = u128::try_from(parts).expect("partition count fits in u128");
    u64::try_from(u128::from(bytes) * index / parts).expect("partition offset fits in u64")
}

fn validate_ranges(path: &Path, ranges: &[FileRange]) -> Result<Summary, PipelineError> {
    let handles: Vec<_> = ranges
        .iter()
        .copied()
        .map(|range| {
            let path = path.to_owned();
            thread::spawn(move || validate_range(&path, range))
        })
        .collect();

    let mut worker_panicked = false;
    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.join() {
            Ok(result) => results.push(result),
            Err(_) => worker_panicked = true,
        }
    }
    if worker_panicked {
        return Err(PipelineError::WorkerPanicked);
    }
    results.sort_unstable_by_key(|result| result.sequence);
    if let Some(index) = results.iter().position(|result| result.error.is_some()) {
        let error = results
            .swap_remove(index)
            .error
            .expect("result was selected because it contains an error");
        return Err(PipelineError::Range(error));
    }

    Ok(Summary {
        bytes: results.iter().map(|result| result.bytes).sum(),
        ranges: results.len(),
        games: results.iter().map(|result| result.games).sum(),
        moves: results.iter().map(|result| result.moves).sum(),
    })
}

fn validate_range(path: &Path, range: FileRange) -> RangeResult {
    let file = match File::open(path) {
        Ok(mut file) => {
            if let Err(source) = file.seek(SeekFrom::Start(range.start)) {
                return failed_range(
                    range.sequence,
                    RangeError::Io {
                        sequence: range.sequence,
                        source,
                    },
                );
            }
            file
        }
        Err(source) => {
            return failed_range(
                range.sequence,
                RangeError::Io {
                    sequence: range.sequence,
                    source,
                },
            );
        }
    };
    let mut reader = GameReader::new(file.take(range.end - range.start));
    let mut games = 0_u64;
    let mut moves = 0_u64;
    let mut offset = range.start;
    loop {
        let game = match reader.read_game() {
            Ok(Some(game)) => game,
            Ok(None) => break,
            Err(source) => {
                return RangeResult {
                    sequence: range.sequence,
                    bytes: reader.bytes_read(),
                    games,
                    moves,
                    error: Some(RangeError::Frame {
                        sequence: range.sequence,
                        source,
                    }),
                };
            }
        };
        let game_number = range.first_game + games + 1;
        match validate_game(game, game_number, offset) {
            Ok(game_moves) => moves += game_moves,
            Err(error) => {
                return RangeResult {
                    sequence: range.sequence,
                    bytes: reader.bytes_read(),
                    games,
                    moves,
                    error: Some(RangeError::Validation(error)),
                };
            }
        }
        games += 1;
        offset += u64::try_from(game.len()).expect("game length fits in u64");
    }
    let bytes = reader.bytes_read();
    let error = (games != range.expected_games).then_some(RangeError::GameCount {
        sequence: range.sequence,
        expected: range.expected_games,
        actual: games,
    });
    RangeResult {
        sequence: range.sequence,
        bytes,
        games,
        moves,
        error,
    }
}

fn failed_range(sequence: usize, error: RangeError) -> RangeResult {
    RangeResult {
        sequence,
        bytes: 0,
        games: 0,
        moves: 0,
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn discovers_contiguous_balanced_game_ranges() {
        let input = b"1. e4 *\n\n1. d4 d5 *\n\n1. c4 e5 2. Nc3 *\n\n1. Nf3 d5 *\n";
        let ranges = discover_ranges(Cursor::new(input), input.len() as u64, 3).unwrap();

        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, input.len() as u64);
        assert_eq!(
            ranges.iter().map(|range| range.expected_games).sum::<u64>(),
            4
        );
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
            assert_eq!(
                pair[0].first_game + pair[0].expected_games,
                pair[1].first_game
            );
        }
    }

    #[test]
    fn handles_empty_input_and_more_workers_than_games() {
        let empty = discover_ranges(Cursor::new(b""), 0, 4).unwrap();
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].start, empty[0].end);

        let input = b"1. e4 *";
        let ranges = discover_ranges(Cursor::new(input), input.len() as u64, 8).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].expected_games, 1);
    }
}
