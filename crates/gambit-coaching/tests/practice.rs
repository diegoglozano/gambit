use gambit_chess::{Color, MoveList};
use gambit_coaching::{
    AttemptVerdict, CacheKey, CacheStore, Diagnosis, DiagnosisOutcome, EngineIdentity, Evaluation,
    PlayerScore, PracticeDisposition, PracticeError, ReviewGame, SolutionStatus, assess_attempt,
    diagnose, verify_attempt,
};
use gambit_engine::{Analysis, Cancellation, Error, GamePosition, Score, ScoreBound, ScoreSource};

fn analysis(position: &GamePosition, cp: i32) -> Analysis {
    let board = position.position();
    let mut legal = MoveList::default();
    board.generate_legal_moves(&mut legal);
    let preferred = if board.side_to_move() == Color::White {
        "a2a3"
    } else {
        "a7a6"
    };
    let m = legal
        .as_slice()
        .iter()
        .find(|m| m.to_uci() == preferred)
        .copied()
        .unwrap_or(legal.as_slice()[0]);
    Analysis {
        engine: "fixture".into(),
        nodes_budget: 100,
        score: Score::Centipawns(cp),
        score_bound: ScoreBound::Exact,
        score_source: ScoreSource::LatestReport,
        depth: Some(12),
        best_move: Some(m.to_uci()),
        pv: vec![m.to_uci()],
        pv_san: vec![board.to_san(m).unwrap()],
    }
}

fn lesson(player: &str) -> (ReviewGame, Diagnosis) {
    let game = ReviewGame::parse(b"[White \"A\"]\n[Black \"B\"]\n1.d4 d5 *", player).unwrap();
    let mut calls = 0;
    let diagnosis = diagnose(&game, 0, 100, &Cancellation::default(), |position| {
        calls += 1;
        Ok(analysis(position, if calls == 1 { 0 } else { 100 }))
    })
    .unwrap();
    assert!(matches!(
        diagnosis.outcome,
        DiagnosisOutcome::TurningPoint(_)
    ));
    (game, diagnosis)
}

fn eval(cp: i32, bound: ScoreBound) -> Evaluation {
    Evaluation {
        score: PlayerScore::Centipawns(cp),
        bound,
        depth: Some(12),
        source: ScoreSource::LatestReport,
    }
}

#[test]
fn tolerance_and_bounds_accept_equivalence_without_exact_move_matching() {
    use AttemptVerdict::{Inconclusive, Strong, TryAgain};
    use ScoreBound::{Exact, Lower, Upper};
    assert_eq!(assess_attempt(eval(0, Exact), eval(-30, Exact)), Strong);
    assert_eq!(assess_attempt(eval(0, Exact), eval(-31, Exact)), TryAgain);
    assert_eq!(assess_attempt(eval(0, Exact), eval(100, Exact)), Strong);
    assert_eq!(assess_attempt(eval(0, Upper), eval(-30, Lower)), Strong);
    assert_eq!(assess_attempt(eval(0, Lower), eval(-31, Upper)), TryAgain);
    assert_eq!(
        assess_attempt(eval(0, Lower), eval(-30, Upper)),
        Inconclusive
    );
    assert_eq!(
        assess_attempt(eval(0, Upper), eval(-31, Lower)),
        Inconclusive
    );
}

#[test]
fn mate_outcomes_never_become_fake_centipawns() {
    use AttemptVerdict::{Inconclusive, Strong, TryAgain};
    let mut reference = eval(0, ScoreBound::Exact);
    reference.score = PlayerScore::MateFor(3);
    let mut attempted = reference;
    attempted.score = PlayerScore::MateFor(7);
    assert_eq!(assess_attempt(reference, attempted), Strong);
    assert_eq!(
        assess_attempt(reference, eval(1000, ScoreBound::Exact)),
        TryAgain
    );
    attempted.score = PlayerScore::MateAgainst(2);
    assert_eq!(assess_attempt(reference, attempted), TryAgain);
    assert_eq!(assess_attempt(attempted, attempted), Inconclusive);
    reference.bound = ScoreBound::Lower;
    assert_eq!(
        assess_attempt(reference, eval(0, ScoreBound::Exact)),
        Inconclusive
    );
}

