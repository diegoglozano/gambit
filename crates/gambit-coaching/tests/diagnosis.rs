use gambit_chess::MoveList;
use gambit_coaching::{
    Assessment, DiagnosisError, DiagnosisOutcome, Evaluation, Loss, PlayerScore, ReviewGame,
    assess, diagnose,
};
use gambit_engine::{Analysis, Cancellation, Error, GamePosition, Score, ScoreBound, ScoreSource};

fn eval(score: PlayerScore, bound: ScoreBound) -> Evaluation {
    Evaluation {
        score,
        bound,
        depth: Some(12),
        source: ScoreSource::LatestReport,
    }
}

#[test]
fn exact_thresholds_and_already_lost_boundary() {
    use PlayerScore::Centipawns as Cp;
    use ScoreBound::Exact;
    for (before, after, expected) in [
        (0, -99, Assessment::NoTurningPoint),
        (
            0,
            -100,
            Assessment::TurningPoint(Loss::Centipawns {
                minimum: 100,
                exact: true,
            }),
        ),
        (
            -300,
            -400,
            Assessment::TurningPoint(Loss::Centipawns {
                minimum: 100,
                exact: true,
            }),
        ),
        (-301, -1000, Assessment::NoTurningPoint),
        (100, 200, Assessment::NoTurningPoint),
        (
            i32::MAX,
            -i32::MAX,
            Assessment::TurningPoint(Loss::Centipawns {
                minimum: 4_294_967_294,
                exact: true,
            }),
        ),
    ] {
        assert_eq!(
            assess(eval(Cp(before), Exact), eval(Cp(after), Exact)),
            expected
        );
    }
}

#[test]
fn bounds_must_prove_the_loss_and_eligibility() {
    use PlayerScore::Centipawns as Cp;
    use ScoreBound::{Exact, Lower, Upper};
    assert_eq!(
        assess(eval(Cp(0), Lower), eval(Cp(-100), Upper)),
        Assessment::TurningPoint(Loss::Centipawns {
            minimum: 100,
            exact: false
        })
    );
    for (before, after) in [(Upper, Exact), (Exact, Lower), (Upper, Lower)] {
        assert_eq!(
            assess(eval(Cp(0), before), eval(Cp(-200), after)),
            Assessment::Inconclusive
        );
    }
    assert_eq!(
        assess(eval(Cp(-301), Upper), eval(Cp(-1000), Exact)),
        Assessment::NoTurningPoint
    );
    assert_eq!(
        assess(eval(Cp(-301), Lower), eval(Cp(-1000), Exact)),
        Assessment::Inconclusive
    );
    assert_eq!(
        assess(eval(Cp(0), Lower), eval(Cp(-99), Upper)),
        Assessment::Inconclusive
    );
}

#[test]
fn mates_remain_typed_and_terminal_zero_keeps_its_direction() {
    use PlayerScore::{Centipawns as Cp, MateAgainst, MateFor};
    use ScoreBound::Exact;
    assert_eq!(
        assess(eval(Cp(0), Exact), eval(MateAgainst(3), Exact)),
        Assessment::TurningPoint(Loss::AllowedMate)
    );
    assert_eq!(
        assess(eval(MateFor(3), Exact), eval(Cp(500), Exact)),
        Assessment::TurningPoint(Loss::LostForcedMate)
    );
    assert_eq!(
        assess(eval(MateFor(3), Exact), eval(MateFor(8), Exact)),
        Assessment::NoTurningPoint
    );
    assert_eq!(
        assess(eval(MateAgainst(3), Exact), eval(MateAgainst(1), Exact)),
        Assessment::NoTurningPoint
    );
    let mut result = analysis(&GamePosition::default(), 0);
    result.score = Score::Mate(0);
    assert_eq!(
        Evaluation::from_analysis(&result, true).score,
        MateAgainst(0)
    );
    let delivered = Evaluation::from_analysis(&result, false);
    assert_eq!(delivered.score, MateFor(0));
    assert_eq!(
        serde_json::to_value(delivered).unwrap()["score"]["kind"],
        "mate_for"
    );
    result.score = Score::Mate(-3);
    assert_eq!(Evaluation::from_analysis(&result, false).score, MateFor(3));
}

fn analysis(position: &GamePosition, cp: i32) -> Analysis {
    let board = position.position();
    let mut moves = MoveList::default();
    board.generate_legal_moves(&mut moves);
    let best = moves.as_slice()[0];
    Analysis {
        engine: "fixture".into(),
        nodes_budget: 100,
        score: Score::Centipawns(cp),
        score_bound: ScoreBound::Exact,
        score_source: ScoreSource::LatestReport,
        depth: Some(12),
        best_move: Some(best.to_uci()),
        pv: vec![best.to_uci()],
        pv_san: vec![board.to_san(best).unwrap()],
    }
}

