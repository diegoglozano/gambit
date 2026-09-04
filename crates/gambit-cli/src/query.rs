use std::borrow::Cow;
use std::fmt;
use std::io::{self, Read, Write};

use gambit_chess::{FenError, Position, SanError};
use gambit_pgn::{Event, FrameError, GameReader, Outcome, Parser, ParserOptions, Tag};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryFormat {
    Pgn,
    Jsonl,
    Count,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerColor {
    White,
    Black,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultFilter {
    Win,
    Loss,
    Draw,
    Unfinished,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QueryOptions {
    pub player: Option<String>,
    pub opponent: Option<String>,
    pub color: Option<PlayerColor>,
    pub result: Option<ResultFilter>,
    pub since: Option<u32>,
    pub until: Option<u32>,
    pub minimum_rating: Option<u32>,
    pub maximum_rating: Option<u32>,
    pub position: Option<Position>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QuerySummary {
    pub bytes: u64,
    pub games: u64,
    pub matches: u64,
}

impl QuerySummary {
    pub fn add(&mut self, other: Self) {
        self.bytes += other.bytes;
        self.games += other.games;
        self.matches += other.matches;
    }
}

#[derive(Debug)]
pub enum QueryError {
    Parse {
        game: u64,
        error: gambit_pgn::ParseError,
    },
    InvalidFen {
        game: u64,
        error: FenError,
    },
    InvalidSan {
        game: u64,
        ply: u64,
        san: String,
        error: SanError,
    },
    Frame(FrameError),
    Database(String),
    Output(io::Error),
}

#[derive(Debug)]
pub struct QueryFailure {
    pub summary: QuerySummary,
    pub error: QueryError,
}

impl QueryError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Parse { .. }
            | Self::InvalidFen { .. }
            | Self::InvalidSan { .. }
            | Self::Frame(FrameError::GameTooLarge { .. } | FrameError::MissingOutcome { .. }) => 1,
            Self::Frame(FrameError::Io(_)) | Self::Database(_) | Self::Output(_) => 3,
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { game, error } => write!(formatter, "game {game}: {error}"),
            Self::InvalidFen { game, error } => {
                write!(formatter, "game {game}: invalid starting FEN: {error}")
            }
            Self::InvalidSan {
                game,
                ply,
                san,
                error,
            } => write!(formatter, "game {game}, ply {ply}: {error} ({san})"),
            Self::Frame(error) => error.fmt(formatter),
            Self::Database(error) => write!(formatter, "failed to query Gambit database: {error}"),
            Self::Output(error) => write!(formatter, "failed to write query output: {error}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedPlayer {
    White,
    Black,
}

#[derive(Debug, Default)]
struct GameMetadata<'a> {
    event: Option<Cow<'a, [u8]>>,
    site: Option<Cow<'a, [u8]>>,
    date: Option<Cow<'a, [u8]>>,
    utc_date: Option<Cow<'a, [u8]>>,
    white: Option<Cow<'a, [u8]>>,
    black: Option<Cow<'a, [u8]>>,
    white_elo: Option<Cow<'a, [u8]>>,
    black_elo: Option<Cow<'a, [u8]>>,
    fen: Option<Cow<'a, [u8]>>,
    variant: Option<Cow<'a, [u8]>>,
    outcome: Option<Outcome>,
    mainline_plies: u64,
    variation_depth: u32,
    position: Position,
    position_ply: Option<u64>,
    position_search_active: bool,
}

impl<'a> GameMetadata<'a> {
    fn observe(
        &mut self,
        event: Event<'a>,
        target: Option<Position>,
        game: u64,
    ) -> Result<(), QueryError> {
        match event {
            Event::Tag(tag) => self.observe_tag(tag),
            Event::MovetextStart { .. } => self.start_movetext(target, game)?,
            Event::San(token) if self.variation_depth == 0 => {
                self.mainline_plies += 1;
                if let Some(target) = target.filter(|_| self.position_search_active) {
                    self.position.play_san(token.as_bytes()).map_err(|error| {
                        QueryError::InvalidSan {
                            game,
                            ply: self.mainline_plies,
                            san: String::from_utf8_lossy(token.as_bytes()).into_owned(),
                            error,
                        }
                    })?;
                    if self.position_ply.is_none() && self.position.same_position(target) {
                        self.position_ply = Some(self.mainline_plies);
                    }
                }
            }
            Event::VariationStart(_) => self.variation_depth += 1,
            Event::VariationEnd(_) => self.variation_depth -= 1,
            Event::Outcome { outcome, .. } if self.variation_depth == 0 => {
                self.outcome = Some(outcome);
            }
            _ => {}
        }
        Ok(())
    }

    fn observe_tag(&mut self, tag: Tag<'a>) {
        let destination = match tag.name() {
            b"Event" => &mut self.event,
            b"Site" => &mut self.site,
            b"Date" => &mut self.date,
            b"UTCDate" => &mut self.utc_date,
            b"White" => &mut self.white,
            b"Black" => &mut self.black,
            b"WhiteElo" => &mut self.white_elo,
            b"BlackElo" => &mut self.black_elo,
            b"FEN" => &mut self.fen,
            b"Variant" => &mut self.variant,
            _ => return,
        };
        if destination.is_none() {
            *destination = Some(tag.value());
        }
    }

    fn start_movetext(&mut self, target: Option<Position>, game: u64) -> Result<(), QueryError> {
        let Some(target) = target else {
            return Ok(());
        };
        if self
            .variant
            .as_deref()
            .is_some_and(|variant| !variant.eq_ignore_ascii_case(b"standard"))
        {
            return Ok(());
        }
        self.position_search_active = true;
        self.position = match self.fen.as_deref() {
            Some(fen) => {
                Position::from_fen(fen).map_err(|error| QueryError::InvalidFen { game, error })?
            }
            None => Position::initial(),
        };
        if self.position.same_position(target) {
            self.position_ply = Some(0);
        }
        Ok(())
    }

    fn selected_player(&self, player: &str) -> Option<SelectedPlayer> {
        if self
            .white
            .as_ref()
            .is_some_and(|value| value.eq_ignore_ascii_case(player.as_bytes()))
        {
            Some(SelectedPlayer::White)
        } else if self
            .black
            .as_ref()
            .is_some_and(|value| value.eq_ignore_ascii_case(player.as_bytes()))
        {
            Some(SelectedPlayer::Black)
        } else {
            None
        }
    }

    fn effective_date(&self) -> Option<&[u8]> {
        self.date.as_deref().or(self.utc_date.as_deref())
    }

    fn matches(&self, options: &QueryOptions) -> bool {
        if options.position.is_some() && self.position_ply.is_none() {
            return false;
        }
        let selected = options
            .player
            .as_deref()
            .and_then(|player| self.selected_player(player));
        if options.player.is_some() && selected.is_none() {
            return false;
        }

        if let Some(color) = options.color {
            let expected = match color {
                PlayerColor::White => SelectedPlayer::White,
                PlayerColor::Black => SelectedPlayer::Black,
            };
            if selected != Some(expected) {
                return false;
            }
        }

        if let Some(opponent) = options.opponent.as_deref() {
            let actual = match selected {
                Some(SelectedPlayer::White) => self.black.as_deref(),
                Some(SelectedPlayer::Black) => self.white.as_deref(),
                None => None,
            };
            if !actual.is_some_and(|value| value.eq_ignore_ascii_case(opponent.as_bytes())) {
                return false;
            }
        }

        if let Some(result) = options.result {
            let matches = match result {
                ResultFilter::Draw => self.outcome == Some(Outcome::Draw),
                ResultFilter::Unfinished => self.outcome == Some(Outcome::Unknown),
                ResultFilter::Win => matches!(
                    (selected, self.outcome),
                    (Some(SelectedPlayer::White), Some(Outcome::WhiteWins))
                        | (Some(SelectedPlayer::Black), Some(Outcome::BlackWins))
                ),
                ResultFilter::Loss => matches!(
                    (selected, self.outcome),
                    (Some(SelectedPlayer::White), Some(Outcome::BlackWins))
                        | (Some(SelectedPlayer::Black), Some(Outcome::WhiteWins))
                ),
            };
            if !matches {
                return false;
            }
        }

        if options.since.is_some() || options.until.is_some() {
            let Some(date) = self.effective_date().and_then(parse_complete_date) else {
                return false;
            };
            if options.since.is_some_and(|minimum| date < minimum)
                || options.until.is_some_and(|maximum| date > maximum)
            {
                return false;
            }
        }

        if options.minimum_rating.is_some() || options.maximum_rating.is_some() {
            let rating = match selected {
                Some(SelectedPlayer::White) => self.white_elo.as_deref(),
                Some(SelectedPlayer::Black) => self.black_elo.as_deref(),
                None => None,
            }
            .and_then(parse_unsigned);
            let Some(rating) = rating else {
                return false;
            };
            if options
                .minimum_rating
                .is_some_and(|minimum| rating < minimum)
                || options
                    .maximum_rating
                    .is_some_and(|maximum| rating > maximum)
            {
                return false;
            }
        }

        true
    }

    fn write_match(
        &self,
        output: &mut impl Write,
        format: QueryFormat,
        source: &str,
        game: u64,
        raw_pgn: &[u8],
    ) -> Result<(), QueryError> {
        match format {
            QueryFormat::Pgn => {
                output.write_all(raw_pgn).map_err(QueryError::Output)?;
                output.write_all(b"\n\n").map_err(QueryError::Output)
            }
            QueryFormat::Jsonl => {
                let record = MatchRecord {
                    schema_version: 1,
                    source,
                    game,
                    event: self.event.as_deref().map(lossy_string),
                    site: self.site.as_deref().map(lossy_string),
                    date: self.effective_date().map(lossy_string),
                    white: self.white.as_deref().map(lossy_string),
                    black: self.black.as_deref().map(lossy_string),
                    white_elo: self.white_elo.as_deref().and_then(parse_unsigned),
                    black_elo: self.black_elo.as_deref().and_then(parse_unsigned),
                    result: self.outcome.map(outcome_label),
                    mainline_plies: self.mainline_plies,
                    position_ply: self.position_ply,
                };
                serde_json::to_writer(&mut *output, &record)
                    .map_err(|error| QueryError::Output(io::Error::other(error)))?;
                output.write_all(b"\n").map_err(QueryError::Output)
            }
            QueryFormat::Count => Ok(()),
        }
    }
}

#[derive(Serialize)]
struct MatchRecord<'a> {
    schema_version: u8,
    source: &'a str,
    game: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    white: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    black: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    white_elo: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    black_elo: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'static str>,
    mainline_plies: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    position_ply: Option<u64>,
}

pub fn query<R: Read>(
    reader: R,
    source: &str,
    options: &QueryOptions,
    format: QueryFormat,
    output: &mut impl Write,
) -> Result<QuerySummary, QueryFailure> {
    let mut reader = GameReader::new(reader);
    let mut summary = QuerySummary::default();
    loop {
        let game = match reader.read_game() {
            Ok(game) => game,
            Err(error) => {
                summary.bytes = reader.bytes_read();
                return Err(QueryFailure {
                    summary,
                    error: QueryError::Frame(error),
                });
            }
        };
        let Some(game) = game else {
            summary.bytes = reader.bytes_read();
            return Ok(summary);
        };
        summary.games += 1;
        let mut metadata = GameMetadata::default();
        for event in Parser::with_options(game, ParserOptions::STRICT) {
            match event {
                Ok(event) => {
                    if let Err(error) = metadata.observe(event, options.position, summary.games) {
                        return Err(QueryFailure { summary, error });
                    }
                }
                Err(error) => {
                    return Err(QueryFailure {
                        summary,
                        error: QueryError::Parse {
                            game: summary.games,
                            error,
                        },
                    });
                }
            }
        }
        if metadata.matches(options) {
            summary.matches += 1;
            if let Err(error) = metadata.write_match(output, format, source, summary.games, game) {
                return Err(QueryFailure { summary, error });
            }
        }
        summary.bytes = reader.bytes_read();
    }
}

const fn outcome_label(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::WhiteWins => "white_win",
        Outcome::BlackWins => "black_win",
        Outcome::Draw => "draw",
        Outcome::Unknown => "unfinished",
    }
}

pub fn parse_date(value: &str) -> Option<u32> {
    parse_complete_date(value.as_bytes())
}

fn parse_complete_date(value: &[u8]) -> Option<u32> {
    if value.len() != 10 || !matches!(value[4], b'.' | b'-') || value[7] != value[4] {
        return None;
    }
    let year = parse_unsigned(&value[..4])?;
    let month = parse_unsigned(&value[5..7])?;
    let day = parse_unsigned(&value[8..])?;
    if year == 0 || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    Some(year * 10_000 + month * 100 + day)
}

fn parse_unsigned(value: &[u8]) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    value.iter().try_fold(0_u32, |number, byte| {
        let digit = u32::from(byte.checked_sub(b'0')?);
        (digit <= 9)
            .then_some(number)
            .and_then(|number| number.checked_mul(10))?
            .checked_add(digit)
    })
}

