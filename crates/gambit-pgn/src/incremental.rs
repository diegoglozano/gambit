use std::error::Error;
use std::fmt;
use std::io::{self, Read};

use crate::parser::is_san_boundary;
use crate::{
    Comment, CommentKind, ErrorKind, Event, Nag, Outcome, ParseError, ParserOptions, Span, Tag,
    Token,
};

const DEFAULT_BUFFER_SIZE: usize = 64 * 1024;
const DEFAULT_MAX_TOKEN_BYTES: usize = 16 * 1024 * 1024;
const UTF8_BOM: &[u8] = &[0xef, 0xbb, 0xbf];

/// Configuration for [`IncrementalParser`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IncrementalParserOptions {
    /// Lexical parser behavior.
    pub parser: ParserOptions,
    /// Number of bytes requested from the reader at once.
    pub buffer_size: usize,
    /// Maximum retained bytes for one token, tag, or comment crossing chunks.
    pub max_token_bytes: usize,
}

impl Default for IncrementalParserOptions {
    fn default() -> Self {
        Self {
            parser: ParserOptions::default(),
            buffer_size: DEFAULT_BUFFER_SIZE,
            max_token_bytes: DEFAULT_MAX_TOKEN_BYTES,
        }
    }
}

/// Aggregate I/O information from one incremental parse.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamStats {
    /// Total bytes obtained from the input reader.
    pub bytes_read: u64,
    /// Largest number of bytes retained in the reusable parsing buffer.
    pub maximum_buffered_bytes: usize,
}

/// An error from incremental PGN parsing.
#[derive(Debug)]
pub enum StreamParseError {
    Io(io::Error),
    Parse(ParseError),
    TokenTooLarge { offset: u64, limit: usize },
}

impl fmt::Display for StreamParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "failed to read PGN stream: {error}"),
            Self::Parse(error) => error.fmt(f),
            Self::TokenTooLarge { offset, limit } => write!(
                f,
                "PGN token at byte {offset} exceeds the {limit}-byte streaming limit"
            ),
        }
    }
}

impl Error for StreamParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::TokenTooLarge { .. } => None,
        }
    }
}

impl From<io::Error> for StreamParseError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ParseError> for StreamParseError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    BetweenGames,
    Headers,
    MovetextStart,
    Movetext,
    GameEnd,
    Done,
}

enum Step<'a> {
    Event(Event<'a>),
    NeedMore,
    Done,
    Error(ParseError),
}

#[derive(Clone, Copy)]
enum Attempt<'a> {
    Complete(Event<'a>, usize),
    NeedMore,
    NotApplicable,
    Error(ParseError),
}

/// A single-pass, bounded-memory parser over any [`Read`] source.
///
/// Events borrow the internal input buffer and are valid only for the duration
/// of the visitor call. Parsing and game-boundary detection share one lexical
/// state machine, so every input byte is classified only once.
#[derive(Debug)]
pub struct IncrementalParser<R> {
    reader: R,
    options: IncrementalParserOptions,
    buffer: Vec<u8>,
    read_buffer: Vec<u8>,
    cursor: usize,
    base_offset: u64,
    state: State,
    variation_depth: u32,
    line_start: bool,
    bom_checked: bool,
    eof: bool,
    stats: StreamStats,
}

impl<R: Read> IncrementalParser<R> {
    #[must_use]
    pub fn new(reader: R) -> Self {
        Self::with_options(reader, IncrementalParserOptions::default())
    }

    #[must_use]
    pub fn with_options(reader: R, options: IncrementalParserOptions) -> Self {
        let buffer_size = options.buffer_size.max(1);
        Self {
            reader,
            options,
            buffer: Vec::with_capacity(buffer_size),
            read_buffer: vec![0; buffer_size],
            cursor: 0,
            base_offset: 0,
            state: State::BetweenGames,
            variation_depth: 0,
            line_start: true,
            bom_checked: false,
            eof: false,
            stats: StreamStats::default(),
        }
    }

