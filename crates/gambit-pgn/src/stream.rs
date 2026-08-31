use std::error::Error;
use std::fmt;
use std::io::{self, Read};

const DEFAULT_BUFFER_SIZE: usize = 64 * 1024;
const DEFAULT_MAX_GAME_BYTES: usize = 16 * 1024 * 1024;

/// Configuration for [`GameReader`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameReaderOptions {
    /// Number of bytes requested from the underlying reader at once.
    pub buffer_size: usize,
    /// Maximum bytes retained while looking for one game's outcome marker.
    pub max_game_bytes: usize,
}

impl Default for GameReaderOptions {
    fn default() -> Self {
        Self {
            buffer_size: DEFAULT_BUFFER_SIZE,
            max_game_bytes: DEFAULT_MAX_GAME_BYTES,
        }
    }
}

/// An error while framing complete games from a byte stream.
#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    GameTooLarge { offset: u64, limit: usize },
    MissingOutcome { offset: u64, buffered: usize },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "failed to read PGN stream: {error}"),
            Self::GameTooLarge { offset, limit } => write!(
                f,
                "PGN game starting near byte {offset} exceeds the {limit}-byte limit"
            ),
            Self::MissingOutcome { offset, buffered } => write!(
                f,
                "PGN stream ended near byte {offset} with {buffered} unterminated game bytes"
            ),
        }
    }
}