const GAME: &str = "[White \"A\"]\n[Black \"B\"]\n1.d4 d5 2.Bf4 Nf6 3.e3 e6 4.Nf3 Nc6 *";

#[test]
fn chooses_first_supported_player_error_and_stops_before_later_blunders() {
    for (player, expected_ply) in [("A", 5), ("B", 6)] {
        let game = ReviewGame::parse(GAME.as_bytes(), player).unwrap();
        let cancel = Cancellation::default();
        let mut calls = 0;
        let diagnosis = diagnose(&game, 2, 100, &cancel, |position| {
            let raw_cp = [0, 99, 20, 180][calls];
            calls += 1;
            Ok(analysis(position, raw_cp))
        })
        .unwrap();
        assert_eq!(calls, 4);
        assert_eq!(diagnosis.analyzed_moves, 2);
        let DiagnosisOutcome::TurningPoint(point) = diagnosis.outcome else {
            panic!("expected turning point")
        };
        assert_eq!(point.ply, expected_ply);
        assert_eq!(point.history.len(), expected_ply - 1);
        assert_eq!(
            point.loss,
            Loss::Centipawns {
                minimum: 200,
                exact: true
            }
        );
        assert_eq!(point.before.score, PlayerScore::Centipawns(20));
        assert_eq!(point.after.score, PlayerScore::Centipawns(-180));
    }
}

#[test]
fn no_result_and_uncertainty_are_honest() {
    let game = ReviewGame::parse(GAME.as_bytes(), "A").unwrap();
    let cancel = Cancellation::default();
    let diagnosis = diagnose(&game, 4, 100, &cancel, |position| Ok(analysis(position, 0))).unwrap();
    assert_eq!(diagnosis.outcome, DiagnosisOutcome::NoClearTurningPoint);
    assert_eq!(diagnosis.analyzed_moves, 2);
    let diagnosis = diagnose(&game, 4, 100, &cancel, |position| {
        let mut result = analysis(position, 1000);
        result.score_bound = ScoreBound::Upper;
        Ok(result)
    })
    .unwrap();
    assert_eq!(diagnosis.outcome, DiagnosisOutcome::NoClearTurningPoint);
    assert_eq!(diagnosis.inconclusive_moves, 2);
    let empty = diagnose(&game, 100, 100, &cancel, |_| {
        panic!("no engine for short game")
    })
    .unwrap();
    assert_eq!(empty.analyzed_moves, 0);
    assert!(empty.engine.is_none());
}

#[test]
fn cancellation_failure_and_changed_settings_never_return_partial_success() {
    let game = ReviewGame::parse(GAME.as_bytes(), "A").unwrap();
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        diagnose(&game, 0, 100, &cancel, |_| panic!("cancelled")),
        Err(DiagnosisError::Engine(Error::Cancelled))
    ));
    let cancel = Cancellation::default();
    let mut calls = 0;
    assert!(matches!(
        diagnose(&game, 0, 100, &cancel, |position| {
            calls += 1;
            cancel.cancel();
            Ok(analysis(position, 0))
        }),
        Err(DiagnosisError::Engine(Error::Cancelled))
    ));
    assert_eq!(calls, 1);
    assert!(matches!(
        diagnose(&game, 0, 100, &Cancellation::default(), |_| Err(
            Error::Exited
        )),
        Err(DiagnosisError::Engine(Error::Exited))
    ));
    assert!(matches!(
        diagnose(&game, 0, 200, &Cancellation::default(), |p| Ok(analysis(
            p, 0
        ))),
        Err(DiagnosisError::InconsistentEvidence)
    ));
}

#[test]
fn does_not_label_the_preferred_move_an_error_from_search_noise() {
    let game = ReviewGame::parse(b"[White \"A\"]\n[Black \"B\"]\n1.d4 *", "A").unwrap();
    let diagnosis = diagnose(&game, 0, 100, &Cancellation::default(), |position| {
        let mut result = analysis(position, if position.moves().is_empty() { 0 } else { 500 });
        if position.moves().is_empty() {
            result.best_move = Some("d2d4".into());
            result.pv = vec!["d2d4".into()];
            result.pv_san = vec!["d4".into()];
        }
        Ok(result)
    })
    .unwrap();
    assert_eq!(diagnosis.outcome, DiagnosisOutcome::NoClearTurningPoint);
    assert_eq!(diagnosis.inconclusive_moves, 1);
    let restored: gambit_coaching::Diagnosis =
        serde_json::from_slice(&serde_json::to_vec(&diagnosis).unwrap()).unwrap();
    assert_eq!(restored, diagnosis);
}
