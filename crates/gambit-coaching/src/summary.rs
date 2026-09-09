use crate::{
    CacheEntry, DiagnosisOutcome, Loss, PracticeDisposition, SolutionStatus, TurningPoint,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LossStatistics {
    pub count: usize,
    pub mean_cp: f64,
    pub median_cp: f64,
    /// If any loss was bounded, display both aggregates as lower estimates.
    pub minimum_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepeatedChoice {
    pub uci: String,
    pub san: String,
    pub games: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepeatedPosition {
    pub example_fen: String,
    pub games: usize,
    pub repeated_choices: Vec<RepeatedChoice>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub games_analyzed: usize,
    pub turning_points: usize,
    pub no_clear_turning_point: usize,
    pub inconclusive_moves: usize,
    pub solved_without_reveal: usize,
    pub solved_after_hint: usize,
    pub completed_after_reveal: usize,
    pub practice_again_later: usize,
    pub move_range: Option<(u16, u16)>,
    pub centipawn_loss: Option<LossStatistics>,
    pub allowed_mates: usize,
    pub lost_forced_mates: usize,
    pub repeated_positions: Vec<RepeatedPosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryError {
    TooManyGames,
    DuplicateGame,
    InvalidEvidence,
}
impl std::fmt::Display for SummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SummaryError {}

/// Aggregate only recorded facts from one current review set. No chess themes or
/// opening-quality claims are inferred. Pending/failed/unsupported queue counts
/// belong to the queue layer, not to completed diagnosis records.
///
/// # Errors
/// Rejects duplicate game identities, more than six games, or invalid evidence.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn summarize(records: &[CacheEntry]) -> Result<ReviewSummary, SummaryError> {
    if records.len() > 6 {
        return Err(SummaryError::TooManyGames);
    }
    let mut ids = HashSet::new();
    let mut summary = ReviewSummary::default();
    let mut losses = Vec::new();
    let mut bounded = false;
    let mut positions: BTreeMap<[u8; 16], Vec<&TurningPoint>> = BTreeMap::new();
    for record in records {
        if !ids.insert(record.key.game_id()) {
            return Err(SummaryError::DuplicateGame);
        }
        summary.games_analyzed += 1;
        summary.inconclusive_moves += record.diagnosis.inconclusive_moves;
        let progress = &record.practice;
        summary.solved_without_reveal +=
            usize::from(progress.solution == SolutionStatus::WithoutReveal);
        summary.solved_after_hint += usize::from(progress.solution == SolutionStatus::AfterHint);
        summary.completed_after_reveal += usize::from(
            progress.revealed
                && progress.disposition == PracticeDisposition::Completed
                && progress.solution != SolutionStatus::WithoutReveal,
        );
        summary.practice_again_later +=
            usize::from(progress.disposition == PracticeDisposition::AgainLater);
        let DiagnosisOutcome::TurningPoint(point) = &record.diagnosis.outcome else {
            summary.no_clear_turning_point += 1;
            continue;
        };
        summary.turning_points += 1;
        let position = gambit_chess::Position::from_fen(point.position_fen.as_bytes())
            .map_err(|_| SummaryError::InvalidEvidence)?;
        let move_number = position.fullmove_number();
        summary.move_range = Some(
            summary
                .move_range
                .map_or((move_number, move_number), |(low, high)| {
                    (low.min(move_number), high.max(move_number))
                }),
        );
        positions
            .entry(position.position_key())
            .or_default()
            .push(point);
        match point.loss {
            Loss::Centipawns { minimum, exact } => {
                if !(100..=i64::from(i32::MAX) * 2).contains(&minimum) {
                    return Err(SummaryError::InvalidEvidence);
                }
                losses.push(minimum);
                bounded |= !exact;
            }
            Loss::AllowedMate => summary.allowed_mates += 1,
            Loss::LostForcedMate => summary.lost_forced_mates += 1,
        }
    }
    if !losses.is_empty() {
        losses.sort_unstable();
        let count = losses.len();
        let middle = count / 2;
        // At most six losses, each <= 2 * i32::MAX: integers fit exactly in f64.
        let median_cp = if count % 2 == 0 {
            (losses[middle - 1] + losses[middle]) as f64 / 2.0
        } else {
            losses[middle] as f64
        };
        summary.centipawn_loss = Some(LossStatistics {
            count,
            mean_cp: losses.iter().sum::<i64>() as f64 / count as f64,
            median_cp,
            minimum_only: bounded,
        });
    }
    for group in positions.values().filter(|group| group.len() > 1) {
        let first = group.first().ok_or(SummaryError::InvalidEvidence)?;
        let mut choices: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
        for point in group {
            let entry = choices
                .entry(&point.played_uci)
                .or_insert((&point.played_san, 0));
            entry.1 += 1;
        }
        let repeated_choices = choices
            .into_iter()
            .filter(|(_, (_, count))| *count > 1)
            .map(|(uci, (san, games))| RepeatedChoice {
                uci: uci.into(),
                san: san.into(),
                games,
            })
            .collect();
        summary.repeated_positions.push(RepeatedPosition {
            example_fen: first.position_fen.clone(),
            games: group.len(),
            repeated_choices,
        });
    }
    Ok(summary)
}
