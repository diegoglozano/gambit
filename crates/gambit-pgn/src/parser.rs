use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::iter::FusedIterator;

/// A half-open byte range in the original PGN input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use]
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A borrowed token from the movetext section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token<'a> {
    bytes: &'a [u8],
    span: Span,
}

impl<'a> Token<'a> {
    #[must_use]
    pub const fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// Returns the token as UTF-8, if it is valid UTF-8.
    ///
    /// # Errors
    ///
    /// Returns an error when the borrowed token is not valid UTF-8.
    pub fn as_str(self) -> Result<&'a str, std::str::Utf8Error> {
        std::str::from_utf8(self.bytes)
    }

    #[must_use]
    pub const fn span(self) -> Span {
        self.span
    }
}

/// A tag pair from the header section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tag<'a> {
    name: &'a [u8],
    value: &'a [u8],
    span: Span,
}

impl<'a> Tag<'a> {
    #[must_use]
    pub const fn name(self) -> &'a [u8] {
        self.name
    }

    /// The raw tag value, without quotes. Backslash escapes are retained.
    #[must_use]
    pub const fn raw_value(self) -> &'a [u8] {
        self.value
    }

    /// Decodes PGN's `\\` and `\"` escapes, allocating only when needed.
    #[must_use]
    pub fn value(self) -> Cow<'a, [u8]> {
        if !self.value.contains(&b'\\') {
            return Cow::Borrowed(self.value);
        }

        let mut decoded = Vec::with_capacity(self.value.len());
        let mut cursor = 0;
        while cursor < self.value.len() {
            if self.value[cursor] == b'\\' && cursor + 1 < self.value.len() {
                cursor += 1;
            }
            decoded.push(self.value[cursor]);
            cursor += 1;
        }
        Cow::Owned(decoded)
    }

    #[must_use]
    pub const fn span(self) -> Span {
        self.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommentKind {
    Brace,
    Line,
    EscapeLine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Comment<'a> {
    pub kind: CommentKind,
    pub text: &'a [u8],
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Nag<'a> {
    Numeric { value: u32, span: Span },
    Glyph(Token<'a>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    WhiteWins,
    BlackWins,
    Draw,
    Unknown,
}

/// An event in the PGN stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event<'a> {
    GameStart { offset: usize },
    Tag(Tag<'a>),
    MovetextStart { offset: usize },
    MoveNumber { number: u32, dots: u8, span: Span },
    San(Token<'a>),
    Nag(Nag<'a>),
    Comment(Comment<'a>),
    VariationStart(Span),
    VariationEnd(Span),
    Outcome { outcome: Outcome, span: Span },
    GameEnd { offset: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserOptions {
    /// Require every game to end in `1-0`, `0-1`, `1/2-1/2`, or `*`.
    pub require_outcome: bool,
}

impl ParserOptions {
    pub const STRICT: Self = Self {
        require_outcome: true,
    };
    pub const LENIENT: Self = Self {
        require_outcome: false,
    };
}

impl Default for ParserOptions {
    fn default() -> Self {
        Self::STRICT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    ExpectedTagName,
    ExpectedTagValue,
    ExpectedClosingBracket,
    NewlineInTag,
    UnterminatedTagValue,
    UnterminatedComment,
    InvalidMoveNumber,
    InvalidNag,
    UnexpectedHeader,
    UnexpectedByte(u8),
    UnclosedVariation { depth: u32 },
    UnmatchedVariationEnd,
    MissingOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub offset: usize,
    pub kind: ErrorKind,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PGN parse error at byte {}: ", self.offset)?;
        match self.kind {
            ErrorKind::ExpectedTagName => f.write_str("expected a tag name"),
            ErrorKind::ExpectedTagValue => f.write_str("expected a quoted tag value"),
            ErrorKind::ExpectedClosingBracket => f.write_str("expected ']'"),
            ErrorKind::NewlineInTag => f.write_str("newline in tag pair"),
            ErrorKind::UnterminatedTagValue => f.write_str("unterminated tag value"),
            ErrorKind::UnterminatedComment => f.write_str("unterminated brace comment"),
            ErrorKind::InvalidMoveNumber => f.write_str("invalid or overflowing move number"),
            ErrorKind::InvalidNag => f.write_str("invalid or overflowing numeric annotation glyph"),
            ErrorKind::UnexpectedHeader => f.write_str("tag pair encountered in movetext"),
            ErrorKind::UnexpectedByte(byte) => write!(f, "unexpected byte 0x{byte:02x}"),
            ErrorKind::UnclosedVariation { depth } => {
                write!(f, "game ended with {depth} unclosed variation(s)")
            }
            ErrorKind::UnmatchedVariationEnd => f.write_str("unmatched ')'"),
            ErrorKind::MissingOutcome => f.write_str("game has no outcome marker"),
        }
    }
}

impl Error for ParseError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    BetweenGames,
    Headers,
    MovetextStart,
    Movetext,
    GameEnd,
    Done,
}

/// A zero-allocation iterator over the structural events in a PGN byte slice.
///
/// Parsing stops after the first error. Create a new parser at a known game
/// boundary to implement error recovery for bulk import pipelines.
#[derive(Clone, Debug)]
pub struct Parser<'a> {
    input: &'a [u8],
    cursor: usize,
    state: State,
    options: ParserOptions,
    variation_depth: u32,
}

impl<'a> Parser<'a> {
    #[must_use]
    pub fn new(input: &'a [u8]) -> Self {
        Self::with_options(input, ParserOptions::default())
    }

    #[must_use]
    pub fn with_options(input: &'a [u8], options: ParserOptions) -> Self {
        let cursor = usize::from(input.starts_with(&[0xef, 0xbb, 0xbf])) * 3;
        Self {
            input,
            cursor,
            state: State::BetweenGames,
            options,
            variation_depth: 0,
        }
    }

    #[must_use]
    pub const fn offset(&self) -> usize {
        self.cursor
    }

    fn error(&mut self, offset: usize, kind: ErrorKind) -> Result<Event<'a>, ParseError> {
        self.state = State::Done;
        Err(ParseError { offset, kind })
    }

    fn stop_on_error(
        &mut self,
        result: Result<Event<'a>, ParseError>,
    ) -> Result<Event<'a>, ParseError> {
        if result.is_err() {
            self.state = State::Done;
        }
        result
    }

    fn skip_whitespace(&mut self) {
        while self
            .input
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn skip_horizontal_whitespace(&mut self) -> Result<(), ParseError> {
        while matches!(self.input.get(self.cursor), Some(b' ' | b'\t')) {
            self.cursor += 1;
        }
        if matches!(self.input.get(self.cursor), Some(b'\r' | b'\n')) {
            return Err(ParseError {
                offset: self.cursor,
                kind: ErrorKind::NewlineInTag,
            });
        }
        Ok(())
    }

    fn parse_tag(&mut self) -> Result<Event<'a>, ParseError> {
        let start = self.cursor;
        self.cursor += 1;
        self.skip_horizontal_whitespace()?;

        let name_start = self.cursor;
        while self
            .input
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.cursor += 1;
        }
        if self.cursor == name_start {
            return Err(ParseError {
                offset: self.cursor,
                kind: ErrorKind::ExpectedTagName,
            });
        }
        let name_end = self.cursor;
        self.skip_horizontal_whitespace()?;

        if self.input.get(self.cursor) != Some(&b'"') {
            return Err(ParseError {
                offset: self.cursor,
                kind: ErrorKind::ExpectedTagValue,
            });
        }
        self.cursor += 1;
        let value_start = self.cursor;
        loop {
            match self.input.get(self.cursor) {
                Some(b'"') => break,
                Some(b'\\') => {
                    self.cursor += 1;
                    match self.input.get(self.cursor) {
                        Some(b'\r' | b'\n') => {
                            return Err(ParseError {
                                offset: self.cursor,
                                kind: ErrorKind::NewlineInTag,
                            });
                        }
                        Some(_) => self.cursor += 1,
                        None => {
                            return Err(ParseError {
                                offset: self.cursor,
                                kind: ErrorKind::UnterminatedTagValue,
                            });
                        }
                    }
                }
                Some(b'\r' | b'\n') => {
                    return Err(ParseError {
                        offset: self.cursor,
                        kind: ErrorKind::NewlineInTag,
                    });
                }
                Some(_) => self.cursor += 1,
                None => {
                    return Err(ParseError {
                        offset: self.cursor,
                        kind: ErrorKind::UnterminatedTagValue,
                    });
                }
            }
        }
        let value_end = self.cursor;
        self.cursor += 1;
        self.skip_horizontal_whitespace()?;
        if self.input.get(self.cursor) != Some(&b']') {
            return Err(ParseError {
                offset: self.cursor,
                kind: ErrorKind::ExpectedClosingBracket,
            });
        }
        self.cursor += 1;

        Ok(Event::Tag(Tag {
            name: &self.input[name_start..name_end],
            value: &self.input[value_start..value_end],
            span: Span {
                start,
                end: self.cursor,
            },
        }))
    }

    fn parse_comment(&mut self, kind: CommentKind) -> Result<Event<'a>, ParseError> {
        let start = self.cursor;
        match kind {
            CommentKind::Brace => {
                self.cursor += 1;
                let text_start = self.cursor;
                while self.input.get(self.cursor) != Some(&b'}') {
                    if self.cursor == self.input.len() {
                        return Err(ParseError {
                            offset: start,
                            kind: ErrorKind::UnterminatedComment,
                        });
                    }
                    self.cursor += 1;
                }
                let text_end = self.cursor;
                self.cursor += 1;
                Ok(Event::Comment(Comment {
                    kind,
                    text: &self.input[text_start..text_end],
                    span: Span {
                        start,
                        end: self.cursor,
                    },
                }))
            }
            CommentKind::Line | CommentKind::EscapeLine => {
                self.cursor += 1;
                let text_start = self.cursor;
                while !matches!(self.input.get(self.cursor), None | Some(b'\r' | b'\n')) {
                    self.cursor += 1;
                }
                Ok(Event::Comment(Comment {
                    kind,
                    text: &self.input[text_start..self.cursor],
                    span: Span {
                        start,
                        end: self.cursor,
                    },
                }))
            }
        }
    }

    fn parse_numeric_nag(&mut self) -> Result<Event<'a>, ParseError> {
        let start = self.cursor;
        self.cursor += 1;
        let digits_start = self.cursor;
        while self.input.get(self.cursor).is_some_and(u8::is_ascii_digit) {
            self.cursor += 1;
        }
        if digits_start == self.cursor {
            return Err(ParseError {
                offset: start,
                kind: ErrorKind::InvalidNag,
            });
        }
        let value = parse_decimal(&self.input[digits_start..self.cursor]).ok_or(ParseError {
            offset: start,
            kind: ErrorKind::InvalidNag,
        })?;
        Ok(Event::Nag(Nag::Numeric {
            value,
            span: Span {
                start,
                end: self.cursor,
            },
        }))
    }

    fn parse_glyph_nag(&mut self) -> Event<'a> {
        let start = self.cursor;
        while matches!(self.input.get(self.cursor), Some(b'!' | b'?')) {
            self.cursor += 1;
        }
        Event::Nag(Nag::Glyph(Token {
            bytes: &self.input[start..self.cursor],
            span: Span {
                start,
                end: self.cursor,
            },
        }))
    }

    fn parse_move_number(&mut self) -> Result<Option<Event<'a>>, ParseError> {
        let start = self.cursor;
        while self.input.get(self.cursor).is_some_and(u8::is_ascii_digit) {
            self.cursor += 1;
        }
        let digits_end = self.cursor;
        let dots_start = self.cursor;
        while self.input.get(self.cursor) == Some(&b'.') {
            self.cursor += 1;
        }
        let dots = self.cursor - dots_start;
        if dots == 0 {
            self.cursor = start;
            return Ok(None);
        }
        if !matches!(dots, 1 | 3) {
            return Err(ParseError {
                offset: start,
                kind: ErrorKind::InvalidMoveNumber,
            });
        }
        let number = parse_decimal(&self.input[start..digits_end]).ok_or(ParseError {
            offset: start,
            kind: ErrorKind::InvalidMoveNumber,
        })?;
        Ok(Some(Event::MoveNumber {
            number,
            dots: u8::try_from(dots).expect("at most three dots"),
            span: Span {
                start,
                end: self.cursor,
            },
        }))
    }

    fn parse_san(&mut self) -> Result<Event<'a>, ParseError> {
        let start = self.cursor;
        while self.input.get(self.cursor).is_some_and(|byte| {
            !byte.is_ascii_whitespace()
                && !matches!(
                    *byte,
                    b'{' | b'}' | b';' | b'(' | b')' | b'$' | b'!' | b'?' | b'['
                )
        }) {
            self.cursor += 1;
        }
        if self.cursor == start {
            return Err(ParseError {
                offset: start,
                kind: ErrorKind::UnexpectedByte(self.input[start]),
            });
        }
        Ok(Event::San(Token {
            bytes: &self.input[start..self.cursor],
            span: Span {
                start,
                end: self.cursor,
            },
        }))
    }

    fn outcome_at_cursor(&self) -> Option<(Outcome, usize)> {
        const OUTCOMES: [(&[u8], Outcome); 4] = [
            (b"1/2-1/2", Outcome::Draw),
            (b"1-0", Outcome::WhiteWins),
            (b"0-1", Outcome::BlackWins),
            (b"*", Outcome::Unknown),
        ];
        OUTCOMES.into_iter().find_map(|(marker, outcome)| {
            let rest = self.input.get(self.cursor..)?;
            if !rest.starts_with(marker) {
                return None;
            }
            let following = rest.get(marker.len());
            if following.is_none_or(|byte| is_token_boundary(*byte)) {
                Some((outcome, marker.len()))
            } else {
                None
            }
        })
    }

    fn finish_at_eof(&mut self) -> Result<Event<'a>, ParseError> {
        if self.variation_depth != 0 {
            return self.error(
                self.cursor,
                ErrorKind::UnclosedVariation {
                    depth: self.variation_depth,
                },
            );
        }
        if self.options.require_outcome {
            return self.error(self.cursor, ErrorKind::MissingOutcome);
        }
        self.state = State::Done;
        Ok(Event::GameEnd {
            offset: self.cursor,
        })
    }

    fn next_movetext(&mut self) -> Option<Result<Event<'a>, ParseError>> {
        self.skip_whitespace();
        if self.cursor == self.input.len() {
            return Some(self.finish_at_eof());
        }

        let start = self.cursor;
        let byte = self.input[self.cursor];
        if byte == b'[' {
            if !self.options.require_outcome && self.variation_depth == 0 {
                self.state = State::GameEnd;
                return None;
            }
            return Some(self.error(start, ErrorKind::UnexpectedHeader));
        }
        if byte == b'{' {
            let event = self.parse_comment(CommentKind::Brace);
            return Some(self.stop_on_error(event));
        }
        if byte == b';' {
            let event = self.parse_comment(CommentKind::Line);
            return Some(self.stop_on_error(event));
        }
        if byte == b'%' && (start == 0 || self.input[start - 1] == b'\n') {
            let event = self.parse_comment(CommentKind::EscapeLine);
            return Some(self.stop_on_error(event));
        }
        if byte == b'(' {
            self.cursor += 1;
            self.variation_depth += 1;
            return Some(Ok(Event::VariationStart(Span {
                start,
                end: self.cursor,
            })));
        }
        if byte == b')' {
            if self.variation_depth == 0 {
                return Some(self.error(start, ErrorKind::UnmatchedVariationEnd));
            }
            self.cursor += 1;
            self.variation_depth -= 1;
            return Some(Ok(Event::VariationEnd(Span {
                start,
                end: self.cursor,
            })));
        }
        if byte == b'$' {
            let event = self.parse_numeric_nag();
            return Some(self.stop_on_error(event));
        }
        if matches!(byte, b'!' | b'?') {
            return Some(Ok(self.parse_glyph_nag()));
        }
        if let Some((outcome, marker_len)) = self.outcome_at_cursor() {
            self.cursor += marker_len;
            let event = Event::Outcome {
                outcome,
                span: Span {
                    start,
                    end: self.cursor,
                },
            };
            if self.variation_depth == 0 {
                self.state = State::GameEnd;
            }
            return Some(Ok(event));
        }
        if byte.is_ascii_digit() {
            match self.parse_move_number() {
                Ok(Some(event)) => return Some(Ok(event)),
                Ok(None) => {}
                Err(error) => {
                    self.state = State::Done;
                    return Some(Err(error));
                }
            }
        }
        let event = self.parse_san();
        Some(self.stop_on_error(event))
    }
}