    /// Parses the complete stream and calls `visitor` for every event.
    ///
    /// # Errors
    ///
    /// Returns an I/O error, the first lexical [`ParseError`], or
    /// [`StreamParseError::TokenTooLarge`] when an incomplete lexical item
    /// exceeds the configured memory bound.
    pub fn parse<F>(&mut self, mut visitor: F) -> Result<StreamStats, StreamParseError>
    where
        F: for<'event> FnMut(Event<'event>),
    {
        loop {
            let step = next_step(
                &self.buffer,
                &mut self.cursor,
                self.base_offset,
                &mut self.state,
                &mut self.variation_depth,
                &mut self.line_start,
                &mut self.bom_checked,
                self.eof,
                self.options.parser,
            );
            match step {
                Step::Event(event) => visitor(event),
                Step::Done => return Ok(self.stats),
                Step::Error(error) => return Err(error.into()),
                Step::NeedMore => {
                    self.compact();
                    if self.buffer.len() > self.options.max_token_bytes {
                        return Err(StreamParseError::TokenTooLarge {
                            offset: self.base_offset,
                            limit: self.options.max_token_bytes,
                        });
                    }
                    let read = self.reader.read(&mut self.read_buffer)?;
                    if read == 0 {
                        self.eof = true;
                    } else {
                        self.buffer.extend_from_slice(&self.read_buffer[..read]);
                        self.stats.bytes_read += read as u64;
                        self.stats.maximum_buffered_bytes =
                            self.stats.maximum_buffered_bytes.max(self.buffer.len());
                    }
                }
            }
        }
    }

    /// Returns the underlying reader.
    #[must_use]
    pub fn into_inner(self) -> R {
        self.reader
    }

    fn compact(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let remaining = self.buffer.len() - self.cursor;
        self.buffer.copy_within(self.cursor.., 0);
        self.buffer.truncate(remaining);
        self.base_offset += self.cursor as u64;
        self.cursor = 0;
    }
}

