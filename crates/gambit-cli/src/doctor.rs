use std::cell::RefCell;
use std::io::Read;
use std::rc::Rc;
use std::time::Instant;

use gambit_chess::{Position, SanErrorKind};
use gambit_pgn::{
    Event, IncrementalParser, IncrementalParserOptions, ParserOptions, StreamParseError, Tag,
};
use serde::Serialize;

const SOURCE_CONTEXT_BYTES: usize = 128 * 1024;

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
    pub(crate) line: Option<u64>,
    pub(crate) column: Option<u64>,
    pub(crate) context: Option<String>,
    pub(crate) excerpt: Option<String>,
    pub(crate) headers: Option<GameHeaders>,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub(crate) struct GameHeaders {
    pub(crate) event: Option<String>,
    pub(crate) date: Option<String>,
    pub(crate) round: Option<String>,
    pub(crate) white: Option<String>,
    pub(crate) black: Option<String>,
}

impl GameHeaders {
    fn is_empty(&self) -> bool {
        self.event.is_none()
            && self.date.is_none()
            && self.round.is_none()
            && self.white.is_none()
            && self.black.is_none()
    }
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
                line: None,
                column: None,
                context: None,
                excerpt: None,
                headers: None,
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
    let tracker = Rc::new(RefCell::new(SourceTracker::default()));
    let input = TrackingReader::new(input, Rc::clone(&tracker));
    let mut parser = IncrementalParser::with_options(input, incremental_options);
    let mut validator = Validator::new(options.mode, Rc::clone(&tracker));
    let parse_result = parser.parse(|event| validator.observe(event));
    let bytes = parser.into_inner().bytes_read;
    let elapsed = started.elapsed().as_secs_f64();

    let parse_diagnostic = parse_result.err().map(|error| {
        diagnostic_from_stream_error(
            error,
            validator.games,
            &validator.headers,
            &tracker.borrow(),
        )
    });
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

fn diagnostic_from_stream_error(
    error: StreamParseError,
    completed_games: u64,
    headers: &GameHeaders,
    tracker: &SourceTracker,
) -> Diagnostic {
    match error {
        StreamParseError::Parse(error) => located_diagnostic(
            DiagnosticDetails {
                category: DiagnosticCategory::Syntax,
                game: completed_games + 1,
                ply: None,
                offset: error.offset as u64,
                context: None,
                message: error.to_string(),
            },
            headers,
            tracker,
        ),
        StreamParseError::Io(error) => Diagnostic {
            category: DiagnosticCategory::Input,
            game: Some(completed_games + 1),
            ply: None,
            byte: None,
            line: None,
            column: None,
            context: None,
            excerpt: None,
            headers: (!headers.is_empty()).then(|| headers.clone()),
            message: format!("failed to read PGN stream: {error}"),
        },
        StreamParseError::TokenTooLarge { offset, limit } => located_diagnostic(
            DiagnosticDetails {
                category: DiagnosticCategory::Limit,
                game: completed_games + 1,
                ply: None,
                offset,
                context: None,
                message: format!(
                    "PGN token at byte {offset} exceeds the {limit}-byte streaming limit"
                ),
            },
            headers,
            tracker,
        ),
    }
}

struct DiagnosticDetails {
    category: DiagnosticCategory,
    game: u64,
    ply: Option<u64>,
    offset: u64,
    context: Option<String>,
    message: String,
}

fn located_diagnostic(
    details: DiagnosticDetails,
    headers: &GameHeaders,
    tracker: &SourceTracker,
) -> Diagnostic {
    let location = tracker.locate(details.offset);
    Diagnostic {
        category: details.category,
        game: Some(details.game),
        ply: details.ply,
        byte: Some(details.offset),
        line: location.as_ref().map(|location| location.line),
        column: location.as_ref().map(|location| location.column),
        context: details.context,
        excerpt: location.map(|location| location.excerpt),
        headers: (!headers.is_empty()).then(|| headers.clone()),
        message: details.message,
    }
}

struct TrackingReader<R> {
    inner: R,
    bytes_read: u64,
    tracker: Rc<RefCell<SourceTracker>>,
}

impl<R> TrackingReader<R> {
    fn new(inner: R, tracker: Rc<RefCell<SourceTracker>>) -> Self {
        Self {
            inner,
            bytes_read: 0,
            tracker,
        }
    }
}

impl<R: Read> Read for TrackingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.bytes_read += read as u64;
        self.tracker.borrow_mut().append(&buffer[..read]);
        Ok(read)
    }
}

