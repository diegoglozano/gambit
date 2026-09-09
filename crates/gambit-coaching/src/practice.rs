use crate::{Diagnosis, DiagnosisOutcome, Evaluation, PlayerScore};
use gambit_engine::{Analysis, Cancellation, Error, GamePosition, ScoreBound};
use serde::{Deserialize, Serialize};

pub const ACCEPTANCE_CP: i64 = 30;
pub const PRACTICE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptVerdict {
    Strong,
    TryAgain,
    Inconclusive,
    Illegal,
}

/// Only the player's submitted move appears in feedback, never the answer/PV.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub uci: String,
    pub verdict: AttemptVerdict,
    pub reference: Option<Evaluation>,
    pub evaluation: Option<Evaluation>,
}

#[derive(Debug)]
pub enum PracticeError {
    NoExercise,
    InvalidEvidence,
    InvalidMoveText,
    Engine(Error),
}
impl std::fmt::Display for PracticeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for PracticeError {}

/// Accept equivalent legal alternatives, not just one coordinate string.
/// Comparison uses inequalities; insufficient evidence is not a failed attempt.
#[must_use]
pub fn assess_attempt(reference: Evaluation, attempted: Evaluation) -> AttemptVerdict {
    use AttemptVerdict::{Inconclusive, Strong, TryAgain};
    use PlayerScore::{Centipawns, MateAgainst, MateFor};
    use ScoreBound::{Exact, Lower, Upper};
    match (reference.score, attempted.score) {
        (Centipawns(best), Centipawns(actual)) => {
            let loss = i64::from(best) - i64::from(actual);
            if matches!(reference.bound, Exact | Upper)
                && matches!(attempted.bound, Exact | Lower)
                && loss <= ACCEPTANCE_CP
            {
                Strong
            } else if matches!(reference.bound, Exact | Lower)
                && matches!(attempted.bound, Exact | Upper)
                && loss > ACCEPTANCE_CP
            {
                TryAgain
            } else {
                Inconclusive
            }
        }
        (Centipawns(_) | MateFor(_), MateFor(_))
            if reference.bound == Exact && attempted.bound == Exact =>
        {
            Strong
        }
        (MateFor(_), Centipawns(_) | MateAgainst(_)) | (Centipawns(_), MateAgainst(_))
            if reference.bound == Exact && attempted.bound == Exact =>
        {
            TryAgain
        }
        _ => Inconclusive,
    }
}

/// Evaluate reference and attempted continuations with identical history/budget.
/// Even the recorded preferred move is actually searched; it can share the one
/// reference search. Alternative moves are never rejected merely for differing.
///
/// # Errors
/// Rejects missing/corrupt exercise evidence, oversized input, cancellation,
/// engine failure or changed settings. Ordinary illegal moves return feedback
/// without launching the engine.
pub fn verify_attempt(
    diagnosis: &Diagnosis,
    uci: &str,
    cancel: &Cancellation,
    mut analyze: impl FnMut(&GamePosition) -> Result<Analysis, Error>,
) -> Result<Attempt, PracticeError> {
    check_cancelled(cancel)?;
    if uci.len() > 5 {
        return Err(PracticeError::InvalidMoveText);
    }
    let DiagnosisOutcome::TurningPoint(point) = &diagnosis.outcome else {
        return Err(PracticeError::NoExercise);
    };
    let mut root =
        GamePosition::from_fen(&point.initial_fen).map_err(|_| PracticeError::InvalidEvidence)?;
    for m in &point.history {
        root.play_uci(m)
            .map_err(|_| PracticeError::InvalidEvidence)?;
    }
    if root.position().to_fen() != point.position_fen {
        return Err(PracticeError::InvalidEvidence);
    }
    let mut attempted = root.clone();
    if attempted.play_uci(uci).is_err() {
        return Ok(Attempt {
            uci: uci.into(),
            verdict: AttemptVerdict::Illegal,
            reference: None,
            evaluation: None,
        });
    }
    root.play_uci(&point.best_uci)
        .map_err(|_| PracticeError::InvalidEvidence)?;
    let best = analyze(&root).map_err(PracticeError::Engine)?;
    check_cancelled(cancel)?;
    validate_analysis(diagnosis, &best)?;
    let reference = Evaluation::from_analysis(&best, false);
    let (evaluation, verdict) = if uci == point.best_uci {
        let verdict = if matches!(reference.score, PlayerScore::MateAgainst(_)) {
            AttemptVerdict::Inconclusive
        } else {
            AttemptVerdict::Strong
        };
        (reference, verdict)
    } else {
        let actual = analyze(&attempted).map_err(PracticeError::Engine)?;
        check_cancelled(cancel)?;
        validate_analysis(diagnosis, &actual)?;
        let evaluation = Evaluation::from_analysis(&actual, false);
        (evaluation, assess_attempt(reference, evaluation))
    };
    Ok(Attempt {
        uci: uci.into(),
        verdict,
        reference: Some(reference),
        evaluation: Some(evaluation),
    })
}

fn validate_analysis(diagnosis: &Diagnosis, analysis: &Analysis) -> Result<(), PracticeError> {
    if diagnosis.nodes == 0
        || analysis.nodes_budget != diagnosis.nodes
        || diagnosis.engine.as_ref() != Some(&analysis.engine)
    {
        Err(PracticeError::InvalidEvidence)
    } else {
        Ok(())
    }
}

fn check_cancelled(cancel: &Cancellation) -> Result<(), PracticeError> {
    if cancel.is_cancelled() {
        Err(PracticeError::Engine(Error::Cancelled))
    } else {
        Ok(())
    }
}