#[allow(clippy::too_many_arguments)]
fn next_step<'a>(
    input: &'a [u8],
    cursor: &mut usize,
    base: u64,
    state: &mut State,
    variation_depth: &mut u32,
    line_start: &mut bool,
    bom_checked: &mut bool,
    eof: bool,
    options: ParserOptions,
) -> Step<'a> {
    loop {
        if !*bom_checked {
            let remaining = &input[*cursor..];
            if remaining.starts_with(UTF8_BOM) {
                *cursor += UTF8_BOM.len();
                *bom_checked = true;
                *line_start = false;
            } else if !eof && remaining.len() < UTF8_BOM.len() && UTF8_BOM.starts_with(remaining) {
                return Step::NeedMore;
            } else {
                *bom_checked = true;
            }
        }

        match *state {
            State::Done => return Step::Done,
            State::BetweenGames => {
                skip_whitespace(input, cursor, line_start);
                if *cursor == input.len() {
                    if eof {
                        *state = State::Done;
                        return Step::Done;
                    }
                    return Step::NeedMore;
                }
                let offset = absolute(base, *cursor);
                *variation_depth = 0;
                *state = if input[*cursor] == b'[' {
                    State::Headers
                } else {
                    State::MovetextStart
                };
                return Step::Event(Event::GameStart { offset });
            }
            State::Headers => {
                skip_whitespace(input, cursor, line_start);
                if *cursor == input.len() && !eof {
                    return Step::NeedMore;
                }
                if input.get(*cursor) == Some(&b'[') {
                    let attempt = parse_tag(input, *cursor, base, eof);
                    return commit_attempt(attempt, cursor, line_start, state);
                }
                *state = State::MovetextStart;
            }
            State::MovetextStart => {
                skip_whitespace(input, cursor, line_start);
                if *cursor == input.len() && !eof {
                    return Step::NeedMore;
                }
                *state = State::Movetext;
                return Step::Event(Event::MovetextStart {
                    offset: absolute(base, *cursor),
                });
            }
            State::GameEnd => {
                *state = State::BetweenGames;
                return Step::Event(Event::GameEnd {
                    offset: absolute(base, *cursor),
                });
            }
            State::Movetext => {
                if let Some(step) = next_movetext(
                    input,
                    cursor,
                    base,
                    state,
                    variation_depth,
                    line_start,
                    eof,
                    options,
                ) {
                    return step;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn next_movetext<'a>(
    input: &'a [u8],
    cursor: &mut usize,
    base: u64,
    state: &mut State,
    variation_depth: &mut u32,
    line_start: &mut bool,
    eof: bool,
    options: ParserOptions,
) -> Option<Step<'a>> {
    skip_whitespace(input, cursor, line_start);
    if *cursor == input.len() {
        if !eof {
            return Some(Step::NeedMore);
        }
        if *variation_depth != 0 {
            return Some(fail(
                base,
                *cursor,
                ErrorKind::UnclosedVariation {
                    depth: *variation_depth,
                },
                state,
            ));
        }
        if options.require_outcome {
            return Some(fail(base, *cursor, ErrorKind::MissingOutcome, state));
        }
        *state = State::Done;
        return Some(Step::Event(Event::GameEnd {
            offset: absolute(base, *cursor),
        }));
    }

    let start = *cursor;
    let byte = input[start];
    if byte == b'[' {
        if !options.require_outcome && *variation_depth == 0 {
            *state = State::GameEnd;
            return None;
        }
        return Some(fail(base, start, ErrorKind::UnexpectedHeader, state));
    }
    if byte == b'{' {
        let attempt = parse_comment(input, start, base, eof, CommentKind::Brace);
        return Some(commit_attempt(attempt, cursor, line_start, state));
    }
    if byte == b';' {
        let attempt = parse_comment(input, start, base, eof, CommentKind::Line);
        return Some(commit_attempt(attempt, cursor, line_start, state));
    }
    if byte == b'%' && *line_start {
        let attempt = parse_comment(input, start, base, eof, CommentKind::EscapeLine);
        return Some(commit_attempt(attempt, cursor, line_start, state));
    }
    if byte == b'(' {
        *cursor += 1;
        *line_start = false;
        *variation_depth = variation_depth.saturating_add(1);
        return Some(Step::Event(Event::VariationStart(span(
            base, start, *cursor,
        ))));
    }
    if byte == b')' {
        if *variation_depth == 0 {
            return Some(fail(base, start, ErrorKind::UnmatchedVariationEnd, state));
        }
        *cursor += 1;
        *line_start = false;
        *variation_depth -= 1;
        return Some(Step::Event(Event::VariationEnd(span(base, start, *cursor))));
    }
    if byte == b'$' {
        let attempt = parse_numeric_nag(input, start, base, eof);
        return Some(commit_attempt(attempt, cursor, line_start, state));
    }
    if matches!(byte, b'!' | b'?') {
        let attempt = parse_glyph_nag(input, start, base, eof);
        return Some(commit_attempt(attempt, cursor, line_start, state));
    }
    if matches!(byte, b'0' | b'1' | b'*') {
        match parse_outcome(input, start, base, eof) {
            Attempt::Complete(event, end) => {
                *cursor = end;
                *line_start = false;
                if *variation_depth == 0 {
                    *state = State::GameEnd;
                }
                return Some(Step::Event(event));
            }
            Attempt::NeedMore => return Some(Step::NeedMore),
            Attempt::Error(error) => return Some(stop(error, state)),
            Attempt::NotApplicable => {}
        }
    }
    if byte.is_ascii_digit() {
        let attempt = parse_move_number(input, start, base, eof);
        match attempt {
            Attempt::Complete(_, _) | Attempt::NeedMore | Attempt::Error(_) => {
                return Some(commit_attempt(attempt, cursor, line_start, state));
            }
            Attempt::NotApplicable => {}
        }
    }
    let attempt = parse_san(input, start, base, eof);
    Some(commit_attempt(attempt, cursor, line_start, state))
}

fn commit_attempt<'a>(
    attempt: Attempt<'a>,
    cursor: &mut usize,
    line_start: &mut bool,
    state: &mut State,
) -> Step<'a> {
    match attempt {
        Attempt::Complete(event, end) => {
            *cursor = end;
            *line_start = false;
            Step::Event(event)
        }
        Attempt::NeedMore => Step::NeedMore,
        Attempt::NotApplicable => unreachable!("attempt must apply in this parser state"),
        Attempt::Error(error) => stop(error, state),
    }
}