#[test]
fn alternative_moves_pass_for_both_colors_and_feedback_has_no_answer() {
    for (player, uci) in [("A", "b2b3"), ("B", "b7b6")] {
        let (_, diagnosis) = lesson(player);
        let mut calls = 0;
        let attempt = verify_attempt(&diagnosis, uci, &Cancellation::default(), |position| {
            calls += 1;
            Ok(analysis(position, if calls == 1 { 50 } else { 80 }))
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(attempt.verdict, AttemptVerdict::Strong);
        assert_eq!(attempt.uci, uci);
        let json = serde_json::to_value(attempt).unwrap();
        assert!(json.get("best_move").is_none());
        assert!(json.get("pv").is_none());
    }
}

#[test]
fn illegal_moves_do_not_search_and_the_preferred_move_still_does() {
    let (_, diagnosis) = lesson("A");
    let illegal = verify_attempt(&diagnosis, "e2e5", &Cancellation::default(), |_| {
        panic!("illegal move must not search")
    })
    .unwrap();
    assert_eq!(illegal.verdict, AttemptVerdict::Illegal);
    assert!(illegal.evaluation.is_none());
    let DiagnosisOutcome::TurningPoint(point) = &diagnosis.outcome else {
        unreachable!()
    };
    let mut calls = 0;
    let strong = verify_attempt(
        &diagnosis,
        &point.best_uci,
        &Cancellation::default(),
        |position| {
            calls += 1;
            assert_eq!(position.moves().last(), Some(&point.best_uci));
            Ok(analysis(position, 0))
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(strong.verdict, AttemptVerdict::Strong);
}

#[test]
fn engine_failures_cancellation_and_changed_settings_do_not_complete_attempts() {
    let (_, diagnosis) = lesson("A");
    let cancel = Cancellation::default();
    assert!(matches!(
        verify_attempt(&diagnosis, "b2b3", &cancel, |p| {
            cancel.cancel();
            Ok(analysis(p, 0))
        }),
        Err(PracticeError::Engine(Error::Cancelled))
    ));
    assert!(matches!(
        verify_attempt(&diagnosis, "b2b3", &Cancellation::default(), |_| Err(
            Error::Exited
        )),
        Err(PracticeError::Engine(Error::Exited))
    ));
    assert!(matches!(
        verify_attempt(&diagnosis, "b2b3", &Cancellation::default(), |p| {
            let mut a = analysis(p, 0);
            a.nodes_budget = 200;
            Ok(a)
        }),
        Err(PracticeError::InvalidEvidence)
    ));
}

#[test]
fn attempts_and_explicit_outcomes_survive_reopening_and_history_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let library = root.path().join("library.gambit");
    let engine = root.path().join("engine");
    std::fs::write(&library, b"evidence").unwrap();
    std::fs::write(&engine, b"fixture engine").unwrap();
    let (game, diagnosis) = lesson("A");
    let identity = EngineIdentity::from_file("fixture", &engine).unwrap();
    let key = CacheKey::new("game-1", &game, 0, &identity, 100).unwrap();
    let store = CacheStore::for_library(root.path(), &library).unwrap();
    store.save_diagnosis(&key, diagnosis.clone()).unwrap();
    let illegal = verify_attempt(&diagnosis, "e2e5", &Cancellation::default(), |_| {
        panic!("illegal")
    })
    .unwrap();
    for _ in 0..101 {
        store.record_attempt(&key, illegal.clone()).unwrap();
    }
    let strong = verify_attempt(&diagnosis, "b2b3", &Cancellation::default(), |p| {
        Ok(analysis(p, 0))
    })
    .unwrap();
    store.record_attempt(&key, strong).unwrap();
    store
        .update_practice(&key, |p| p.disposition = PracticeDisposition::Completed)
        .unwrap();
    let record = CacheStore::for_library(root.path(), &library)
        .unwrap()
        .load(&key)
        .unwrap()
        .unwrap();
    assert_eq!(record.practice.total_attempts, 102);
    assert_eq!(record.practice.attempts.len(), 100);
    assert_eq!(record.practice.solution, SolutionStatus::WithoutReveal);
    assert_eq!(record.practice.disposition, PracticeDisposition::Completed);
    assert!(!record.practice.revealed);
    let other = CacheKey::new("game-2", &game, 0, &identity, 100).unwrap();
    store.save_diagnosis(&other, diagnosis.clone()).unwrap();
    store
        .update_practice(&other, |p| p.revealed = true)
        .unwrap();
    let strong = verify_attempt(&diagnosis, "b2b3", &Cancellation::default(), |p| {
        Ok(analysis(p, 0))
    })
    .unwrap();
    let record = store.record_attempt(&other, strong).unwrap();
    assert_eq!(record.practice.solution, SolutionStatus::AfterHint);
}
