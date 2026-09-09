use gambit_engine::{Analysis, Cancellation, Error, GamePosition};
use serde::{Deserialize, Serialize};

use crate::{Assessment, Evaluation, InputError, Loss, ReviewGame, assess};

/// Provisional first budget after native Intel/Apple Silicon profiling.
pub const DEFAULT_NODES: u64 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurningPoint {
    pub ply: usize,
    pub initial_fen: String,
    pub history: Vec<String>,
    pub position_fen: String,
    pub played_uci: String,
    pub played_san: String,
    pub best_uci: String,
    pub best_san: String,
    pub before: Evaluation,
    pub after: Evaluation,
    pub loss: Loss,
    pub pv: Vec<String>,
    pub pv_san: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "evidence", rename_all = "snake_case")]
pub enum DiagnosisOutcome {
    TurningPoint(Box<TurningPoint>),
    NoClearTurningPoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub evidence_version: u32,
    pub selection_version: u32,
    pub nodes: u64,
    pub engine: Option<String>,
    pub shared_ply: usize,
    pub analyzed_moves: usize,
    /// These moves had bounds insufficient to establish or exclude a candidate.
    pub inconclusive_moves: usize,
    pub outcome: DiagnosisOutcome,
}

#[derive(Debug)]
pub enum DiagnosisError {
    Input(InputError),
    Engine(Error),
    InconsistentEvidence,
    InvalidBudget,
}

impl std::fmt::Display for DiagnosisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DiagnosisError {}

/// Select the earliest supported player-caused deterioration; stop immediately
/// after it. The caller supplies a serialized local engine on a background worker.
/// No cache writes or partial-game success are performed by this function.
///
/// # Errors
/// Returns an error on replay/engine failure, cancellation, or inconsistent engine
/// identity/settings/evidence. Finished earlier games are owned by the caller.
pub fn diagnose(
    game: &ReviewGame,
    shared_ply: usize,
    nodes: u64,
    cancel: &Cancellation,
    mut analyze: impl FnMut(&GamePosition) -> Result<Analysis, Error>,
) -> Result<Diagnosis, DiagnosisError> {
    if nodes == 0 {
        return Err(DiagnosisError::InvalidBudget);
    }
    check_cancelled(cancel)?;
    let mut diagnosis = Diagnosis {
        evidence_version: gambit_engine::EVIDENCE_VERSION,
        selection_version: crate::score::SELECTION_VERSION,
        nodes,
        engine: None,
        shared_ply,
        analyzed_moves: 0,
        inconclusive_moves: 0,
        outcome: DiagnosisOutcome::NoClearTurningPoint,
    };
    for decision in game.decisions(shared_ply) {
        check_cancelled(cancel)?;
        let decision = decision.map_err(DiagnosisError::Input)?;
        let before = analyze(&decision.before).map_err(DiagnosisError::Engine)?;
        check_cancelled(cancel)?;
        let after = analyze(&decision.after).map_err(DiagnosisError::Engine)?;
        check_cancelled(cancel)?;
        if before.nodes_budget != nodes
            || after.nodes_budget != nodes
            || before.engine.is_empty()
            || before.engine != after.engine
            || diagnosis
                .engine
                .as_ref()
                .is_some_and(|engine| *engine != before.engine)
        {
            return Err(DiagnosisError::InconsistentEvidence);
        }
        diagnosis.engine = Some(before.engine.clone());
        diagnosis.analyzed_moves += 1;
        let before_score = Evaluation::from_analysis(&before, true);
        let after_score = Evaluation::from_analysis(&after, false);
        match assess(before_score, after_score) {
            Assessment::TurningPoint(loss) => {
                let best_uci = before
                    .best_move
                    .clone()
                    .ok_or(DiagnosisError::InconsistentEvidence)?;
                let best_san = before
                    .pv_san
                    .first()
                    .cloned()
                    .ok_or(DiagnosisError::InconsistentEvidence)?;
                if before.pv.first() != Some(&best_uci) || before.pv.len() != before.pv_san.len() {
                    return Err(DiagnosisError::InconsistentEvidence);
                }
                // Fixed-budget noise must not call the engine's own preferred
                // move an actionable error with no distinct alternative.
                if best_uci == decision.played_uci {
                    diagnosis.inconclusive_moves += 1;
                    continue;
                }
                diagnosis.outcome = DiagnosisOutcome::TurningPoint(Box::new(TurningPoint {
                    ply: decision.ply,
                    initial_fen: decision.before.initial_fen().into(),
                    history: decision.before.moves().to_vec(),
                    position_fen: decision.before.position().to_fen(),
                    played_uci: decision.played_uci,
                    played_san: decision.played_san,
                    best_uci,
                    best_san,
                    before: before_score,
                    after: after_score,
                    loss,
                    pv: before.pv,
                    pv_san: before.pv_san,
                }));
                return Ok(diagnosis);
            }
            Assessment::Inconclusive => diagnosis.inconclusive_moves += 1,
            Assessment::NoTurningPoint => {}
        }
    }
    check_cancelled(cancel)?;
    Ok(diagnosis)
}

fn check_cancelled(cancel: &Cancellation) -> Result<(), DiagnosisError> {
    if cancel.is_cancelled() {
        Err(DiagnosisError::Engine(Error::Cancelled))
    } else {
        Ok(())
    }
}