fn stop<'a>(error: ParseError, state: &mut State) -> Step<'a> {
    *state = State::Done;
    Step::Error(error)
}

fn fail<'a>(base: u64, local_offset: usize, kind: ErrorKind, state: &mut State) -> Step<'a> {
    stop(error(base, local_offset, kind), state)
}

fn skip_whitespace(input: &[u8], cursor: &mut usize, line_start: &mut bool) {
    while let Some(byte) = input.get(*cursor) {
        if !byte.is_ascii_whitespace() {
            break;
        }
        *line_start = *byte == b'\n';
        *cursor += 1;
    }
}

fn parse_tag(input: &[u8], start: usize, base: u64, eof: bool) -> Attempt<'_> {
    let mut cursor = start + 1;
    match skip_horizontal(input, &mut cursor, base) {
        Ok(()) => {}
        Err(error) => return Attempt::Error(error),
    }
    if cursor == input.len() && !eof {
        return Attempt::NeedMore;
    }

    let name_start = cursor;
    while input
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        cursor += 1;
    }
    if cursor == input.len() && !eof {
        return Attempt::NeedMore;
    }
    if cursor == name_start {
        return Attempt::Error(error(base, cursor, ErrorKind::ExpectedTagName));
    }
    let name_end = cursor;
    match skip_horizontal(input, &mut cursor, base) {
        Ok(()) => {}
        Err(error) => return Attempt::Error(error),
    }
    if cursor == input.len() && !eof {
        return Attempt::NeedMore;
    }
    if input.get(cursor) != Some(&b'"') {
        return Attempt::Error(error(base, cursor, ErrorKind::ExpectedTagValue));
    }
    cursor += 1;
    let value_start = cursor;
    loop {
        match input.get(cursor) {
            Some(b'"') => break,
            Some(b'\\') => {
                cursor += 1;
                match input.get(cursor) {
                    Some(b'\r' | b'\n') => {
                        return Attempt::Error(error(base, cursor, ErrorKind::NewlineInTag));
                    }
                    Some(_) => cursor += 1,
                    None if !eof => return Attempt::NeedMore,
                    None => {
                        return Attempt::Error(error(
                            base,
                            cursor,
                            ErrorKind::UnterminatedTagValue,
                        ));
                    }
                }
            }
            Some(b'\r' | b'\n') => {
                return Attempt::Error(error(base, cursor, ErrorKind::NewlineInTag));
            }
            Some(_) => cursor += 1,
            None if !eof => return Attempt::NeedMore,
            None => {
                return Attempt::Error(error(base, cursor, ErrorKind::UnterminatedTagValue));
            }
        }
    }
    let value_end = cursor;
    cursor += 1;
    match skip_horizontal(input, &mut cursor, base) {
        Ok(()) => {}
        Err(error) => return Attempt::Error(error),
    }
    if cursor == input.len() && !eof {
        return Attempt::NeedMore;
    }
    if input.get(cursor) != Some(&b']') {
        return Attempt::Error(error(base, cursor, ErrorKind::ExpectedClosingBracket));
    }
    cursor += 1;

    Attempt::Complete(
        Event::Tag(Tag {
            name: &input[name_start..name_end],
            value: &input[value_start..value_end],
            span: span(base, start, cursor),
        }),
        cursor,
    )
}

fn skip_horizontal(input: &[u8], cursor: &mut usize, base: u64) -> Result<(), ParseError> {
    while matches!(input.get(*cursor), Some(b' ' | b'\t')) {
        *cursor += 1;
    }
    if matches!(input.get(*cursor), Some(b'\r' | b'\n')) {
        return Err(error(base, *cursor, ErrorKind::NewlineInTag));
    }
    Ok(())
}

