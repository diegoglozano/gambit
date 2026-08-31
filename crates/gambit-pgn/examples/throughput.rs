use std::env;
use std::fs;
use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use gambit_pgn::Parser;

fn main() -> ExitCode {
    let (input, source) = if let Some(path) = env::args_os().nth(1) {
        match fs::read(&path) {
            Ok(input) => (input, path.to_string_lossy().into_owned()),
            Err(error) => {
                eprintln!("failed to read {}: {error}", path.to_string_lossy());
                return ExitCode::FAILURE;
            }
        }
    } else {
        (synthetic_corpus(), String::from("synthetic corpus"))
    };

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut iterations = 0_u64;
    let started = Instant::now();
    while Instant::now() < deadline || iterations == 0 {
        let mut count = 0_usize;
        for event in Parser::new(black_box(&input)) {
            match event {
                Ok(event) => {
                    black_box(event);
                    count += 1;
                }
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        black_box(count);
        iterations += 1;
    }
    let elapsed = started.elapsed();
    #[allow(clippy::cast_precision_loss)] // Human-readable benchmark output is approximate.
    let bytes = input.len() as f64 * iterations as f64;
    let mib_per_second = bytes / elapsed.as_secs_f64() / (1024.0 * 1024.0);
    println!(
        "{source}: {iterations} iteration(s), {:.2} MiB/s ({:.2} MiB in {:.3}s)",
        mib_per_second,
        bytes / (1024.0 * 1024.0),
        elapsed.as_secs_f64()
    );
    ExitCode::SUCCESS
}

fn synthetic_corpus() -> Vec<u8> {
    const GAME: &[u8] = br#"[Event "Synthetic"]
[Site "Local"]
[Date "2026.08.31"]
[Round "1"]
[White "Alpha"]
[Black "Beta"]
[Result "1/2-1/2"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7
6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7
11. c4 {A representative comment.} (11. Nbd2 Bb7 $5) 11... c6
12. Nc3 Qc7 13. Be3 Bb7 14. Rc1 b4 15. Nd5 cxd5 1/2-1/2

"#;
    const TARGET_BYTES: usize = 8 * 1024 * 1024;

    let mut corpus = Vec::with_capacity(TARGET_BYTES + GAME.len());
    while corpus.len() < TARGET_BYTES {
        corpus.extend_from_slice(GAME);
    }
    corpus
}
