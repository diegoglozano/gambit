//! Reproducible six-game cost measurement. No turning-point selection or cache.
mod support;

use gambit_chess::Color;
use gambit_engine::{Analysis, Cancellation, Score, ScoreBound, analyze};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

fn score_json(score: Score) -> Value {
    match score {
        Score::Centipawns(value) => json!({"kind": "cp", "value": value}),
        Score::Mate(value) => json!({"kind": "mate", "value": value}),
    }
}

fn evidence(result: &Analysis, root_is_player: bool) -> Value {
    let bound = |bound| match bound {
        ScoreBound::Exact => "exact",
        ScoreBound::Lower => "lower",
        ScoreBound::Upper => "upper",
    };
    json!({
        "engine": result.engine, "root_is_player": root_is_player,
        "score": score_json(result.score),
        "score_bound": bound(result.score_bound), "depth": result.depth,
        "player_score_bound": bound(result.score_bound.for_player(root_is_player)),
        "player_score": score_json(result.score.for_player(root_is_player)),
        "best_move": result.best_move, "pv": result.pv,
    })
}

fn emit(value: &Value) -> io::Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, value)?;
    writeln!(out)?;
    out.flush()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err("usage: review_profile ENGINE SIX_GAME_PGN PLAYER SHARED_PLY NODES".into());
    }
    let engine = PathBuf::from(&args[0]);
    let player = args[2].to_str().ok_or("player must be UTF-8")?;
    let shared_ply: usize = args[3].to_str().ok_or("invalid shared ply")?.parse()?;
    let nodes: u64 = args[4].to_str().ok_or("invalid nodes")?.parse()?;
    if nodes == 0 {
        return Err("nodes must be positive".into());
    }
    let games = support::workload(File::open(&args[1])?, player, shared_ply)?;
    emit(
        &json!({"type": "start", "schema": 1, "architecture": std::env::consts::ARCH,
        "games": games.len(), "shared_ply": shared_ply, "nodes_per_search": nodes,
        "threads": 1, "hash_mib": 16, "fresh_process_per_search": true}),
    )?;
    let total = Instant::now();
    let mut failures = 0;
    let mut searches = 0;
    let mut completed_decisions = 0;
    for (index, game) in games.iter().enumerate() {
        let start = Instant::now();
        let mut samples = Vec::new();
        for decision in &game.decisions {
            let before = analyze(
                &engine,
                &decision.before,
                nodes,
                &Cancellation::default(),
                Duration::from_secs(30),
            );
            searches += 1;
            let result = before.map_err(|e| ("before", e)).and_then(|before| {
                searches += 1;
                analyze(
                    &engine,
                    &decision.after,
                    nodes,
                    &Cancellation::default(),
                    Duration::from_secs(30),
                )
                .map(|after| (before, after))
                .map_err(|e| ("after", e))
            });
            match result {
                Ok((before, after)) => {
                    completed_decisions += 1;
                    samples.push(json!({"ply": decision.ply, "played": decision.played,
                        "fen": decision.before, "before": evidence(&before, true), "after": evidence(&after, false)}));
                }
                Err((stage, error)) => {
                    failures += 1;
                    samples.push(json!({"ply": decision.ply, "error": error.to_string(),
                        "stage": stage, "fen": decision.before, "after_fen": decision.after}));
                    // A broken engine must not cost 30 seconds for every remaining ply.
                    break;
                }
            }
        }
        emit(
            &json!({"type": "game", "game": index + 1, "plies": game.plies,
            "player_color": if game.player == Color::White { "white" } else { "black" },
            "decisions": game.decisions.len(), "elapsed_ms": start.elapsed().as_millis(), "samples": samples}),
        )?;
    }
    emit(
        &json!({"type": "summary", "games": games.len(), "searches": searches,
        "completed_decisions": completed_decisions, "failed_games": failures,
        "elapsed_ms": total.elapsed().as_millis(), "complete": failures == 0}),
    )?;
    if failures != 0 {
        return Err("profile incomplete; inspect the per-game failures".into());
    }
    Ok(())
}