fn parse_comment(
    input: &[u8],
    start: usize,
    base: u64,
    eof: bool,
    kind: CommentKind,
) -> Attempt<'_> {
    let text_start = start + 1;
    match kind {
        CommentKind::Brace => {
            let Some(relative_end) = input[text_start..].iter().position(|byte| *byte == b'}')
            else {
                return if eof {
                    Attempt::Error(error(base, start, ErrorKind::UnterminatedComment))
                } else {
                    Attempt::NeedMore
                };
            };
            let text_end = text_start + relative_end;
            let end = text_end + 1;
            Attempt::Complete(
                Event::Comment(Comment {
                    kind,
                    text: &input[text_start..text_end],
                    span: span(base, start, end),
                }),
                end,
            )
        }
        CommentKind::Line | CommentKind::EscapeLine => {
            let relative_end = input[text_start..]
                .iter()
                .position(|byte| matches!(*byte, b'\r' | b'\n'));
            let Some(relative_end) = relative_end else {
                if !eof {
                    return Attempt::NeedMore;
                }
                let end = input.len();
                return Attempt::Complete(
                    Event::Comment(Comment {
                        kind,
                        text: &input[text_start..end],
                        span: span(base, start, end),
                    }),
                    end,
                );
            };
            let end = text_start + relative_end;
            Attempt::Complete(
                Event::Comment(Comment {
                    kind,
                    text: &input[text_start..end],
                    span: span(base, start, end),
                }),
                end,
            )
        }
    }
}

fn parse_numeric_nag(input: &[u8], start: usize, base: u64, eof: bool) -> Attempt<'_> {
    let digits_start = start + 1;
    let mut end = digits_start;
    while input.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == input.len() && !eof {
        return Attempt::NeedMore;
    }
    if end == digits_start {
        return Attempt::Error(error(base, start, ErrorKind::InvalidNag));
    }
    let Some(value) = parse_decimal(&input[digits_start..end]) else {
        return Attempt::Error(error(base, start, ErrorKind::InvalidNag));
    };
    Attempt::Complete(
        Event::Nag(Nag::Numeric {
            value,
            span: span(base, start, end),
        }),
        end,
    )
}

fn parse_glyph_nag(input: &[u8], start: usize, base: u64, eof: bool) -> Attempt<'_> {
    let mut end = start;
    while matches!(input.get(end), Some(b'!' | b'?')) {
        end += 1;
    }
    if end == input.len() && !eof {
        return Attempt::NeedMore;
    }
    Attempt::Complete(
        Event::Nag(Nag::Glyph(Token {
            bytes: &input[start..end],
            span: span(base, start, end),
        })),
        end,
    )
}

fn parse_move_number(input: &[u8], start: usize, base: u64, eof: bool) -> Attempt<'_> {
    let mut digits_end = start;
    while input.get(digits_end).is_some_and(u8::is_ascii_digit) {
        digits_end += 1;
    }
    if digits_end == input.len() && !eof {
        return Attempt::NeedMore;
    }
    let mut end = digits_end;
    while input.get(end) == Some(&b'.') {
        end += 1;
    }
    if end == input.len() && !eof {
        return Attempt::NeedMore;
    }
    let dots = end - digits_end;
    if dots == 0 {
        return Attempt::NotApplicable;
    }
    if !matches!(dots, 1 | 3) {
        return Attempt::Error(error(base, start, ErrorKind::InvalidMoveNumber));
    }
    let Some(number) = parse_decimal(&input[start..digits_end]) else {
        return Attempt::Error(error(base, start, ErrorKind::InvalidMoveNumber));
    };
    Attempt::Complete(
        Event::MoveNumber {
            number,
            dots: u8::try_from(dots).expect("at most three dots"),
            span: span(base, start, end),
        },
        end,
    )
}