#[derive(Debug)]
struct SourceLocation {
    line: u64,
    column: u64,
    excerpt: String,
}

#[derive(Debug)]
struct SourceTracker {
    bytes: Vec<u8>,
    base_offset: u64,
    base_line: u64,
    base_column: u64,
}

impl Default for SourceTracker {
    fn default() -> Self {
        Self {
            bytes: Vec::with_capacity(SOURCE_CONTEXT_BYTES),
            base_offset: 0,
            base_line: 1,
            base_column: 1,
        }
    }
}

impl SourceTracker {
    fn append(&mut self, input: &[u8]) {
        let overflow = self
            .bytes
            .len()
            .saturating_add(input.len())
            .saturating_sub(SOURCE_CONTEXT_BYTES);
        if overflow == 0 {
            self.bytes.extend_from_slice(input);
            return;
        }

        let from_buffer = overflow.min(self.bytes.len());
        (self.base_line, self.base_column) =
            advance_location(self.base_line, self.base_column, &self.bytes[..from_buffer]);
        self.base_offset += from_buffer as u64;
        self.bytes.copy_within(from_buffer.., 0);
        self.bytes.truncate(self.bytes.len() - from_buffer);

        let from_input = overflow - from_buffer;
        if from_input > 0 {
            (self.base_line, self.base_column) =
                advance_location(self.base_line, self.base_column, &input[..from_input]);
            self.base_offset += from_input as u64;
        }
        self.bytes.extend_from_slice(&input[from_input..]);
    }

