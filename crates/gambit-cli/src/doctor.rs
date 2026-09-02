use std::cell::RefCell;
use std::io::Read;
use std::rc::Rc;
use std::time::Instant;

use gambit_chess::{Color, Position, SanErrorKind};
use gambit_pgn::{
    Event, FrameError, GameReader, IncrementalParser, IncrementalParserOptions, Outcome,
    ParseError, Parser, ParserOptions, StreamParseError, Tag, Token,
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
    pub(crate) max_errors: usize,
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
    InconsistentResult,
    InconsistentSetup,
    IncorrectMoveNumber,
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
            Self::InconsistentResult => "inconsistent_result",
            Self::InconsistentSetup => "inconsistent_setup",
            Self::IncorrectMoveNumber => "incorrect_move_number",
            Self::Input => "input",
            Self::Limit => "limit",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
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
    pub(crate) additional_diagnostics: Vec<Diagnostic>,
    pub(crate) diagnostic_count: usize,
    pub(crate) error_limit_reached: bool,
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
            additional_diagnostics: Vec::new(),
            diagnostic_count: 1,
            error_limit_reached: false,
        }
    }

    pub(crate) fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostic
            .iter()
            .chain(self.additional_diagnostics.iter())
    }
}

pub(crate) fn inspect<R: Read>(input: R, source: String, options: DoctorOptions) -> Report {
    if options.max_errors == 1 {
        inspect_first(input, source, options)
    } else {
        inspect_complete(input, source, options)
    }
}

fn inspect_first<R: Read>(input: R, source: String, options: DoctorOptions) -> Report {
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
    let mut validator = Validator::new(options.mode, Rc::clone(&tracker), 0, 0);
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
        diagnostic_count: usize::from(diagnostic.is_some()),
        diagnostic,
        additional_diagnostics: Vec::new(),
        error_limit_reached: false,
    }
}

fn inspect_complete<R: Read>(input: R, source: String, options: DoctorOptions) -> Report {
    let started = Instant::now();
    let mut reader = GameReader::new(input);
    let mut diagnostics = Vec::new();
    let mut games = 0_u64;
    let mut moves = 0_u64;
    let mut base_offset = 0_u64;
    let mut base_line = 1_u64;
    let mut base_column = 1_u64;
    let mut error_limit_reached = false;

    loop {
        match reader.read_game() {
            Ok(Some(game)) => {
                let game_len = game.len();
                let result = inspect_framed_game(
                    game,
                    games + 1,
                    base_offset,
                    base_line,
                    base_column,
                    options,
                    true,
                );
                games += 1;
                moves += result.moves;
                if let Some(diagnostic) = result.diagnostic {
                    diagnostics.push(diagnostic);
                }
                (base_line, base_column) =
                    advance_location(base_line, base_column, &game[..game_len]);
                base_offset += game_len as u64;
            }
            Ok(None) => break,
            Err(FrameError::MissingOutcome { .. }) => {
                let game = reader.pending_game();
                let result = inspect_framed_game(
                    game,
                    games + 1,
                    base_offset,
                    base_line,
                    base_column,
                    options,
                    false,
                );
                moves += result.moves;
                if !options.require_outcome {
                    games += 1;
                }
                if let Some(diagnostic) = result.diagnostic {
                    diagnostics.push(diagnostic);
                }
                break;
            }
            Err(error) => {
                diagnostics.push(diagnostic_from_frame_error(error, games));
                break;
            }
        }

        if diagnostics.len() >= options.max_errors {
            error_limit_reached = true;
            break;
        }
    }

    let bytes = reader.bytes_read();
    let elapsed = started.elapsed().as_secs_f64();
    let status = report_status(&diagnostics);
    #[allow(clippy::cast_precision_loss)]
    let throughput = if elapsed > 0.0 {
        bytes as f64 / (1024.0 * 1024.0) / elapsed
    } else {
        0.0
    };
    let diagnostic_count = diagnostics.len();
    let mut diagnostics = diagnostics.into_iter();
    let diagnostic = diagnostics.next();

    Report {
        schema_version: 1,
        status,
        source,
        mode: options.mode.label(),
        outcome_required: options.require_outcome,
        bytes,
        games,
        moves,
        elapsed_seconds: elapsed,
        throughput_mib_per_second: throughput,
        diagnostic,
        additional_diagnostics: diagnostics.collect(),
        diagnostic_count,
        error_limit_reached,
    }
}

struct FramedGameResult {
    moves: u64,
    diagnostic: Option<Diagnostic>,
}

