use gambit_chess::MoveList;
use gambit_coaching::{
    CacheEntry, CacheKey, DiagnosisOutcome, EngineIdentity, Loss, PlayerScore, PracticeDisposition,
    PracticeProgress, ReviewGame, ReviewSummary, SolutionStatus, SummaryError, diagnose, summarize,
};
use gambit_engine::{Analysis, Cancellation, Score, ScoreBound, ScoreSource};

fn entry(id: &str, loss: i32, exact: bool, move_number: u16) -> CacheEntry {
    let root = tempfile::tempdir().unwrap();
    let engine_path = root.path().join("engine");
    std::fs::write(&engine_path, b"fixture").unwrap();
    let engine = EngineIdentity::from_file("fixture", &engine_path).unwrap();
    let pgn = format!(
        "[White \"A\"]\n[Black \"B\"]\n[SetUp \"1\"]\n[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 {move_number}\"]\n{move_number}. d4 d5 *"
    );
    let game = ReviewGame::parse(pgn.as_bytes(), "A").unwrap();
    let key = CacheKey::new(id, &game, 0, &engine, 100).unwrap();
    let diagnosis = diagnose(&game, 0, 100, &Cancellation::default(), |history| {
        let board = history.position();
        let mut moves = MoveList::default();
        board.generate_legal_moves(&mut moves);
        let best = moves.as_slice()[0];
        Ok(Analysis {
            engine: "fixture".into(),
            nodes_budget: 100,
            score: Score::Centipawns(if history.moves().is_empty() { 0 } else { loss }),
            score_bound: if exact {
                ScoreBound::Exact
            } else {
                ScoreBound::Lower
            },
            score_source: ScoreSource::LatestReport,
            depth: Some(12),
            best_move: Some(best.to_uci()),
            pv: vec![best.to_uci()],
            pv_san: vec![board.to_san(best).unwrap()],
        })
    })
    .unwrap();
    CacheEntry {
        key,
        diagnosis,
        practice: PracticeProgress::default(),
    }
}

#[test]
fn aggregates_only_objective_facts_and_preserves_bounded_statistics() {
    let mut records = vec![
        entry("1", 100, true, 1),
        entry("2", 200, false, 9),
        entry("3", 600, true, 12),
    ];
    records[0].practice.solution = SolutionStatus::WithoutReveal;
    records[1].practice.revealed = true;
    records[1].practice.disposition = PracticeDisposition::Completed;
    records[2].practice.disposition = PracticeDisposition::AgainLater;
    let summary = summarize(&records).unwrap();
    assert_eq!(summary.games_analyzed, 3);
    assert_eq!(summary.turning_points, 3);
    assert_eq!(summary.move_range, Some((1, 12)));
    assert_eq!(summary.solved_without_reveal, 1);
    assert_eq!(summary.completed_after_reveal, 1);
    assert_eq!(summary.practice_again_later, 1);
    let stats = summary.centipawn_loss.unwrap();
    assert_eq!(stats.count, 3);
    assert!((stats.mean_cp - 300.0).abs() < f64::EPSILON);
    assert!((stats.median_cp - 200.0).abs() < f64::EPSILON);
    assert!(stats.minimum_only);
    assert_eq!(summary.repeated_positions.len(), 1);
    assert_eq!(summary.repeated_positions[0].games, 3);
    assert_eq!(summary.repeated_positions[0].repeated_choices[0].games, 3);
    assert_eq!(summary.repeated_positions[0].repeated_choices[0].san, "d4");
}

#[test]
fn empty_no_result_and_mate_cases_do_not_manufacture_numeric_losses() {
    assert_eq!(summarize(&[]).unwrap(), ReviewSummary::default());
    let no_result = entry("none", 0, true, 1);
    let summary = summarize(&[no_result]).unwrap();
    assert_eq!(summary.no_clear_turning_point, 1);
    assert!(summary.centipawn_loss.is_none());
    assert!(summary.move_range.is_none());
    let mut mates = vec![entry("mate", 100, true, 1), entry("lost", 100, true, 2)];
    if let DiagnosisOutcome::TurningPoint(point) = &mut mates[0].diagnosis.outcome {
        point.after.score = PlayerScore::MateAgainst(2);
        point.loss = Loss::AllowedMate;
    }
    if let DiagnosisOutcome::TurningPoint(point) = &mut mates[1].diagnosis.outcome {
        point.before.score = PlayerScore::MateFor(2);
        point.loss = Loss::LostForcedMate;
    }
    let summary = summarize(&mates).unwrap();
    assert!(summary.centipawn_loss.is_none());
    assert_eq!(summary.allowed_mates, 1);
    assert_eq!(summary.lost_forced_mates, 1);
}

#[test]
fn duplicate_or_oversized_review_sets_are_not_double_counted() {
    let record = entry("same", 100, true, 1);
    assert_eq!(
        summarize(&[record.clone(), record.clone()]),
        Err(SummaryError::DuplicateGame)
    );
    assert_eq!(summarize(&vec![record; 7]), Err(SummaryError::TooManyGames));
}
