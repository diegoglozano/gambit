use gambit_engine::{Analysis, Score, ScoreBound, ScoreSource};
use serde::{Deserialize, Serialize};

/// Explicit direction preserves terminal mate zero when changing perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PlayerScore {
    Centipawns(i32),
    MateFor(u32),
    MateAgainst(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    pub score: PlayerScore,
    pub bound: ScoreBound,
    pub depth: Option<u32>,
    pub source: ScoreSource,
}

impl Evaluation {
    #[must_use]
    pub fn from_analysis(analysis: &Analysis, root_is_player: bool) -> Self {
        let score = match analysis.score {
            Score::Centipawns(cp) => PlayerScore::Centipawns(if root_is_player {
                cp
            } else {
                cp.saturating_neg()
            }),
            Score::Mate(n) if (n > 0) == root_is_player => PlayerScore::MateFor(n.unsigned_abs()),
            Score::Mate(n) => PlayerScore::MateAgainst(n.unsigned_abs()),
        };
        Self {
            score,
            bound: analysis.score_bound.for_player(root_is_player),
            depth: analysis.depth,
            source: analysis.score_source,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Loss {
    Centipawns { minimum: i64, exact: bool },
    AllowedMate,
    LostForcedMate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assessment {
    TurningPoint(Loss),
    NoTurningPoint,
    Inconclusive,
}

/// Versioned product hypotheses, not universal definitions of chess mistakes.
pub const SELECTION_VERSION: u32 = 1;
pub const MINIMUM_LOSS_CP: i64 = 100;
pub const ALREADY_LOST_CP: i32 = -300;

/// Only establish a turning point when the score inequalities prove it.
/// Lower/upper bounds are never silently interpreted as exact centipawns.
#[must_use]
pub fn assess(before: Evaluation, after: Evaluation) -> Assessment {
    use Assessment::{Inconclusive, NoTurningPoint, TurningPoint};
    use PlayerScore::{Centipawns, MateAgainst, MateFor};
    use ScoreBound::{Exact, Lower, Upper};

    match before.score {
        MateAgainst(_) if before.bound == Exact => return NoTurningPoint,
        Centipawns(cp) if cp < ALREADY_LOST_CP && matches!(before.bound, Exact | Upper) => {
            return NoTurningPoint;
        }
        _ => {}
    }
    let eligible = match before.score {
        Centipawns(cp) => cp >= ALREADY_LOST_CP && matches!(before.bound, Exact | Lower),
        MateFor(_) => before.bound == Exact,
        MateAgainst(_) => false,
    };
    if !eligible {
        return Inconclusive;
    }
    match (before.score, after.score) {
        (_, MateAgainst(_)) if after.bound == Exact => TurningPoint(Loss::AllowedMate),
        (MateFor(_), Centipawns(_)) if after.bound == Exact => TurningPoint(Loss::LostForcedMate),
        (_, MateFor(_)) if after.bound == Exact => NoTurningPoint,
        (Centipawns(a), Centipawns(b)) if matches!(after.bound, Exact | Upper) => {
            let minimum = i64::from(a) - i64::from(b);
            let exact = before.bound == Exact && after.bound == Exact;
            if minimum >= MINIMUM_LOSS_CP {
                TurningPoint(Loss::Centipawns { minimum, exact })
            } else if exact {
                NoTurningPoint
            } else {
                Inconclusive
            }
        }
        _ => Inconclusive,
    }
}