#[allow(clippy::too_many_arguments)]
fn inspect_framed_game(
    game: &[u8],
    game_number: u64,
    base_offset: u64,
    base_line: u64,
    base_column: u64,
    options: DoctorOptions,
    framed_with_outcome: bool,
) -> FramedGameResult {
    let parser_options = if options.require_outcome || framed_with_outcome {
        ParserOptions::STRICT
    } else {
        ParserOptions::LENIENT
    };
    let tracker = Rc::new(RefCell::new(SourceTracker::at(
        base_offset,
        base_line,
        base_column,
    )));
    let mut validator = Validator::new(
        options.mode,
        Rc::clone(&tracker),
        game_number - 1,
        base_offset,
    );
    let mut fed = 0;
    let mut parse_diagnostic = None;

    for event in Parser::with_options(game, parser_options) {
        match event {
            Ok(event) => {
                feed_event_context(game, event, &tracker, &mut fed);
                validator.observe(event);
                if validator.error.is_some() {
                    break;
                }
            }
            Err(error) => {
                feed_line_context(game, error.offset, &tracker, &mut fed);
                parse_diagnostic = Some(diagnostic_from_parse_error(
                    error,
                    game_number,
                    &validator.headers,
                    &tracker.borrow(),
                    base_offset,
                ));
                break;
            }
        }
    }

    FramedGameResult {
        moves: validator.moves,
        diagnostic: earliest_diagnostic(validator.error, parse_diagnostic),
    }
}

fn feed_event_context(
    input: &[u8],
    event: Event<'_>,
    tracker: &Rc<RefCell<SourceTracker>>,
    fed: &mut usize,
) {
    let offset = match event {
        Event::GameStart { offset }
        | Event::MovetextStart { offset }
        | Event::GameEnd { offset } => offset,
        Event::Tag(tag) => tag.span().start,
        Event::San(token) => token.span().start,
        Event::MoveNumber { span, .. }
        | Event::VariationStart(span)
        | Event::VariationEnd(span)
        | Event::Outcome { span, .. } => span.start,
        Event::Nag(_) | Event::Comment(_) => return,
    };
    feed_line_context(input, offset, tracker, fed);
}

fn feed_line_context(
    input: &[u8],
    offset: usize,
    tracker: &Rc<RefCell<SourceTracker>>,
    fed: &mut usize,
) {
    if offset < *fed {
        return;
    }
    let end = input[offset.min(input.len())..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(input.len(), |newline| offset.min(input.len()) + newline + 1);
    tracker.borrow_mut().append(&input[*fed..end]);
    *fed = end;
}

fn diagnostic_from_parse_error(
    error: ParseError,
    game: u64,
    headers: &GameHeaders,
    tracker: &SourceTracker,
    base_offset: u64,
) -> Diagnostic {
    let offset = base_offset + error.offset as u64;
    let global_error = ParseError {
        offset: usize::try_from(offset).unwrap_or(usize::MAX),
        kind: error.kind,
    };
    located_diagnostic(
        DiagnosticDetails {
            category: DiagnosticCategory::Syntax,
            game,
            ply: None,
            offset,
            context: None,
            message: global_error.to_string(),
        },
        headers,
        tracker,
    )
}

fn diagnostic_from_frame_error(error: FrameError, completed_games: u64) -> Diagnostic {
    match error {
        FrameError::Io(error) => Diagnostic {
            category: DiagnosticCategory::Input,
            game: Some(completed_games + 1),
            ply: None,
            byte: None,
            line: None,
            column: None,
            context: None,
            excerpt: None,
            headers: None,
            message: format!("failed to read PGN stream: {error}"),
        },
        FrameError::GameTooLarge { offset, limit } => Diagnostic {
            category: DiagnosticCategory::Limit,
            game: Some(completed_games + 1),
            ply: None,
            byte: Some(offset),
            line: None,
            column: None,
            context: None,
            excerpt: None,
            headers: None,
            message: format!("PGN game starting near byte {offset} exceeds the {limit}-byte limit"),
        },
        FrameError::MissingOutcome { offset, buffered } => Diagnostic {
            category: DiagnosticCategory::Syntax,
            game: Some(completed_games + 1),
            ply: None,
            byte: Some(offset),
            line: None,
            column: None,
            context: None,
            excerpt: None,
            headers: None,
            message: format!(
                "PGN stream ended near byte {offset} with {buffered} unterminated game bytes"
            ),
        },
    }
}

fn report_status(diagnostics: &[Diagnostic]) -> ReportStatus {
    if diagnostics.is_empty() {
        ReportStatus::Valid
    } else if diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.category,
            DiagnosticCategory::Input | DiagnosticCategory::Limit
        )
    }) {
        ReportStatus::Error
    } else {
        ReportStatus::Invalid
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
    fn at(base_offset: u64, base_line: u64, base_column: u64) -> Self {
        Self {
            bytes: Vec::with_capacity(SOURCE_CONTEXT_BYTES),
            base_offset,
            base_line,
            base_column,
        }
    }

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
    fen: Option<TagValue>,
    setup: Option<TagValue>,
    declared_result: Option<TagValue>,
    games: u64,
    moves: u64,
    game_ply: u64,
    headers: GameHeaders,
    error: Option<Diagnostic>,
    offset_base: u64,
}