fn parse_san(input: &[u8], start: usize, base: u64, eof: bool) -> Attempt<'_> {
    let mut end = start;
    while input.get(end).is_some_and(|byte| !is_san_boundary(*byte)) {
        end += 1;
    }
    if end == input.len() && !eof {
        return Attempt::NeedMore;
    }
    if end == start {
        return Attempt::Error(error(base, start, ErrorKind::UnexpectedByte(input[start])));
    }
    Attempt::Complete(
        Event::San(Token {
            bytes: &input[start..end],
            span: span(base, start, end),
        }),
        end,
    )
}

fn parse_outcome(input: &[u8], start: usize, base: u64, eof: bool) -> Attempt<'_> {
    const OUTCOMES: [(&[u8], Outcome); 4] = [
        (b"1/2-1/2", Outcome::Draw),
        (b"1-0", Outcome::WhiteWins),
        (b"0-1", Outcome::BlackWins),
        (b"*", Outcome::Unknown),
    ];
    let remaining = &input[start..];
    let mut possible_prefix = false;
    for (marker, outcome) in OUTCOMES {
        if remaining.len() < marker.len() {
            possible_prefix |= marker.starts_with(remaining);
            continue;
        }
        if !remaining.starts_with(marker) {
            continue;
        }
        let end = start + marker.len();
        return match input.get(end) {
            Some(byte) if is_token_boundary(*byte) => Attempt::Complete(
                Event::Outcome {
                    outcome,
                    span: span(base, start, end),
                },
                end,
            ),
            Some(_) => Attempt::NotApplicable,
            None if eof => Attempt::Complete(
                Event::Outcome {
                    outcome,
                    span: span(base, start, end),
                },
                end,
            ),
            None => Attempt::NeedMore,
        };
    }
    if possible_prefix && !eof {
        Attempt::NeedMore
    } else {
        Attempt::NotApplicable
    }
}

fn parse_decimal(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        value
            .checked_mul(10)?
            .checked_add(u32::from(byte.checked_sub(b'0')?))
    })
}

fn is_token_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(byte, b'{' | b';' | b'(' | b')' | b'$' | b'!' | b'?' | b'[')
}

fn span(base: u64, start: usize, end: usize) -> Span {
    Span {
        start: absolute(base, start),
        end: absolute(base, end),
    }
}

fn error(base: u64, local: usize, kind: ErrorKind) -> ParseError {
    ParseError {
        offset: absolute(base, local),
        kind,
    }
}

