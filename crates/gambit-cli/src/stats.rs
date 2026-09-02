use std::io::Read;
use std::time::Instant;

use gambit_pgn::{
    Event, IncrementalParser, IncrementalParserOptions, Outcome, ParserOptions, StreamParseError,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatsOptions {
    pub require_outcome: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatsStatus {
    Valid,
    Invalid,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ResultCounts {
    pub white_wins: u64,
    pub black_wins: u64,
    pub draws: u64,
    pub unfinished: u64,
}

impl ResultCounts {
    fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::WhiteWins => self.white_wins += 1,
            Outcome::BlackWins => self.black_wins += 1,
            Outcome::Draw => self.draws += 1,
            Outcome::Unknown => self.unfinished += 1,
        }
    }

    pub fn add(&mut self, other: Self) {
        self.white_wins += other.white_wins;
        self.black_wins += other.black_wins;
        self.draws += other.draws;
        self.unfinished += other.unfinished;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct GameLengthStats {
    pub minimum_plies: Option<u64>,
    pub average_plies: f64,
    pub maximum_plies: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatsDiagnosticCategory {
    Syntax,
    Input,
    Limit,
}

impl StatsDiagnosticCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Input => "input",
            Self::Limit => "limit",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StatsDiagnostic {
    pub category: StatsDiagnosticCategory,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte: Option<u64>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct StatsReport {
    pub schema_version: u8,
    pub status: StatsStatus,
    pub source: String,
    pub outcome_required: bool,
    pub bytes: u64,
    pub games: u64,
    pub mainline_plies: u64,
    pub results: ResultCounts,
    pub game_length: GameLengthStats,
    pub elapsed_seconds: f64,
    pub throughput_mib_per_second: f64,
    pub diagnostic: Option<StatsDiagnostic>,
}

impl StatsReport {
    pub fn input_error(source: String, options: StatsOptions, message: String) -> Self {
        Self {
            schema_version: 1,
            status: StatsStatus::Error,
            source,
            outcome_required: options.require_outcome,
            bytes: 0,
            games: 0,
            mainline_plies: 0,
            results: ResultCounts::default(),
            game_length: GameLengthStats::default(),
            elapsed_seconds: 0.0,
            throughput_mib_per_second: 0.0,
            diagnostic: Some(StatsDiagnostic {
                category: StatsDiagnosticCategory::Input,
                byte: None,
                message,
            }),
        }
    }

    pub const fn exit_code(&self) -> u8 {
        match self.status {
            StatsStatus::Valid => 0,
            StatsStatus::Invalid => 1,
            StatsStatus::Error => 3,
        }
    }
}

#[derive(Debug)]
struct Accumulator {
    games: u64,
    mainline_plies: u64,
    results: ResultCounts,
    minimum_plies: Option<u64>,
    maximum_plies: Option<u64>,
    current_plies: u64,
    current_outcome: Outcome,
    variation_depth: u32,
}

impl Default for Accumulator {
    fn default() -> Self {
        Self {
            games: 0,
            mainline_plies: 0,
            results: ResultCounts::default(),
            minimum_plies: None,
            maximum_plies: None,
            current_plies: 0,
            current_outcome: Outcome::Unknown,
            variation_depth: 0,
        }
    }
}

impl Accumulator {
    fn observe(&mut self, event: Event<'_>) {
        match event {
            Event::GameStart { .. } => {
                self.current_plies = 0;
                self.current_outcome = Outcome::Unknown;
                self.variation_depth = 0;
            }
            Event::San(_) if self.variation_depth == 0 => self.current_plies += 1,
            Event::VariationStart(_) => self.variation_depth += 1,
            Event::VariationEnd(_) => self.variation_depth -= 1,
            Event::Outcome { outcome, .. } if self.variation_depth == 0 => {
                self.current_outcome = outcome;
            }
            Event::GameEnd { .. } => self.finish_game(),
            _ => {}
        }
    }

    fn finish_game(&mut self) {
        self.games += 1;
        self.mainline_plies += self.current_plies;
        self.results.record(self.current_outcome);
        self.minimum_plies = Some(self.minimum_plies.map_or(self.current_plies, |minimum| {
            minimum.min(self.current_plies)
        }));
        self.maximum_plies = Some(self.maximum_plies.map_or(self.current_plies, |maximum| {
            maximum.max(self.current_plies)
        }));
    }

    fn game_length(&self) -> GameLengthStats {
        #[allow(clippy::cast_precision_loss)]
        let average_plies = if self.games == 0 {
            0.0
        } else {
            self.mainline_plies as f64 / self.games as f64
        };
        GameLengthStats {
            minimum_plies: self.minimum_plies,
            average_plies,
            maximum_plies: self.maximum_plies,
        }
    }
}

pub fn inspect<R: Read>(reader: R, source: String, options: StatsOptions) -> StatsReport {
    let parser_options = IncrementalParserOptions {
        parser: if options.require_outcome {
            ParserOptions::STRICT
        } else {
            ParserOptions::LENIENT
        },
        ..IncrementalParserOptions::default()
    };
    let mut parser = IncrementalParser::with_options(reader, parser_options);
    let mut accumulator = Accumulator::default();
    let started = Instant::now();
    let result = parser.parse(|event| accumulator.observe(event));
    let elapsed_seconds = started.elapsed().as_secs_f64();
    let bytes = parser.stats().bytes_read;
    #[allow(clippy::cast_precision_loss)]
    let throughput_mib_per_second = if elapsed_seconds > 0.0 {
        bytes as f64 / (1024.0 * 1024.0) / elapsed_seconds
    } else {
        0.0
    };
    let (status, diagnostic) = match result {
        Ok(_) => (StatsStatus::Valid, None),
        Err(StreamParseError::Parse(error)) => (
            StatsStatus::Invalid,
            Some(StatsDiagnostic {
                category: StatsDiagnosticCategory::Syntax,
                byte: u64::try_from(error.offset).ok(),
                message: error.to_string(),
            }),
        ),
        Err(StreamParseError::Io(error)) => (
            StatsStatus::Error,
            Some(StatsDiagnostic {
                category: StatsDiagnosticCategory::Input,
                byte: None,
                message: error.to_string(),
            }),
        ),
        Err(StreamParseError::TokenTooLarge { offset, limit }) => (
            StatsStatus::Error,
            Some(StatsDiagnostic {
                category: StatsDiagnosticCategory::Limit,
                byte: Some(offset),
                message: format!(
                    "PGN token at byte {offset} exceeds the {limit}-byte streaming limit"
                ),
            }),
        ),
    };

    StatsReport {
        schema_version: 1,
        status,
        source,
        outcome_required: options.require_outcome,
        bytes,
        games: accumulator.games,
        mainline_plies: accumulator.mainline_plies,
        results: accumulator.results,
        game_length: accumulator.game_length(),
        elapsed_seconds,
        throughput_mib_per_second,
        diagnostic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_mainline_plies_and_outcomes() {
        let report = inspect(
            &b"1. e4 (1. d4 d5 1/2-1/2) e5 1-0\n\n1. d4 *\n"[..],
            String::from("memory"),
            StatsOptions {
                require_outcome: true,
            },
        );

        assert_eq!(report.status, StatsStatus::Valid);
        assert_eq!(report.games, 2);
        assert_eq!(report.mainline_plies, 3);
        assert_eq!(report.results.white_wins, 1);
        assert_eq!(report.results.draws, 0);
        assert_eq!(report.results.unfinished, 1);
        assert_eq!(report.game_length.minimum_plies, Some(1));
        assert!((report.game_length.average_plies - 1.5).abs() < f64::EPSILON);
        assert_eq!(report.game_length.maximum_plies, Some(2));
    }

    #[test]
    fn preserves_completed_counts_on_a_syntax_error() {
        let report = inspect(
            &b"1. e4 *\n\n1. d4"[..],
            String::from("memory"),
            StatsOptions {
                require_outcome: true,
            },
        );

        assert_eq!(report.status, StatsStatus::Invalid);
        assert_eq!(report.games, 1);
        assert_eq!(report.mainline_plies, 1);
        assert!(report.diagnostic.is_some());
    }
}