    fn locate(&self, offset: u64) -> Option<SourceLocation> {
        let relative = usize::try_from(offset.checked_sub(self.base_offset)?).ok()?;
        if relative > self.bytes.len() {
            return None;
        }
        let mut line = self.base_line;
        let mut column = self.base_column;
        let mut line_start = 0;
        for (index, byte) in self.bytes[..relative].iter().enumerate() {
            if *byte == b'\n' {
                line += 1;
                column = 1;
                line_start = index + 1;
            } else {
                column += 1;
            }
        }
        let line_end = self.bytes[relative..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(self.bytes.len(), |newline| relative + newline);
        let (excerpt_start, excerpt_end) = excerpt_bounds(line_start, line_end, relative);
        let mut excerpt = String::new();
        if excerpt_start > line_start || (line_start == 0 && self.base_column > 1) {
            excerpt.push('…');
        }
        excerpt.push_str(&String::from_utf8_lossy(
            &self.bytes[excerpt_start..excerpt_end],
        ));
        if excerpt_end < line_end {
            excerpt.push('…');
        }
        if excerpt.ends_with('\r') {
            excerpt.pop();
        }
        Some(SourceLocation {
            line,
            column,
            excerpt,
        })
    }
}

fn advance_location(mut line: u64, mut column: u64, bytes: &[u8]) -> (u64, u64) {
    for byte in bytes {
        if *byte == b'\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn excerpt_bounds(line_start: usize, line_end: usize, offset: usize) -> (usize, usize) {
    const MAX_EXCERPT_BYTES: usize = 200;
    if line_end - line_start <= MAX_EXCERPT_BYTES {
        return (line_start, line_end);
    }
    let start = offset
        .saturating_sub(MAX_EXCERPT_BYTES / 2)
        .max(line_start)
        .min(line_end - MAX_EXCERPT_BYTES);
    (start, start + MAX_EXCERPT_BYTES)
}

struct Validator {
    mode: ValidationMode,
    tracker: Rc<RefCell<SourceTracker>>,
    position: Position,
    before_last_move: Position,
    variation_stack: Vec<(Position, Position)>,
    fen: Option<(Vec<u8>, usize)>,
    games: u64,
    moves: u64,
    game_ply: u64,
    headers: GameHeaders,
    error: Option<Diagnostic>,
}

impl Validator {
    fn new(mode: ValidationMode, tracker: Rc<RefCell<SourceTracker>>) -> Self {
        Self {
            mode,
            tracker,
            position: Position::initial(),
            before_last_move: Position::initial(),
            variation_stack: Vec::new(),
            fen: None,
            games: 0,
            moves: 0,
            game_ply: 0,
            headers: GameHeaders::default(),
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
                self.headers = GameHeaders::default();
            }
            Event::Tag(tag) => {
                self.record_tag(tag);
            }
            Event::MovetextStart { .. } if self.mode == ValidationMode::Semantic => {
                if let Some((fen, offset)) = &self.fen {
                    match Position::from_fen(fen) {
                        Ok(position) => self.position = position,
                        Err(error) => {
                            self.error = Some(located_diagnostic(
                                DiagnosticDetails {
                                    category: DiagnosticCategory::InvalidFen,
                                    game: self.games + 1,
                                    ply: Some(0),
                                    offset: *offset as u64,
                                    context: Some(String::from_utf8_lossy(fen).into_owned()),
                                    message: error.to_string(),
                                },
                                &self.headers,
                                &self.tracker.borrow(),
                            ));
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
                        self.error = Some(located_diagnostic(
                            DiagnosticDetails {
                                category,
                                game: self.games + 1,
                                ply: Some(self.game_ply),
                                offset: token.span().start as u64,
                                context: Some(
                                    String::from_utf8_lossy(token.as_bytes()).into_owned(),
                                ),
                                message: error.to_string(),
                            },
                            &self.headers,
                            &self.tracker.borrow(),
                        ));
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
            Event::GameEnd { .. } => {
                self.games += 1;
                self.headers = GameHeaders::default();
            }
            Event::MovetextStart { .. }
            | Event::MoveNumber { .. }
            | Event::Nag(_)
            | Event::Comment(_)
            | Event::VariationStart(_)
            | Event::VariationEnd(_)
            | Event::Outcome { .. } => {}
        }
    }

    fn record_tag(&mut self, tag: Tag<'_>) {
        let name = tag.name();
        if name == b"FEN" && self.mode == ValidationMode::Semantic {
            self.fen = Some((tag.value().into_owned(), tag.span().start));
            return;
        }
        let destination = match name {
            b"Event" => &mut self.headers.event,
            b"Date" => &mut self.headers.date,
            b"Round" => &mut self.headers.round,
            b"White" => &mut self.headers.white,
            b"Black" => &mut self.headers.black,
            _ => return,
        };
        let value = tag.value();
        *destination = Some(String::from_utf8_lossy(&value).into_owned());
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

    #[test]
    fn source_tracker_reports_lines_columns_and_excerpts() {
        let mut tracker = SourceTracker::default();
        tracker.append(b"first\nsecond line\nthird");
        let location = tracker.locate(9).unwrap();
        assert_eq!(location.line, 2);
        assert_eq!(location.column, 4);
        assert_eq!(location.excerpt, "second line");
    }

    #[test]
    fn source_tracker_retains_locations_after_compaction() {
        let mut tracker = SourceTracker::default();
        let prefix = vec![b'x'; SOURCE_CONTEXT_BYTES];
        tracker.append(&prefix);
        tracker.append(b"\nlast line");
        let location = tracker.locate(SOURCE_CONTEXT_BYTES as u64 + 6).unwrap();
        assert_eq!(location.line, 2);
        assert_eq!(location.column, 6);
        assert_eq!(location.excerpt, "last line");
    }
}