fn absolute(base: u64, local: usize) -> usize {
    usize::try_from(base + local as u64).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::Parser;

    #[derive(Debug, Eq, PartialEq)]
    enum OwnedEvent {
        GameStart(usize),
        Tag(Vec<u8>, Vec<u8>, Span),
        MovetextStart(usize),
        MoveNumber(u32, u8, Span),
        San(Vec<u8>, Span),
        NumericNag(u32, Span),
        GlyphNag(Vec<u8>, Span),
        Comment(CommentKind, Vec<u8>, Span),
        VariationStart(Span),
        VariationEnd(Span),
        Outcome(Outcome, Span),
        GameEnd(usize),
    }

    fn own(event: Event<'_>) -> OwnedEvent {
        match event {
            Event::GameStart { offset } => OwnedEvent::GameStart(offset),
            Event::Tag(tag) => {
                OwnedEvent::Tag(tag.name().to_vec(), tag.raw_value().to_vec(), tag.span())
            }
            Event::MovetextStart { offset } => OwnedEvent::MovetextStart(offset),
            Event::MoveNumber { number, dots, span } => OwnedEvent::MoveNumber(number, dots, span),
            Event::San(token) => OwnedEvent::San(token.as_bytes().to_vec(), token.span()),
            Event::Nag(Nag::Numeric { value, span }) => OwnedEvent::NumericNag(value, span),
            Event::Nag(Nag::Glyph(token)) => {
                OwnedEvent::GlyphNag(token.as_bytes().to_vec(), token.span())
            }
            Event::Comment(comment) => {
                OwnedEvent::Comment(comment.kind, comment.text.to_vec(), comment.span)
            }
            Event::VariationStart(span) => OwnedEvent::VariationStart(span),
            Event::VariationEnd(span) => OwnedEvent::VariationEnd(span),
            Event::Outcome { outcome, span } => OwnedEvent::Outcome(outcome, span),
            Event::GameEnd { offset } => OwnedEvent::GameEnd(offset),
        }
    }

    fn incremental(input: &[u8], buffer_size: usize) -> Result<Vec<OwnedEvent>, StreamParseError> {
        let mut events = Vec::new();
        let mut parser = IncrementalParser::with_options(
            Cursor::new(input),
            IncrementalParserOptions {
                buffer_size,
                max_token_bytes: 1024,
                ..IncrementalParserOptions::default()
            },
        );
        parser.parse(|event| events.push(own(event)))?;
        Ok(events)
    }

    #[test]
    fn matches_slice_parser_at_every_small_chunk_size() {
        let input = br#"[Event "A \"quoted\" event"]
[Result "1-0"]

1. e4! {main 0-1} (1. d4 $14 d5?!) e5 ; line
2. Nf3 1-0

[Result "*"]
1. c4 *"#;
        let expected: Vec<_> = Parser::new(input)
            .map(|event| own(event.unwrap()))
            .collect();

        for buffer_size in 1..=32 {
            assert_eq!(incremental(input, buffer_size).unwrap(), expected);
        }
    }

    #[test]
    fn matches_slice_parser_for_arbitrary_short_inputs() {
        const ALPHABET: &[u8] = b" \n\r[{%1*$!";

        for length in 0..=4_u32 {
            let combinations = ALPHABET.len().pow(length);
            for mut encoded in 0..combinations {
                let mut input = vec![0; length as usize];
                for byte in &mut input {
                    *byte = ALPHABET[encoded % ALPHABET.len()];
                    encoded /= ALPHABET.len();
                }

                let expected: Result<Vec<_>, _> =
                    Parser::new(&input).map(|event| event.map(own)).collect();
                for buffer_size in 1..=3 {
                    let actual = incremental(&input, buffer_size);
                    match (&expected, actual) {
                        (Ok(expected), Ok(actual)) => assert_eq!(&actual, expected, "{input:?}"),
                        (Err(expected), Err(StreamParseError::Parse(actual))) => {
                            assert_eq!(&actual, expected, "{input:?}");
                        }
                        (expected, actual) => {
                            panic!("parser mismatch for {input:?}: {expected:?} != {actual:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn matches_slice_parser_errors_and_offsets() {
        let inputs: &[&[u8]] = &[
            b"1. e4",
            b"1. e4 (1... c5 *",
            b"[Event \"unterminated]",
            b"1. e4 $4294967296 *",
        ];
        for input in inputs {
            let expected = Parser::new(input)
                .find_map(Result::err)
                .expect("slice parser error");
            for buffer_size in 1..=8 {
                let error = incremental(input, buffer_size).unwrap_err();
                assert!(matches!(error, StreamParseError::Parse(actual) if actual == expected));
            }
        }
    }

    #[test]
    fn enforces_partial_token_limit() {
        let options = IncrementalParserOptions {
            buffer_size: 4,
            max_token_bytes: 8,
            ..IncrementalParserOptions::default()
        };
        let mut parser = IncrementalParser::with_options(
            Cursor::new(b"1. e4 {a comment larger than the limit} *"),
            options,
        );
        assert!(matches!(
            parser.parse(|_| {}),
            Err(StreamParseError::TokenTooLarge { limit: 8, .. })
        ));
    }

    #[test]
    fn accepts_empty_input_and_split_bom() {
        assert!(incremental(b"", 1).unwrap().is_empty());
        let events = incremental(b"\xef\xbb\xbf1. e4 *", 1).unwrap();
        assert!(matches!(events.first(), Some(OwnedEvent::GameStart(3))));
    }
}