struct TagValue {
    value: Vec<u8>,
    offset: usize,
}

impl Validator {
    fn new(
        mode: ValidationMode,
        tracker: Rc<RefCell<SourceTracker>>,
        completed_games: u64,
        offset_base: u64,
    ) -> Self {
        Self {
            mode,
            tracker,
            position: Position::initial(),
            before_last_move: Position::initial(),
            variation_stack: Vec::new(),
            fen: None,
            setup: None,
            declared_result: None,
            games: completed_games,
            moves: 0,
            game_ply: 0,
            headers: GameHeaders::default(),
            error: None,
            offset_base,
        }
    }

    fn observe(&mut self, event: Event<'_>) {
        if self.error.is_some() {
            return;
        }
        match event {
            Event::GameStart { .. } => self.start_game(),
            Event::Tag(tag) => self.record_tag(tag),
            Event::MovetextStart { .. } if self.mode == ValidationMode::Semantic => {
                self.start_movetext();
            }
            Event::MoveNumber { number, dots, span } if self.mode == ValidationMode::Semantic => {
                self.validate_move_number(number, dots, span.start);
            }
            Event::San(token) => self.validate_san(token),
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
            Event::Outcome { outcome, span } if self.mode == ValidationMode::Semantic => {
                if self.variation_stack.is_empty() {
                    self.validate_outcome(outcome, span.start);
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

    fn start_game(&mut self) {
        self.position = Position::initial();
        self.before_last_move = self.position;
        self.variation_stack.clear();
        self.fen = None;
        self.setup = None;
        self.declared_result = None;
        self.game_ply = 0;
        self.headers = GameHeaders::default();
    }

    fn start_movetext(&mut self) {
        if let Some(fen) = &self.fen {
            match Position::from_fen(&fen.value) {
                Ok(position) => self.position = position,
                Err(error) => {
                    self.error = Some(located_diagnostic(
                        DiagnosticDetails {
                            category: DiagnosticCategory::InvalidFen,
                            game: self.games + 1,
                            ply: Some(0),
                            offset: self.offset_base + fen.offset as u64,
                            context: Some(String::from_utf8_lossy(&fen.value).into_owned()),
                            message: error.to_string(),
                        },
                        &self.headers,
                        &self.tracker.borrow(),
                    ));
                    return;
                }
            }
        }
        if let Some(diagnostic) = self.validate_metadata() {
            self.error = Some(diagnostic);
            return;
        }
        self.before_last_move = self.position;
    }

    fn validate_san(&mut self, token: Token<'_>) {
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
                    SanErrorKind::InvalidCheckSuffix => DiagnosticCategory::InvalidCheckSuffix,
                };
                self.error = Some(located_diagnostic(
                    DiagnosticDetails {
                        category,
                        game: self.games + 1,
                        ply: Some(self.game_ply),
                        offset: self.offset_base + token.span().start as u64,
                        context: Some(String::from_utf8_lossy(token.as_bytes()).into_owned()),
                        message: error.to_string(),
                    },
                    &self.headers,
                    &self.tracker.borrow(),
                ));
            }
        }
    }

    fn validate_metadata(&self) -> Option<Diagnostic> {
        let setup_issue = match (&self.setup, &self.fen) {
            (Some(setup), _) if !matches!(setup.value.as_slice(), b"0" | b"1") => Some((
                setup,
                format!(
                    "SetUp tag must be \"0\" or \"1\", found {:?}",
                    String::from_utf8_lossy(&setup.value)
                ),
            )),
            (Some(setup), None) if setup.value == b"1" => Some((
                setup,
                String::from("SetUp tag is \"1\" but the game has no FEN tag"),
            )),
            (Some(setup), Some(_)) if setup.value != b"1" => Some((
                setup,
                String::from("a game with a FEN tag must have SetUp set to \"1\""),
            )),
            (None, Some(fen)) => Some((
                fen,
                String::from("a game with a FEN tag must also have SetUp set to \"1\""),
            )),
            _ => None,
        };
        if let Some((tag, message)) = setup_issue {
            return Some(self.tag_diagnostic(DiagnosticCategory::InconsistentSetup, tag, message));
        }

        self.declared_result.as_ref().and_then(|result| {
            (!is_result_value(&result.value)).then(|| {
                self.tag_diagnostic(
                    DiagnosticCategory::InconsistentResult,
                    result,
                    format!(
                        "Result tag must be one of \"1-0\", \"0-1\", \"1/2-1/2\", or \"*\", found {:?}",
                        String::from_utf8_lossy(&result.value)
                    ),
                )
            })
        })
    }

    fn validate_move_number(&mut self, number: u32, dots: u8, offset: usize) {
        let expected_number = u32::from(self.position.fullmove_number());
        let (expected_dots, side) = match self.position.side_to_move() {
            Color::White => (1, "White"),
            Color::Black => (3, "Black"),
        };
        if number == expected_number && dots == expected_dots {
            return;
        }
        let actual = format!("{number}{}", ".".repeat(usize::from(dots)));
        let expected = format!(
            "{expected_number}{}",
            ".".repeat(usize::from(expected_dots))
        );
        self.error = Some(located_diagnostic(
            DiagnosticDetails {
                category: DiagnosticCategory::IncorrectMoveNumber,
                game: self.games + 1,
                ply: Some(self.game_ply + 1),
                offset: self.offset_base + offset as u64,
                context: Some(actual.clone()),
                message: format!(
                    "move number {actual} does not match the position; expected {expected} for {side} to move"
                ),
            },
            &self.headers,
            &self.tracker.borrow(),
        ));
    }

    fn validate_outcome(&mut self, outcome: Outcome, offset: usize) {
        let Some(result) = &self.declared_result else {
            return;
        };
        let marker = outcome_value(outcome);
        if result.value == marker {
            return;
        }
        let declared = String::from_utf8_lossy(&result.value);
        let marker = String::from_utf8_lossy(marker);
        self.error = Some(located_diagnostic(
            DiagnosticDetails {
                category: DiagnosticCategory::InconsistentResult,
                game: self.games + 1,
                ply: None,
                offset: self.offset_base + offset as u64,
                context: Some(format!("Result={declared:?}, movetext={marker:?}")),
                message: format!(
                    "Result tag {declared:?} does not match movetext outcome {marker:?}"
                ),
            },
            &self.headers,
            &self.tracker.borrow(),
        ));
    }

    fn tag_diagnostic(
        &self,
        category: DiagnosticCategory,
        tag: &TagValue,
        message: String,
    ) -> Diagnostic {
        located_diagnostic(
            DiagnosticDetails {
                category,
                game: self.games + 1,
                ply: Some(0),
                offset: self.offset_base + tag.offset as u64,
                context: Some(String::from_utf8_lossy(&tag.value).into_owned()),
                message,
            },
            &self.headers,
            &self.tracker.borrow(),
        )
    }

    fn record_tag(&mut self, tag: Tag<'_>) {
        let name = tag.name();
        if self.mode == ValidationMode::Semantic {
            let destination = match name {
                b"FEN" => Some(&mut self.fen),
                b"SetUp" => Some(&mut self.setup),
                b"Result" => Some(&mut self.declared_result),
                _ => None,
            };
            if let Some(destination) = destination {
                *destination = Some(TagValue {
                    value: tag.value().into_owned(),
                    offset: tag.span().start,
                });
                return;
            }
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

fn is_result_value(value: &[u8]) -> bool {
    matches!(value, b"1-0" | b"0-1" | b"1/2-1/2" | b"*")
}

const fn outcome_value(outcome: Outcome) -> &'static [u8] {
    match outcome {
        Outcome::WhiteWins => b"1-0",
        Outcome::BlackWins => b"0-1",
        Outcome::Draw => b"1/2-1/2",
        Outcome::Unknown => b"*",
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
                max_errors: 1,
            },
        )
    }

    fn inspect_all(input: &[u8], max_errors: usize) -> Report {
        inspect(
            input,
            String::from("test"),
            DoctorOptions {
                mode: ValidationMode::Semantic,
                require_outcome: true,
                max_errors,
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

    #[test]
    fn complete_scan_recovers_at_game_boundaries() {
        let report = inspect_all(b"1. e5 *\n\n1. d5 *\n\n1. e4 e5 *\n", 100);
        assert_eq!(report.status, ReportStatus::Invalid);
        assert_eq!(report.games, 3);
        assert_eq!(report.moves, 2);
        assert_eq!(report.diagnostic_count, 2);
        let diagnostics = report.diagnostics().collect::<Vec<_>>();
        assert_eq!(diagnostics[0].game, Some(1));
        assert_eq!(diagnostics[0].line, Some(1));
        assert_eq!(diagnostics[1].game, Some(2));
        assert_eq!(diagnostics[1].line, Some(3));
        assert!(!report.error_limit_reached);
    }

    #[test]
    fn complete_scan_stops_at_the_error_limit() {
        let report = inspect_all(b"1. e5 *\n1. d5 *\n1. c5 *\n", 2);
        assert_eq!(report.games, 2);
        assert_eq!(report.diagnostic_count, 2);
        assert!(report.error_limit_reached);
    }

    #[test]
    fn rejects_a_result_that_disagrees_with_movetext() {
        let report = inspect_bytes(
            b"[Event \"Example\"]\n[Result \"1-0\"]\n\n1. e4 0-1\n",
            ValidationMode::Semantic,
        );
        let diagnostic = report.diagnostic.unwrap();
        assert_eq!(diagnostic.category, DiagnosticCategory::InconsistentResult);
        assert_eq!(diagnostic.game, Some(1));
        assert_eq!(diagnostic.line, Some(4));
        assert_eq!(
            diagnostic.context.as_deref(),
            Some("Result=\"1-0\", movetext=\"0-1\"")
        );
    }

    #[test]
    fn rejects_an_unknown_result_tag_value() {
        let report = inspect_bytes(b"[Result \"win\"]\n\n*\n", ValidationMode::Semantic);
        let diagnostic = report.diagnostic.unwrap();
        assert_eq!(diagnostic.category, DiagnosticCategory::InconsistentResult);
        assert_eq!(diagnostic.line, Some(1));
        assert_eq!(diagnostic.context.as_deref(), Some("win"));
    }

    #[test]
    fn requires_setup_for_a_fen_start() {
        let report = inspect_bytes(
            b"[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\"]\n\n*\n",
            ValidationMode::Semantic,
        );
        assert_eq!(
            report.diagnostic.unwrap().category,
            DiagnosticCategory::InconsistentSetup
        );
    }

    #[test]
    fn requires_fen_when_setup_is_enabled() {
        let report = inspect_bytes(b"[SetUp \"1\"]\n\n*\n", ValidationMode::Semantic);
        assert_eq!(
            report.diagnostic.unwrap().category,
            DiagnosticCategory::InconsistentSetup
        );
    }

    #[test]
    fn validates_move_numbers_against_the_position() {
        let report = inspect_bytes(b"1. e4 e5 3. Nf3 *\n", ValidationMode::Semantic);
        let diagnostic = report.diagnostic.unwrap();
        assert_eq!(diagnostic.category, DiagnosticCategory::IncorrectMoveNumber);
        assert_eq!(diagnostic.ply, Some(3));
        assert_eq!(diagnostic.context.as_deref(), Some("3."));
        assert!(diagnostic.message.contains("expected 2."));
    }

    #[test]
    fn validates_move_number_dots_against_the_side_to_move() {
        let report = inspect_bytes(b"1... e4 *\n", ValidationMode::Semantic);
        let diagnostic = report.diagnostic.unwrap();
        assert_eq!(diagnostic.category, DiagnosticCategory::IncorrectMoveNumber);
        assert!(diagnostic.message.contains("expected 1. for White"));
    }

    #[test]
    fn fen_move_numbers_and_variations_follow_live_positions() {
        let fen_game = inspect_bytes(
            b"[SetUp \"1\"]\n[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 42\"]\n\n42... e5 *\n",
            ValidationMode::Semantic,
        );
        assert_eq!(fen_game.status, ReportStatus::Valid);

        let variation = inspect_bytes(b"1. e4 e5 (1... c5) 2. Nf3 *\n", ValidationMode::Semantic);
        assert_eq!(variation.status, ReportStatus::Valid);
    }

    #[test]
    fn result_check_ignores_outcomes_inside_variations() {
        let report = inspect_bytes(
            b"[Result \"1-0\"]\n\n1. e4 e5 (1... c5 *) 2. Nf3 1-0\n",
            ValidationMode::Semantic,
        );
        assert_eq!(report.status, ReportStatus::Valid);
    }

    #[test]
    fn syntax_mode_ignores_cross_field_consistency() {
        let report = inspect_bytes(
            b"[SetUp \"1\"]\n[Result \"1-0\"]\n\n9... e5 0-1\n",
            ValidationMode::Syntax,
        );
        assert_eq!(report.status, ReportStatus::Valid);
    }
}
