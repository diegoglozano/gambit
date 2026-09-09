use gambit_coaching::{
    AttemptVerdict, DEFAULT_NODES, DiagnosisOutcome, ReviewGame, diagnose, verify_attempt,
};
use gambit_engine::{Cancellation, analyze_game};
use std::{path::PathBuf, time::Duration};

#[test]
#[ignore = "set GAMBIT_ENGINE_PATH to packaged Stockfish"]
fn packaged_practice() {
    let engine = PathBuf::from(std::env::var_os("GAMBIT_ENGINE_PATH").expect("GAMBIT_ENGINE_PATH"));
    // Both Qg8# and Qh6# mate. Qh7+ instead lets the king take the queen.
    let game = ReviewGame::parse(b"[White \"A\"]\n[Black \"B\"]\n[SetUp \"1\"]\n[FEN \"7k/5K2/6Q1/8/8/8/8/8 w - - 0 1\"]\n1.Qh7+ *", "A").unwrap();
    let cancel = Cancellation::default();
    let search = |position: &gambit_engine::GamePosition| {
        analyze_game(
            &engine,
            position,
            DEFAULT_NODES,
            &cancel,
            Duration::from_secs(30),
        )
    };
    let diagnosis = diagnose(&game, 0, DEFAULT_NODES, &cancel, search).unwrap();
    let DiagnosisOutcome::TurningPoint(point) = &diagnosis.outcome else {
        panic!("expected lost forced mate")
    };
    assert_eq!(point.ply, 1);
    let alternative = if point.best_uci == "g6g8" {
        "g6h6"
    } else {
        "g6g8"
    };
    assert_ne!(point.best_uci, alternative);
    let strong = verify_attempt(&diagnosis, alternative, &cancel, search).unwrap();
    assert_eq!(strong.verdict, AttemptVerdict::Strong);
    let poor = verify_attempt(&diagnosis, "g6h7", &cancel, search).unwrap();
    assert_eq!(poor.verdict, AttemptVerdict::TryAgain);
}
