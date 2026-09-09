#![cfg(unix)]
use gambit_engine::{Cancellation, Error, GamePosition, Score, ScoreBound, analyze, analyze_game};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(scenario: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "gambit-engine-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let script = path.join("engine");
        fs::write(
            path.join("engine.recorded"),
            include_str!("fixtures/stockfish-17.1-node-limit.uci"),
        )
        .unwrap();
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nscenario={scenario}\n{}",
                include_str!("fixtures/engine.sh")
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("engine")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(pid) = fs::read_to_string(self.0.join("engine.pid")) {
            let alive = std::process::Command::new("kill")
                .args(["-0", &pid])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(!alive, "engine process {pid} leaked");
        }
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn handshake_fixed_nodes_and_exact_result() {
    let fixture = Fixture::new("success");
    let result = analyze(
        &fixture.path(),
        FEN,
        100,
        &Cancellation::default(),
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(result.engine, "Stockfish 17.1 fixture");
    assert_eq!(result.nodes_budget, 100);
    assert_eq!(result.score, Score::Centipawns(25));
    assert_eq!(result.score_bound, ScoreBound::Exact);
    assert_eq!(result.best_move.as_deref(), Some("d2d4"));
    assert_eq!(result.pv, ["d2d4", "d7d5"]);
}

#[test]
fn node_limit_preserves_the_chosen_moves_bound_instead_of_a_different_moves_score() {
    let fixture = Fixture::new("recorded");
    let result = analyze(
        &fixture.path(),
        NODE_LIMIT_FEN,
        100_000,
        &Cancellation::default(),
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(result.best_move.as_deref(), Some("a5c5"));
    assert_eq!(result.score, Score::Centipawns(22));
    assert_eq!(result.score_bound, ScoreBound::Lower);
    assert_eq!(result.depth, Some(15));
    assert_eq!(result.pv, ["a5c5"]);
}

const NODE_LIMIT_FEN: &str = "r3r1k1/ppbn1p2/2p2n1p/q2pp1pb/4P3/PP1P2PP/1BPN1PBN/R3QRK1 b - - 2 16";

#[test]
fn sends_starting_position_and_history_instead_of_only_the_final_board() {
    let fixture = Fixture::new("success");
    let mut position = GamePosition::default();
    for m in ["g1f3", "g8f6", "f3g1", "f6g8"] {
        position.play_uci(m).unwrap();
    }
    analyze_game(
        &fixture.path(),
        &position,
        100,
        &Cancellation::default(),
        Duration::from_secs(2),
    )
    .unwrap();
    let commands = fs::read_to_string(fixture.0.join("engine.commands")).unwrap();
    assert!(
        commands
            .lines()
            .any(|line| line == format!("position fen {FEN} moves g1f3 g8f6 f3g1 f6g8"))
    );
    assert!(!commands.contains(&position.position().to_fen()));
    assert!(commands.contains("go nodes 100\n"));
}

#[test]
fn crash_and_malformed_output_fail_only_this_request() {
    for scenario in ["startup-crash", "crash", "malformed", "mismatch"] {
        let fixture = Fixture::new(scenario);
        assert!(matches!(
            analyze(
                &fixture.path(),
                FEN,
                100,
                &Cancellation::default(),
                Duration::from_secs(2)
            ),
            Err(Error::Exited | Error::Protocol(_))
        ));
    }
    handshake_fixed_nodes_and_exact_result();
}

#[test]
fn cancellation_during_search_and_flood_is_bounded() {
    for scenario in ["hang", "flood", "startup-hang"] {
        let fixture = Fixture::new(scenario);
        let cancel = Cancellation::default();
        let signal = cancel.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            signal.cancel();
        });
        let start = Instant::now();
        assert!(matches!(
            analyze(&fixture.path(), FEN, 100, &cancel, Duration::from_secs(5)),
            Err(Error::Cancelled)
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        worker.join().unwrap();
    }
}

#[test]
fn watchdog_and_input_validation() {
    let fixture = Fixture::new("hang");
    assert!(matches!(
        analyze(
            &fixture.path(),
            FEN,
            100,
            &Cancellation::default(),
            Duration::from_millis(80)
        ),
        Err(Error::Timeout)
    ));
    for fen in ["invalid", &format!("{FEN}\nquit")] {
        assert!(matches!(
            analyze(
                &fixture.path(),
                fen,
                100,
                &Cancellation::default(),
                Duration::from_secs(1)
            ),
            Err(Error::InvalidPosition)
        ));
    }
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        analyze(
            std::path::Path::new("/does-not-exist"),
            FEN,
            100,
            &cancel,
            Duration::from_secs(1)
        ),
        Err(Error::Cancelled)
    ));
}

#[test]
#[ignore = "set GAMBIT_ENGINE_PATH to the packaged Stockfish executable"]
fn packaged_stockfish() {
    let path = PathBuf::from(std::env::var_os("GAMBIT_ENGINE_PATH").expect("GAMBIT_ENGINE_PATH"));
    let result = analyze(
        &path,
        FEN,
        100_000,
        &Cancellation::default(),
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(result.engine.starts_with("Stockfish 17.1"));
    assert!(result.best_move.is_some());
    assert!(!result.pv.is_empty());
    let mate = analyze(
        &path,
        "7k/6Q1/6K1/8/8/8/8/8 b - - 0 1",
        100_000,
        &Cancellation::default(),
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(mate.score, Score::Mate(0));
    assert!(mate.best_move.is_none());
    let mut history = GamePosition::default();
    for m in ["f2f3", "e7e5", "g2g4", "d8h4"] {
        history.play_uci(m).unwrap();
    }
    let history_mate = analyze_game(
        &path,
        &history,
        100_000,
        &Cancellation::default(),
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(history_mate.score, Score::Mate(0));
    assert!(history_mate.best_move.is_none());
    let limited = analyze(
        &path,
        NODE_LIMIT_FEN,
        100_000,
        &Cancellation::default(),
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(limited.best_move.as_deref(), Some("a5c5"));
    assert_eq!(limited.score_bound, ScoreBound::Lower);
}
