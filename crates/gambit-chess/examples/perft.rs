use std::env;
use std::process::ExitCode;
use std::time::Instant;

use gambit_chess::{MoveList, Position};

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let depth = match arguments.next() {
        Some(argument) => match argument.parse::<u8>() {
            Ok(depth) => depth,
            Err(error) => {
                eprintln!("invalid depth: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => 6,
    };
    let fen = arguments.collect::<Vec<_>>().join(" ");
    let position = if fen.is_empty() {
        Position::initial()
    } else {
        match Position::from_fen(fen.as_bytes()) {
            Ok(position) => position,
            Err(error) => {
                eprintln!("invalid FEN: {error}");
                return ExitCode::FAILURE;
            }
        }
    };

    let started = Instant::now();
    let nodes = perft(position, depth);
    let elapsed = started.elapsed();
    #[allow(clippy::cast_precision_loss)]
    let million_nodes = nodes as f64 / 1_000_000.0;

    println!("depth: {depth}");
    println!("nodes: {nodes}");
    println!("elapsed: {:.3}s", elapsed.as_secs_f64());
    println!(
        "move generation: {:.2} million nodes/s",
        million_nodes / elapsed.as_secs_f64()
    );
    ExitCode::SUCCESS
}

fn perft(position: Position, depth: u8) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut moves = MoveList::default();
    position.generate_legal_moves(&mut moves);
    if depth == 1 {
        return moves.len() as u64;
    }
    moves
        .as_slice()
        .iter()
        .map(|chess_move| {
            let mut next = position;
            next.play_unchecked(*chess_move);
            perft(next, depth - 1)
        })
        .sum()
}
