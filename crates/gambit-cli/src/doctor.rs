use std::io::Read;
use std::time::Instant;

use gambit_chess::{Position, SanErrorKind};
use gambit_pgn::{
    Event, IncrementalParser, IncrementalParserOptions, ParserOptions, StreamParseError,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValidationMode {
    Semantic,
    Syntax,
}

impl ValidationMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Syntax => "syntax",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DoctorOptions {
    pub(crate) mode: ValidationMode,
    pub(crate) require_outcome: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReportStatus {
    Valid,
    Invalid,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticCategory {
    Syntax,
    InvalidFen,
    InvalidSan,
    IllegalMove,
    AmbiguousMove,
    InvalidCheckSuffix,
    Input,
    Limit,
}

impl DiagnosticCategory {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::InvalidFen => "invalid_fen",
            Self::InvalidSan => "invalid_san",
            Self::IllegalMove => "illegal_move",
            Self::AmbiguousMove => "ambiguous_move",
            Self::InvalidCheckSuffix => "invalid_check_suffix",
            Self::Input => "input",
            Self::Limit => "limit",
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub(crate) category: DiagnosticCategory,
    pub(crate) game: Option<u64>,
    pub(crate) ply: Option<u64>,
    pub(crate) byte: Option<u64>,
    pub(crate) context: Option<String>,
    pub(crate) message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) schema_version: u8,
    pub(crate) status: ReportStatus,
    pub(crate) source: String,
    pub(crate) mode: &'static str,
    pub(crate) outcome_required: bool,
    pub(crate) bytes: u64,
    pub(crate) games: u64,
    pub(crate) moves: u64,
    pub(crate) elapsed_seconds: f64,
    pub(crate) throughput_mib_per_second: f64,
    pub(crate) diagnostic: Option<Diagnostic>,
}

impl Report {
    pub(crate) const fn exit_code(&self) -> u8 {
        match self.status {
            ReportStatus::Valid => 0,
            ReportStatus::Invalid => 1,
            ReportStatus::Error => 3,
        }
    }

    pub(crate) fn input_error(source: String, options: DoctorOptions, message: String) -> Self {
        Self {
            schema_version: 1,
            status: ReportStatus::Error,
            source,
            mode: options.mode.label(),
            outcome_required: options.require_outcome,
            bytes: 0,
            games: 0,
            moves: 0,
            elapsed_seconds: 0.0,
            throughput_mib_per_second: 0.0,
            diagnostic: Some(Diagnostic {
                category: DiagnosticCategory::Input,
                game: None,
                ply: None,
                byte: None,
                context: None,
                message,
            }),
        }
    }
}

pub(crate) fn inspect<R: Read>(input: R, source: String, options: DoctorOptions) -> Report {
    let started = Instant::now();
    let parser_options = if options.require_outcome {
        ParserOptions::STRICT
    } else {
        ParserOptions::LENIENT
    };
    let incremental_options = IncrementalParserOptions {
        parser: parser_options,
        ..IncrementalParserOptions::default()
    };
    let input = CountingReader::new(input);
    let mut parser = IncrementalParser::with_options(input, incremental_options);
    let mut validator = Validator::new(options.mode);
    let parse_result = parser.parse(|event| validator.observe(event));
    let bytes = parser.into_inner().bytes_read;
    let elapsed = started.elapsed().as_secs_f64();

    let parse_diagnostic = parse_result
        .err()
        .map(|error| diagnostic_from_stream_error(error, validator.games));
    let diagnostic = earliest_diagnostic(validator.error, parse_diagnostic);
    let status = diagnostic
        .as_ref()
        .map_or(ReportStatus::Valid, |diagnostic| {
            if matches!(
                diagnostic.category,
                DiagnosticCategory::Input | DiagnosticCategory::Limit
            ) {
                ReportStatus::Error
            } else {
                ReportStatus::Invalid
            }
        });
    #[allow(clippy::cast_precision_loss)]
    let throughput = if elapsed > 0.0 {
        bytes as f64 / (1024.0 * 1024.0) / elapsed
    } else {
        0.0
    };

    Report {
        schema_version: 1,
        status,
        source,
        mode: options.mode.label(),
        outcome_required: options.require_outcome,
        bytes,
        games: validator.games,
        moves: validator.moves,
        elapsed_seconds: elapsed,
        throughput_mib_per_second: throughput,
        diagnostic,
    }
}

fn earliest_diagnostic(
    semantic: Option<Diagnostic>,
    parsing: Option<Diagnostic>,
) -> Option<Diagnostic> {
    match (semantic, parsing) {
        (Some(semantic), Some(parsing)) => match (semantic.byte, parsing.byte) {
            (Some(semantic_byte), Some(parsing_byte)) if parsing_byte < semantic_byte => {
                Some(parsing)
            }
            _ => Some(semantic),
        },
        (Some(diagnostic), None) | (None, Some(diagnostic)) => Some(diagnostic),
        (None, None) => None,
    }
}

fn diagnostic_from_stream_error(error: StreamParseError, completed_games: u64) -> Diagnostic {
    match error {
        StreamParseError::Parse(error) => Diagnostic {
            category: DiagnosticCategory::Syntax,
            game: Some(completed_games + 1),
            ply: None,
            byte: Some(error.offset as u64),
            context: None,
            message: error.to_string(),
        },
        StreamParseError::Io(error) => Diagnostic {
            category: DiagnosticCategory::Input,
            game: Some(completed_games + 1),
            ply: None,
            byte: None,
            context: None,
            message: format!("failed to read PGN stream: {error}"),
        },
        StreamParseError::TokenTooLarge { offset, limit } => Diagnostic {
            category: DiagnosticCategory::Limit,
            game: Some(completed_games + 1),
            ply: None,
            byte: Some(offset),
            context: None,
            message: format!("PGN token at byte {offset} exceeds the {limit}-byte streaming limit"),
        },
    }
}

struct CountingReader<R> {
    inner: R,
    bytes_read: u64,
}

impl<R> CountingReader<R> {
    const fn new(inner: R) -> Self {
        Self {
            inner,
            bytes_read: 0,
        }
    }
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.bytes_read += read as u64;
        Ok(read)
    }
}

struct Validator {
    mode: ValidationMode,
    position: Position,
    before_last_move: Position,
    variation_stack: Vec<(Position, Position)>,
    fen: Option<(Vec<u8>, usize)>,
    games: u64,
    moves: u64,
    game_ply: u64,
    error: Option<Diagnostic>,
}

impl Validator {
    fn new(mode: ValidationMode) -> Self {
        Self {
            mode,
            position: Position::initial(),
            before_last_move: Position::initial(),
            variation_stack: Vec::new(),
            fen: None,
            games: 0,
            moves: 0,
            game_ply: 0,
            error: None,
        }
    }

    fn observe(&mut self, event: Event<'_>) {
        if self.error.is_some() {
            return;
        }
        match event {
            Event::GameStart { .. } => {
                self.position = Position::initial();
                self.before_last_move = self.position;
                self.variation_stack.clear();
                self.fen = None;
                self.game_ply = 0;
            }
            Event::Tag(tag) if tag.name() == b"FEN" && self.mode == ValidationMode::Semantic => {
                self.fen = Some((tag.value().into_owned(), tag.span().start));
            }
            Event::MovetextStart { .. } if self.mode == ValidationMode::Semantic => {
                if let Some((fen, offset)) = &self.fen {
                    match Position::from_fen(fen) {
                        Ok(position) => self.position = position,
                        Err(error) => {
                            self.error = Some(Diagnostic {
                                category: DiagnosticCategory::InvalidFen,
                                game: Some(self.games + 1),
                                ply: Some(0),
                                byte: Some(*offset as u64),
                                context: Some(String::from_utf8_lossy(fen).into_owned()),
                                message: error.to_string(),
                            });
                            return;
                        }
                    }
                }
                self.before_last_move = self.position;
            }
            Event::San(token) => {
                self.game_ply += 1;
                if self.mode == ValidationMode::Syntax {
                    self.moves += 1;
                    return;
                }
                self.before_last_move = self.position;
                match self.position.play_san(token.as_bytes()) {
                    Ok(_) => self.moves += 1,
                    Err(error) => {
                        let category = match error.kind {
                            SanErrorKind::Empty | SanErrorKind::InvalidSyntax => {
                                DiagnosticCategory::InvalidSan
                            }
                            SanErrorKind::IllegalMove => DiagnosticCategory::IllegalMove,
                            SanErrorKind::AmbiguousMove => DiagnosticCategory::AmbiguousMove,
                            SanErrorKind::InvalidCheckSuffix => {
                                DiagnosticCategory::InvalidCheckSuffix
                            }
                        };
                        self.error = Some(Diagnostic {
                            category,
                            game: Some(self.games + 1),
                            ply: Some(self.game_ply),
                            byte: Some(token.span().start as u64),
                            context: Some(String::from_utf8_lossy(token.as_bytes()).into_owned()),
                            message: error.to_string(),
                        });
                    }
                }
            }
            Event::VariationStart(_) if self.mode == ValidationMode::Semantic => {
                self.variation_stack
                    .push((self.position, self.before_last_move));
                let branch_base = self.before_last_move;
                self.position = branch_base;
                self.before_last_move = branch_base;
            }
            Event::VariationEnd(_) if self.mode == ValidationMode::Semantic => {
                if let Some((position, before_last_move)) = self.variation_stack.pop() {
                    self.position = position;
                    self.before_last_move = before_last_move;
                }
            }
            Event::GameEnd { .. } => self.games += 1,
            Event::Tag(_)
            | Event::MovetextStart { .. }
            | Event::MoveNumber { .. }
            | Event::Nag(_)
            | Event::Comment(_)
            | Event::VariationStart(_)
            | Event::VariationEnd(_)
            | Event::Outcome { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inspect_bytes(input: &[u8], mode: ValidationMode) -> Report {
        inspect(
            input,
            String::from("test"),
            DoctorOptions {
                mode,
                require_outcome: true,
            },
        )
    }

    #[test]
    fn semantic_error_before_later_parse_error_wins() {
        let report = inspect_bytes(b"1. e5 ) *", ValidationMode::Semantic);
        assert_eq!(report.status, ReportStatus::Invalid);
        assert_eq!(
            report.diagnostic.unwrap().category,
            DiagnosticCategory::IllegalMove
        );
    }

    #[test]
    fn syntax_mode_does_not_execute_moves() {
        let report = inspect_bytes(b"1. e5 *", ValidationMode::Syntax);
        assert_eq!(report.status, ReportStatus::Valid);
        assert_eq!(report.moves, 1);
    }
}