fn lossy_string(value: &[u8]) -> String {
    String::from_utf8_lossy(value).into_owned()
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

const fn is_leap_year(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAMES: &[u8] = br#"[Event "First"]
[Site "https://example.test/1"]
[Date "2026.01.02"]
[White "DiegoGLozano"]
[Black "OpponentA"]
[WhiteElo "1210"]
[BlackElo "1300"]

1. e4 e5 1-0

[Event "Second"]
[UTCDate "2025.12.31"]
[White "OpponentB"]
[Black "diegoglozano"]
[WhiteElo "1400"]
[BlackElo "1190"]

1. d4 d5 2. c4 1-0
"#;

    #[test]
    fn applies_player_relative_filters() {
        let options = QueryOptions {
            player: Some(String::from("diegoglozano")),
            color: Some(PlayerColor::Black),
            result: Some(ResultFilter::Loss),
            minimum_rating: Some(1100),
            maximum_rating: Some(1200),
            ..QueryOptions::default()
        };
        let mut output = Vec::new();
        let summary = query(
            GAMES,
            "games.pgn",
            &options,
            QueryFormat::Jsonl,
            &mut output,
        )
        .unwrap();

        assert_eq!(summary.games, 2);
        assert_eq!(summary.matches, 1);
        let record: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(record["game"], 2);
        assert_eq!(record["date"], "2025.12.31");
        assert_eq!(record["black_elo"], 1190);
        assert_eq!(record["result"], "white_win");
        assert_eq!(record["mainline_plies"], 3);
    }

    #[test]
    fn filters_dates_and_opponents() {
        let options = QueryOptions {
            player: Some(String::from("diegoglozano")),
            opponent: Some(String::from("opponentA")),
            since: Some(parse_date("2026.01.01").unwrap()),
            until: Some(parse_date("2026.12.31").unwrap()),
            ..QueryOptions::default()
        };
        let mut output = Vec::new();
        let summary = query(GAMES, "games.pgn", &options, QueryFormat::Pgn, &mut output).unwrap();

        assert_eq!(summary.matches, 1);
        assert!(String::from_utf8_lossy(&output).contains("[Event \"First\"]"));
        assert!(!String::from_utf8_lossy(&output).contains("[Event \"Second\"]"));
    }

    #[test]
    fn matches_a_mainline_position_and_reports_its_first_ply() {
        let options = QueryOptions {
            position: Some(
                Position::from_fen(
                    b"rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 37 82",
                )
                .unwrap(),
            ),
            ..QueryOptions::default()
        };
        let mut output = Vec::new();
        let summary = query(
            GAMES,
            "games.pgn",
            &options,
            QueryFormat::Jsonl,
            &mut output,
        )
        .unwrap();

        assert_eq!(summary.games, 2);
        assert_eq!(summary.matches, 1);
        let record: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(record["game"], 1);
        assert_eq!(record["position_ply"], 2);
    }

    #[test]
    fn includes_the_starting_position_and_ignores_variations() {
        let fen_start =
            b"[SetUp \"1\"]\n[FEN \"4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 7\"]\n\n7. exd6 *\n";
        let options = QueryOptions {
            position: Some(Position::from_fen(b"4k3/8/8/3pP3/8/8/8/4K3 w - d6 50 99").unwrap()),
            ..QueryOptions::default()
        };
        let mut output = Vec::new();
        let summary = query(
            fen_start.as_slice(),
            "fen.pgn",
            &options,
            QueryFormat::Jsonl,
            &mut output,
        )
        .unwrap();
        let record: serde_json::Value = serde_json::from_slice(&output).unwrap();

        assert_eq!(summary.matches, 1);
        assert_eq!(record["position_ply"], 0);

        let variation = b"1. e4 (1. d4 d5) e5 *\n";
        let variation_options = QueryOptions {
            position: Some(
                Position::from_fen(b"rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - 0 1")
                    .unwrap(),
            ),
            ..QueryOptions::default()
        };
        let summary = query(
            variation.as_slice(),
            "variation.pgn",
            &variation_options,
            QueryFormat::Count,
            &mut Vec::new(),
        )
        .unwrap();

        assert_eq!(summary.matches, 0);
    }

    #[test]
    fn position_search_reports_invalid_fen_and_san() {
        let options = QueryOptions {
            position: Some(Position::initial()),
            ..QueryOptions::default()
        };
        let invalid_fen = query(
            b"[FEN \"not a FEN\"]\n\n*\n".as_slice(),
            "bad-fen.pgn",
            &options,
            QueryFormat::Count,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(
            invalid_fen.error,
            QueryError::InvalidFen { game: 1, .. }
        ));

        let invalid_san = query(
            b"1. e5 *\n".as_slice(),
            "bad-san.pgn",
            &options,
            QueryFormat::Count,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(
            invalid_san.error,
            QueryError::InvalidSan {
                game: 1,
                ply: 1,
                ..
            }
        ));
        assert_eq!(invalid_san.error.exit_code(), 1);
    }

    #[test]
    fn position_search_skips_explicit_non_standard_variants() {
        let options = QueryOptions {
            position: Some(Position::initial()),
            ..QueryOptions::default()
        };
        let mut output = Vec::new();
        let summary = query(
            b"[Variant \"Crazyhouse\"]\n\n1. N@e3 *\n".as_slice(),
            "variant.pgn",
            &options,
            QueryFormat::Count,
            &mut output,
        )
        .unwrap();

        assert_eq!(summary.games, 1);
        assert_eq!(summary.matches, 0);
    }

    #[test]
    fn metadata_only_queries_remain_lexical() {
        let mut output = Vec::new();
        let summary = query(
            b"[FEN \"not a FEN\"]\n\n1. e5 *\n".as_slice(),
            "lexical.pgn",
            &QueryOptions::default(),
            QueryFormat::Count,
            &mut output,
        )
        .unwrap();

        assert_eq!(summary.matches, 1);
    }

    #[test]
    fn reports_syntax_errors_with_the_game_number() {
        let mut output = Vec::new();
        let failure = query(
            b"1. e4 *\n\n1. d4 ) 1-0\n".as_slice(),
            "bad.pgn",
            &QueryOptions::default(),
            QueryFormat::Count,
            &mut output,
        )
        .unwrap_err();

        assert!(matches!(failure.error, QueryError::Parse { game: 2, .. }));
        assert_eq!(failure.summary.games, 2);
        assert_eq!(failure.summary.matches, 1);
        assert_eq!(failure.error.exit_code(), 1);
    }
}