impl<'a> Iterator for Parser<'a> {
    type Item = Result<Event<'a>, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.state {
                State::Done => return None,
                State::BetweenGames => {
                    self.skip_whitespace();
                    if self.cursor == self.input.len() {
                        self.state = State::Done;
                        return None;
                    }
                    let offset = self.cursor;
                    self.variation_depth = 0;
                    self.state = if self.input[self.cursor] == b'[' {
                        State::Headers
                    } else {
                        State::MovetextStart
                    };
                    return Some(Ok(Event::GameStart { offset }));
                }
                State::Headers => {
                    self.skip_whitespace();
                    if self.input.get(self.cursor) == Some(&b'[') {
                        let event = self.parse_tag();
                        return Some(self.stop_on_error(event));
                    }
                    self.state = State::MovetextStart;
                }
                State::MovetextStart => {
                    self.skip_whitespace();
                    self.state = State::Movetext;
                    return Some(Ok(Event::MovetextStart {
                        offset: self.cursor,
                    }));
                }
                State::GameEnd => {
                    self.state = State::BetweenGames;
                    return Some(Ok(Event::GameEnd {
                        offset: self.cursor,
                    }));
                }
                State::Movetext => {
                    if let Some(event) = self.next_movetext() {
                        return Some(event);
                    }
                }
            }
        }
    }
}