impl Error for FrameError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::GameTooLarge { .. } | Self::MissingOutcome { .. } => None,
        }
    }
}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanMode {
    Normal,
    Tag,
    TagValue,
    BraceComment,
    LineComment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutcomeProbe {
    Match(usize),
    NeedMore,
    NoMatch,
}

/// Frames strict PGN games from any [`Read`] implementation with bounded memory.
///
/// A returned slice remains valid until the next call to [`Self::read_game`].
/// Games are delimited by a top-level PGN outcome marker, ignoring markers in
/// tag values, comments, and recursive annotation variations.
#[derive(Debug)]
pub struct GameReader<R> {
    reader: R,
    options: GameReaderOptions,
    buffer: Vec<u8>,
    read_buffer: Vec<u8>,
    frame_start: usize,
    scan_cursor: usize,
    returned_end: Option<usize>,
    mode: ScanMode,
    variation_depth: u32,
    tag_escape: bool,
    line_start: bool,
    eof: bool,
    bytes_read: u64,
    bytes_consumed: u64,
}

impl<R: Read> GameReader<R> {
    #[must_use]
    pub fn new(reader: R) -> Self {
        Self::with_options(reader, GameReaderOptions::default())
    }

    #[must_use]
    pub fn with_options(reader: R, options: GameReaderOptions) -> Self {
        let buffer_size = options.buffer_size.max(1);
        Self {
            reader,
            options,
            buffer: Vec::with_capacity(buffer_size),
            read_buffer: vec![0; buffer_size],
            frame_start: 0,
            scan_cursor: 0,
            returned_end: None,
            mode: ScanMode::Normal,
            variation_depth: 0,
            tag_escape: false,
            line_start: true,
            eof: false,
            bytes_read: 0,
            bytes_consumed: 0,
        }
    }

    /// Returns the next complete game, or `None` after trailing whitespace.
    ///
    /// # Errors
    ///
    /// Returns an I/O error, [`FrameError::GameTooLarge`] when an outcome is not
    /// found within the configured limit, or [`FrameError::MissingOutcome`] for
    /// a non-empty final record without a termination marker.
    pub fn read_game(&mut self) -> Result<Option<&[u8]>, FrameError> {
        self.discard_previous_game();

        loop {
            if let Some(end) = self.find_game_end() {
                if end - self.frame_start > self.options.max_game_bytes {
                    return Err(FrameError::GameTooLarge {
                        offset: self.bytes_consumed,
                        limit: self.options.max_game_bytes,
                    });
                }
                self.returned_end = Some(end);
                return Ok(Some(&self.buffer[self.frame_start..end]));
            }

            if self.eof {
                if self.buffer[self.frame_start..]
                    .iter()
                    .all(u8::is_ascii_whitespace)
                {
                    self.bytes_consumed += (self.buffer.len() - self.frame_start) as u64;
                    self.buffer.clear();
                    self.frame_start = 0;
                    return Ok(None);
                }
                return Err(FrameError::MissingOutcome {
                    offset: self.bytes_consumed,
                    buffered: self.buffer.len() - self.frame_start,
                });
            }

            if self.buffer.len() - self.frame_start > self.options.max_game_bytes {
                return Err(FrameError::GameTooLarge {
                    offset: self.bytes_consumed,
                    limit: self.options.max_game_bytes,
                });
            }

            self.compact_consumed_prefix();
            let read = self.reader.read(&mut self.read_buffer)?;
            if read == 0 {
                self.eof = true;
            } else {
                self.buffer.extend_from_slice(&self.read_buffer[..read]);
                self.bytes_read += read as u64;
            }
        }
    }

    #[must_use]
    pub const fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    #[must_use]
    pub fn buffered_bytes(&self) -> usize {
        self.buffer.len() - self.frame_start
    }

    #[must_use]
    pub fn into_inner(self) -> R {
        self.reader
    }

    fn discard_previous_game(&mut self) {
        let Some(end) = self.returned_end.take() else {
            return;
        };
        self.bytes_consumed += (end - self.frame_start) as u64;
        self.frame_start = end;
        self.scan_cursor = end;
        self.mode = ScanMode::Normal;
        self.variation_depth = 0;
        self.tag_escape = false;
        self.line_start = true;
    }

    fn compact_consumed_prefix(&mut self) {
        if self.frame_start == 0 {
            return;
        }
        let remaining = self.buffer.len() - self.frame_start;
        self.buffer.copy_within(self.frame_start.., 0);
        self.buffer.truncate(remaining);
        self.scan_cursor -= self.frame_start;
        self.frame_start = 0;
    }

    fn find_game_end(&mut self) -> Option<usize> {
        while self.scan_cursor < self.buffer.len() {
            let cursor = self.scan_cursor;
            let byte = self.buffer[cursor];
            let was_line_start = self.line_start;

            match self.mode {
                ScanMode::Normal => {
                    if self.variation_depth == 0 && is_outcome_start(byte) {
                        match probe_outcome(&self.buffer, cursor, self.eof) {
                            OutcomeProbe::Match(length) => return Some(cursor + length),
                            OutcomeProbe::NeedMore => return None,
                            OutcomeProbe::NoMatch => {}
                        }
                    }
                    match byte {
                        b'[' => self.mode = ScanMode::Tag,
                        b'{' => self.mode = ScanMode::BraceComment,
                        b';' => self.mode = ScanMode::LineComment,
                        b'%' if was_line_start => self.mode = ScanMode::LineComment,
                        b'(' => self.variation_depth = self.variation_depth.saturating_add(1),
                        b')' => self.variation_depth = self.variation_depth.saturating_sub(1),
                        _ => {}
                    }
                }
                ScanMode::Tag => match byte {
                    b'"' => self.mode = ScanMode::TagValue,
                    b']' => self.mode = ScanMode::Normal,
                    _ => {}
                },
                ScanMode::TagValue => {
                    if self.tag_escape {
                        self.tag_escape = false;
                    } else {
                        match byte {
                            b'\\' => self.tag_escape = true,
                            b'"' => self.mode = ScanMode::Tag,
                            _ => {}
                        }
                    }
                }
                ScanMode::BraceComment => {
                    if byte == b'}' {
                        self.mode = ScanMode::Normal;
                    }
                }
                ScanMode::LineComment => {
                    if matches!(byte, b'\r' | b'\n') {
                        self.mode = ScanMode::Normal;
                    }
                }
            }

            self.line_start = matches!(byte, b'\r' | b'\n');
            self.scan_cursor += 1;
        }
        None
    }
}

fn is_outcome_start(byte: u8) -> bool {
    matches!(byte, b'0' | b'1' | b'*')
}

fn probe_outcome(input: &[u8], cursor: usize, eof: bool) -> OutcomeProbe {
    const MARKERS: [&[u8]; 4] = [b"1/2-1/2", b"1-0", b"0-1", b"*"];

    if cursor != 0 && !is_token_boundary(input[cursor - 1]) {
        return OutcomeProbe::NoMatch;
    }

    let remaining = &input[cursor..];
    for marker in MARKERS {
        if remaining.len() < marker.len() {
            if marker.starts_with(remaining) && !eof {
                return OutcomeProbe::NeedMore;
            }
            continue;
        }
        if !remaining.starts_with(marker) {
            continue;
        }
        return match remaining.get(marker.len()) {
            Some(byte) if is_token_boundary(*byte) => OutcomeProbe::Match(marker.len()),
            Some(_) => OutcomeProbe::NoMatch,
            None if eof => OutcomeProbe::Match(marker.len()),
            None => OutcomeProbe::NeedMore,
        };
    }
    OutcomeProbe::NoMatch
}

fn is_token_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b'{' | b'}' | b';' | b'(' | b')' | b'$' | b'!' | b'?' | b'[' | b']'
        )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn reader(input: &[u8], buffer_size: usize) -> GameReader<Cursor<&[u8]>> {
        GameReader::with_options(
            Cursor::new(input),
            GameReaderOptions {
                buffer_size,
                max_game_bytes: 1024,
            },
        )
    }

    #[test]
    fn frames_games_across_tiny_reads() {
        let input = b"[Result \"1-0\"]\n1. e4 1-0\n\n[Result \"*\"]\n1. d4 *\n";
        let mut games = reader(input, 3);

        assert_eq!(games.read_game().unwrap(), Some(&input[..24] as &[u8]));
        assert_eq!(games.read_game().unwrap(), Some(&input[24..46] as &[u8]));
        assert_eq!(games.read_game().unwrap(), None);
        assert_eq!(games.bytes_read(), input.len() as u64);
    }

    #[test]
    fn ignores_outcomes_in_tags_comments_and_variations() {
        let input = b"[Event \"1-0\"]\n1. e4 { 0-1 } (1... e5 *) c5 1/2-1/2";
        let mut games = reader(input, 5);

        assert_eq!(games.read_game().unwrap(), Some(input.as_slice()));
        assert_eq!(games.read_game().unwrap(), None);
    }

    #[test]
    fn reports_incomplete_and_oversized_games() {
        let mut incomplete = reader(b"1. e4 e5", 4);
        assert!(matches!(
            incomplete.read_game(),
            Err(FrameError::MissingOutcome { buffered: 8, .. })
        ));

        let mut oversized = GameReader::with_options(
            Cursor::new(b"1. e4 e5 e6 e7 e8"),
            GameReaderOptions {
                buffer_size: 4,
                max_game_bytes: 8,
            },
        );
        assert!(matches!(
            oversized.read_game(),
            Err(FrameError::GameTooLarge { limit: 8, .. })
        ));

        let mut complete_but_oversized = GameReader::with_options(
            Cursor::new(b"1. e4 e5 e6 *"),
            GameReaderOptions {
                buffer_size: 32,
                max_game_bytes: 8,
            },
        );
        assert!(matches!(
            complete_but_oversized.read_game(),
            Err(FrameError::GameTooLarge { limit: 8, .. })
        ));
    }

    #[test]
    fn accepts_outcome_at_eof_and_trailing_whitespace() {
        let input = b"\xef\xbb\xbf1. e4 *  \n";
        let mut games = reader(input, 2);
        assert_eq!(games.read_game().unwrap(), Some(&input[..10] as &[u8]));
        assert_eq!(games.read_game().unwrap(), None);
    }
}