impl FusedIterator for Parser<'_> {}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn events(input: &[u8]) -> Vec<Event<'_>> {
        Parser::new(input).collect::<Result<Vec<_>, _>>().unwrap()
    }

    #[test]
    fn parses_complete_game_without_allocating_tokens() {
        let input = br#"[Event "World Championship"]
[Site "Reykjavik ISL"]

1. e4 c5 2. Nf3 d6 3. d4 cxd4 4. Nxd4 Nf6 1/2-1/2"#;
        let parsed = events(input);

        assert!(matches!(parsed[0], Event::GameStart { offset: 0 }));
        let Event::Tag(event) = parsed[1] else {
            panic!("expected Event tag");
        };
        assert_eq!(event.name(), b"Event");
        assert_eq!(event.raw_value(), b"World Championship");
        assert_eq!(
            parsed
                .iter()
                .filter(|event| matches!(event, Event::San(_)))
                .count(),
            8
        );
        assert!(parsed.iter().any(|event| matches!(
            event,
            Event::Outcome {
                outcome: Outcome::Draw,
                ..
            }
        )));
        assert!(matches!(parsed.last(), Some(Event::GameEnd { .. })));
    }

    #[test]
    fn parses_variations_comments_and_nags() {
        let parsed = events(b"1. e4! {main line} (1. d4 $14 d5?!) e5 ; rest\n2. Nf3 *");

        assert!(parsed.iter().any(|event| matches!(
            event,
            Event::Comment(Comment {
                kind: CommentKind::Brace,
                text: b"main line",
                ..
            })
        )));
        assert!(
            parsed
                .iter()
                .any(|event| matches!(event, Event::Nag(Nag::Numeric { value: 14, .. })))
        );
        assert!(parsed.iter().any(|event| matches!(
            event,
            Event::Nag(Nag::Glyph(token)) if token.as_bytes() == b"?!"
        )));
        assert_eq!(
            parsed
                .iter()
                .filter(|event| matches!(event, Event::VariationStart(_)))
                .count(),
            1
        );
    }

    #[test]
    fn retains_raw_tag_value_and_decodes_on_demand() {
        let parsed = events(
            br#"[Annotator "Ada \"A\" \\ Team"]
*"#,
        );
        let Event::Tag(tag) = parsed[1] else {
            panic!("expected tag");
        };
        assert_eq!(tag.raw_value(), br#"Ada \"A\" \\ Team"#);
        assert_eq!(&*tag.value(), br#"Ada "A" \ Team"#);
        assert!(matches!(tag.value(), Cow::Owned(_)));
    }

    #[test]
    fn parses_multiple_games_and_utf8_bom() {
        let parsed = events(b"\xef\xbb\xbf[Result \"1-0\"]\n1. e4 1-0\n\n[Result \"*\"]\n1. d4 *");
        assert_eq!(
            parsed
                .iter()
                .filter(|event| matches!(event, Event::GameStart { .. }))
                .count(),
            2
        );
        assert_eq!(
            parsed
                .iter()
                .filter(|event| matches!(event, Event::GameEnd { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn reports_structural_errors_with_offsets() {
        let error = Parser::new(b"1. e4 (1... c5 *")
            .collect::<Result<Vec<_>, _>>()
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::UnclosedVariation { depth: 1 });

        let error = Parser::new(b"1. e4")
            .collect::<Result<Vec<_>, _>>()
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::MissingOutcome);
    }

    #[test]
    fn lenient_mode_accepts_missing_outcomes_and_finds_next_header() {
        let parser =
            Parser::with_options(b"1. e4 e5\n[Event \"next\"]\n1. d4", ParserOptions::LENIENT);
        let parsed = parser.collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(
            parsed
                .iter()
                .filter(|event| matches!(event, Event::GameEnd { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn parser_is_fused_after_an_error() {
        let mut parser = Parser::new(b"1. e4 }");
        assert!(parser.by_ref().any(|event| event.is_err()));
        assert!(parser.next().is_none());
    }

    #[test]
    fn rejects_malformed_move_numbers_and_numeric_nags() {
        let error = Parser::new(b"1.. e4 *")
            .collect::<Result<Vec<_>, _>>()
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidMoveNumber);

        let error = Parser::new(b"1. e4 $4294967296 *")
            .collect::<Result<Vec<_>, _>>()
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidNag);
    }

    #[test]
    fn arbitrary_short_inputs_do_not_panic() {
        for first in u8::MIN..=u8::MAX {
            for second in u8::MIN..=u8::MAX {
                let input = [first, second];
                let _ = Parser::with_options(&input, ParserOptions::LENIENT).count();
            }
        }
    }
}
