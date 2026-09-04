use std::borrow::Cow;
use std::fmt::{self, Write as _};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use gambit_chess::Position;
use gambit_pgn::{Event, FrameError, GameReader, Outcome, Parser, ParserOptions, Tag};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, params, params_from_iter};
use serde::Serialize;

use crate::query::{
    PlayerColor, QueryError, QueryFailure, QueryFormat, QueryOptions, QuerySummary, ResultFilter,
};

const APPLICATION_ID: i64 = 0x474d_4254;
const SCHEMA_VERSION: i64 = 1;
const COMPRESSION_LEVEL: i32 = 3;
const MAXIMUM_GAME_BYTES: usize = 16 * 1024 * 1024;

const RESULT_UNFINISHED: i64 = 0;
const RESULT_WHITE_WINS: i64 = 1;
const RESULT_BLACK_WINS: i64 = 2;
const RESULT_DRAW: i64 = 3;

const SCHEMA: &str = "
CREATE TABLE sources (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL
);
CREATE TABLE games (
    id              INTEGER PRIMARY KEY,
    source_id       INTEGER NOT NULL REFERENCES sources(id),
    source_game     INTEGER NOT NULL,
    pgn_zstd        BLOB NOT NULL,
    pgn_bytes       INTEGER NOT NULL,
    event           BLOB,
    site            BLOB,
    date_text       BLOB,
    played_on       INTEGER,
    white           BLOB,
    white_key       BLOB,
    black           BLOB,
    black_key       BLOB,
    white_elo       INTEGER,
    black_elo       INTEGER,
    result          INTEGER NOT NULL,
    mainline_plies  INTEGER NOT NULL
);
CREATE TABLE positions (
    position_key BLOB NOT NULL,
    game_id      INTEGER NOT NULL REFERENCES games(id),
    ply          INTEGER NOT NULL
);
";

const QUERY_COLUMNS: &str = "
g.id, s.name, g.source_game, g.pgn_zstd, g.pgn_bytes,
g.event, g.site, g.date_text, g.white, g.black,
g.white_elo, g.black_elo, g.result, g.mainline_plies";

#[derive(Clone, Debug, Serialize)]
pub struct IndexSummary {
    pub schema_version: u32,
    pub sources: u64,
    pub games: u64,
    pub positions: u64,
    pub pgn_bytes: u64,
    pub database_bytes: u64,
}

#[derive(Debug)]
pub enum IndexError {
    DestinationExists(PathBuf),
    Io {
        context: String,
        error: io::Error,
    },
    Database(rusqlite::Error),
    Frame {
        source: String,
        error: FrameError,
    },
    Parse {
        source: String,
        game: u64,
        error: gambit_pgn::ParseError,
    },
    InvalidFen {
        source: String,
        game: u64,
        error: gambit_chess::FenError,
    },
    InvalidSan {
        source: String,
        game: u64,
        ply: u64,
        san: String,
        error: gambit_chess::SanError,
    },
    Limit(&'static str),
}

impl fmt::Display for IndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DestinationExists(path) => {
                write!(formatter, "destination already exists: {}", path.display())
            }
            Self::Io { context, error } => write!(formatter, "{context}: {error}"),
            Self::Database(error) => write!(formatter, "database error: {error}"),
            Self::Frame { source, error } => write!(formatter, "{source}: {error}"),
            Self::Parse {
                source,
                game,
                error,
            } => write!(formatter, "{source}: game {game}: {error}"),
            Self::InvalidFen {
                source,
                game,
                error,
            } => write!(
                formatter,
                "{source}: game {game}: invalid starting FEN: {error}"
            ),
            Self::InvalidSan {
                source,
                game,
                ply,
                san,
                error,
            } => write!(
                formatter,
                "{source}: game {game}, ply {ply}: {error} ({san})"
            ),
            Self::Limit(message) => formatter.write_str(message),
        }
    }
}

impl IndexError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Frame { .. }
            | Self::Parse { .. }
            | Self::InvalidFen { .. }
            | Self::InvalidSan { .. } => 1,
            Self::DestinationExists(_) | Self::Io { .. } | Self::Database(_) | Self::Limit(_) => 3,
        }
    }
}

impl From<rusqlite::Error> for IndexError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

pub struct Builder {
    connection: Connection,
    pending: PendingDatabase,
    summary: IndexSummary,
}

impl Builder {
    pub fn create(destination: &Path) -> Result<Self, IndexError> {
        if destination.exists() {
            return Err(IndexError::DestinationExists(destination.to_path_buf()));
        }
        let pending = PendingDatabase::new(destination)?;
        let connection = Connection::open_with_flags(
            &pending.temporary,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch(&format!(
            "PRAGMA page_size = 32768;
             PRAGMA journal_mode = OFF;
             PRAGMA synchronous = OFF;
             PRAGMA temp_store = FILE;
             PRAGMA cache_size = -65536;
             PRAGMA locking_mode = EXCLUSIVE;
             PRAGMA application_id = {APPLICATION_ID};
             PRAGMA user_version = {SCHEMA_VERSION};
             PRAGMA foreign_keys = ON;
             BEGIN IMMEDIATE;
             {SCHEMA}"
        ))?;
        Ok(Self {
            connection,
            pending,
            summary: IndexSummary {
                schema_version: u32::try_from(SCHEMA_VERSION).expect("schema version fits u32"),
                sources: 0,
                games: 0,
                positions: 0,
                pgn_bytes: 0,
                database_bytes: 0,
            },
        })
    }

    pub fn add<R: Read>(&mut self, reader: R, source: &str) -> Result<(), IndexError> {
        self.connection
            .execute("INSERT INTO sources (name) VALUES (?1)", [source])?;
        let source_id = self.connection.last_insert_rowid();
        self.summary.sources += 1;

        let mut reader = GameReader::new(reader);
        let mut insert_game = self.connection.prepare_cached(
            "INSERT INTO games (
                source_id, source_game, pgn_zstd, pgn_bytes,
                event, site, date_text, played_on,
                white, white_key, black, black_key,
                white_elo, black_elo, result, mainline_plies
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
             )",
        )?;
        let mut insert_position = self.connection.prepare_cached(
            "INSERT INTO positions (position_key, game_id, ply) VALUES (?1, ?2, ?3)",
        )?;
        let mut source_game = 0_u64;
        loop {
            let game = reader.read_game().map_err(|error| IndexError::Frame {
                source: source.to_owned(),
                error,
            })?;
            let Some(game) = game else {
                self.summary.pgn_bytes += reader.bytes_read();
                return Ok(());
            };
            source_game += 1;
            let indexed = IndexedGame::parse(game, source, source_game)?;
            let compressed =
                zstd::bulk::compress(game, COMPRESSION_LEVEL).map_err(|error| IndexError::Io {
                    context: format!("failed to compress {source} game {source_game}"),
                    error,
                })?;
            let pgn_bytes = i64::try_from(game.len())
                .map_err(|_| IndexError::Limit("one PGN game is too large to index"))?;
            let source_game_i64 = i64::try_from(source_game)
                .map_err(|_| IndexError::Limit("source contains too many games"))?;
            let mainline_plies = i64::try_from(indexed.mainline_plies)
                .map_err(|_| IndexError::Limit("one game contains too many moves"))?;
            insert_game.execute(params![
                source_id,
                source_game_i64,
                compressed,
                pgn_bytes,
                indexed.event.as_deref(),
                indexed.site.as_deref(),
                indexed.date_text.as_deref(),
                indexed.played_on.map(i64::from),
                indexed.white.as_deref(),
                indexed.white_key.as_deref(),
                indexed.black.as_deref(),
                indexed.black_key.as_deref(),
                indexed.white_elo.map(i64::from),
                indexed.black_elo.map(i64::from),
                indexed.result,
                mainline_plies,
            ])?;
            let game_id = self.connection.last_insert_rowid();
            for (position_key, ply) in indexed.positions {
                insert_position.execute(params![
                    position_key.as_slice(),
                    game_id,
                    i64::from(ply)
                ])?;
                self.summary.positions += 1;
            }
            self.summary.games += 1;
        }
    }

    pub fn finish(self) -> Result<IndexSummary, IndexError> {
        let Self {
            connection,
            mut pending,
            mut summary,
        } = self;
        connection.execute_batch(
            "CREATE INDEX positions_lookup ON positions (position_key, game_id, ply);
             CREATE INDEX games_white_key ON games (white_key);
             CREATE INDEX games_black_key ON games (black_key);
             CREATE INDEX games_played_on ON games (played_on);
             CREATE INDEX games_result ON games (result);
             COMMIT;
             PRAGMA optimize;",
        )?;
        connection
            .close()
            .map_err(|(_, error)| IndexError::Database(error))?;
        File::open(&pending.temporary)
            .and_then(|file| file.sync_all())
            .map_err(|error| IndexError::Io {
                context: String::from("failed to sync completed database"),
                error,
            })?;
        summary.database_bytes = fs::metadata(&pending.temporary)
            .map_err(|error| IndexError::Io {
                context: String::from("failed to inspect completed database"),
                error,
            })?
            .len();
        pending.commit()?;
        Ok(summary)
    }
}

struct PendingDatabase {
    destination: PathBuf,
    temporary: PathBuf,
    committed: bool,
}

impl PendingDatabase {
    fn new(destination: &Path) -> Result<Self, IndexError> {
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        let file_name = destination.file_name().ok_or_else(|| IndexError::Io {
            context: format!("invalid destination: {}", destination.display()),
            error: io::Error::new(io::ErrorKind::InvalidInput, "missing file name"),
        })?;
        for sequence in 0..100_u32 {
            let mut temporary_name = file_name.to_os_string();
            temporary_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
            let temporary = parent.join(temporary_name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(_) => {
                    return Ok(Self {
                        destination: destination.to_path_buf(),
                        temporary,
                        committed: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(IndexError::Io {
                        context: format!(
                            "failed to create database beside {}",
                            destination.display()
                        ),
                        error,
                    });
                }
            }
        }
        Err(IndexError::Io {
            context: format!(
                "failed to reserve a temporary database beside {}",
                destination.display()
            ),
            error: io::Error::new(io::ErrorKind::AlreadyExists, "temporary names exhausted"),
        })
    }

    fn commit(&mut self) -> Result<(), IndexError> {
        fs::hard_link(&self.temporary, &self.destination).map_err(|error| IndexError::Io {
            context: format!(
                "failed to publish database as {}",
                self.destination.display()
            ),
            error,
        })?;
        fs::remove_file(&self.temporary).map_err(|error| IndexError::Io {
            context: format!(
                "published {}, but failed to remove its temporary link",
                self.destination.display()
            ),
            error,
        })?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for PendingDatabase {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

#[derive(Default)]
struct IndexedGame {
    event: Option<Vec<u8>>,
    site: Option<Vec<u8>>,
    date: Option<Vec<u8>>,
    utc_date: Option<Vec<u8>>,
    date_text: Option<Vec<u8>>,
    played_on: Option<u32>,
    white: Option<Vec<u8>>,
    white_key: Option<Vec<u8>>,
    black: Option<Vec<u8>>,
    black_key: Option<Vec<u8>>,
    white_elo_text: Option<Vec<u8>>,
    black_elo_text: Option<Vec<u8>>,
    white_elo: Option<u32>,
    black_elo: Option<u32>,
    fen: Option<Vec<u8>>,
    variant: Option<Vec<u8>>,
    result: i64,
    mainline_plies: u64,
    positions: Vec<([u8; 16], u32)>,
}

impl IndexedGame {
    fn parse(game_bytes: &[u8], source: &str, game: u64) -> Result<Self, IndexError> {
        let mut indexed = Self::default();
        let mut variation_depth = 0_u32;
        let mut position = Position::initial();
        let mut index_positions = false;
        for event in Parser::with_options(game_bytes, ParserOptions::STRICT) {
            let event = event.map_err(|error| IndexError::Parse {
                source: source.to_owned(),
                game,
                error,
            })?;
            match event {
                Event::Tag(tag) => indexed.observe_tag(tag),
                Event::MovetextStart { .. } => {
                    if indexed
                        .variant
                        .as_deref()
                        .is_none_or(|variant| variant.eq_ignore_ascii_case(b"standard"))
                    {
                        position = match indexed.fen.as_deref() {
                            Some(fen) => {
                                Position::from_fen(fen).map_err(|error| IndexError::InvalidFen {
                                    source: source.to_owned(),
                                    game,
                                    error,
                                })?
                            }
                            None => Position::initial(),
                        };
                        index_positions = true;
                        indexed.positions.push((position.position_key(), 0));
                    }
                }
                Event::San(token) if variation_depth == 0 => {
                    indexed.mainline_plies += 1;
                    if index_positions {
                        position.play_san(token.as_bytes()).map_err(|error| {
                            IndexError::InvalidSan {
                                source: source.to_owned(),
                                game,
                                ply: indexed.mainline_plies,
                                san: String::from_utf8_lossy(token.as_bytes()).into_owned(),
                                error,
                            }
                        })?;
                        let ply = u32::try_from(indexed.mainline_plies)
                            .map_err(|_| IndexError::Limit("one game contains too many moves"))?;
                        indexed.positions.push((position.position_key(), ply));
                    }
                }
                Event::VariationStart(_) => variation_depth += 1,
                Event::VariationEnd(_) => variation_depth -= 1,
                Event::Outcome { outcome, .. } if variation_depth == 0 => {
                    indexed.result = encode_outcome(outcome);
                }
                _ => {}
            }
        }
        indexed.date_text = indexed.date.clone().or_else(|| indexed.utc_date.clone());
        indexed.played_on = indexed.date_text.as_deref().and_then(parse_date);
        indexed.white_key = indexed.white.as_deref().map(ascii_fold);
        indexed.black_key = indexed.black.as_deref().map(ascii_fold);
        indexed.white_elo = indexed.white_elo_text.as_deref().and_then(parse_unsigned);
        indexed.black_elo = indexed.black_elo_text.as_deref().and_then(parse_unsigned);
        Ok(indexed)
    }

    fn observe_tag(&mut self, tag: Tag<'_>) {
        let destination = match tag.name() {
            b"Event" => &mut self.event,
            b"Site" => &mut self.site,
            b"Date" => &mut self.date,
            b"UTCDate" => &mut self.utc_date,
            b"White" => &mut self.white,
            b"Black" => &mut self.black,
            b"FEN" => &mut self.fen,
            b"Variant" => &mut self.variant,
            b"WhiteElo" => &mut self.white_elo_text,
            b"BlackElo" => &mut self.black_elo_text,
            _ => return,
        };
        if destination.is_none() {
            *destination = Some(tag.value().into_owned());
        }
    }
}

fn encode_outcome(outcome: Outcome) -> i64 {
    match outcome {
        Outcome::Unknown => RESULT_UNFINISHED,
        Outcome::WhiteWins => RESULT_WHITE_WINS,
        Outcome::BlackWins => RESULT_BLACK_WINS,
        Outcome::Draw => RESULT_DRAW,
    }
}

fn parse_date(value: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(value).ok()?;
    crate::query::parse_date(text)
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

fn ascii_fold(value: &[u8]) -> Vec<u8> {
    value.iter().map(u8::to_ascii_lowercase).collect()
}

pub fn query(
    path: &Path,
    options: &QueryOptions,
    format: QueryFormat,
    output: &mut impl Write,
) -> Result<QuerySummary, QueryFailure> {
    query_inner(path, options, format, output).map_err(|error| QueryFailure {
        summary: QuerySummary::default(),
        error,
    })
}

fn query_inner(
    path: &Path,
    options: &QueryOptions,
    format: QueryFormat,
    output: &mut impl Write,
) -> Result<QuerySummary, QueryError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| database_query_error(&error))?;
    validate_database(&connection)?;
    let (from, predicates, values, position_ply) = build_query(options);
    let where_clause = if predicates.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", predicates.join(" AND "))
    };

    if format == QueryFormat::Count {
        let sql = format!("SELECT COUNT(*) {from}{where_clause}");
        let matches: i64 = connection
            .query_row(&sql, params_from_iter(values), |row| row.get(0))
            .map_err(|error| database_query_error(&error))?;
        return Ok(QuerySummary {
            bytes: 0,
            games: u64::try_from(matches).unwrap_or(0),
            matches: u64::try_from(matches).unwrap_or(0),
        });
    }

    let sql = format!(
        "SELECT {QUERY_COLUMNS}, {position_ply} AS position_ply {from}{where_clause} ORDER BY g.id"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| database_query_error(&error))?;
    let mut rows = statement
        .query(params_from_iter(values))
        .map_err(|error| database_query_error(&error))?;
    let mut summary = QuerySummary::default();
    while let Some(row) = rows.next().map_err(|error| database_query_error(&error))? {
        let record = StoredGame::from_row(row).map_err(|error| database_query_error(&error))?;
        summary.games += 1;
        summary.matches += 1;
        summary.bytes += record.pgn_bytes;
        match format {
            QueryFormat::Pgn => {
                let maximum = usize::try_from(record.pgn_bytes)
                    .map_err(|_| QueryError::Database(String::from("invalid stored PGN size")))?;
                if maximum > MAXIMUM_GAME_BYTES {
                    return Err(QueryError::Database(String::from(
                        "stored PGN exceeds the 16 MiB game limit",
                    )));
                }
                let pgn = zstd::bulk::decompress(&record.pgn_zstd, maximum)
                    .map_err(|error| QueryError::Database(error.to_string()))?;
                if pgn.len() != maximum {
                    return Err(QueryError::Database(String::from(
                        "stored PGN size does not match its metadata",
                    )));
                }
                output.write_all(&pgn).map_err(QueryError::Output)?;
                output.write_all(b"\n\n").map_err(QueryError::Output)?;
            }
            QueryFormat::Jsonl => {
                let json = record.as_json();
                serde_json::to_writer(&mut *output, &json)
                    .map_err(|error| QueryError::Output(io::Error::other(error)))?;
                output.write_all(b"\n").map_err(QueryError::Output)?;
            }
            QueryFormat::Count => unreachable!("count queries return before row decoding"),
        }
    }
    Ok(summary)
}

fn database_query_error(error: &rusqlite::Error) -> QueryError {
    QueryError::Database(error.to_string())
}

fn validate_database(connection: &Connection) -> Result<(), QueryError> {
    let application_id: i64 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|error| database_query_error(&error))?;
    let schema_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| database_query_error(&error))?;
    if application_id != APPLICATION_ID {
        return Err(QueryError::Database(String::from(
            "file is not a Gambit database",
        )));
    }
    if schema_version != SCHEMA_VERSION {
        return Err(QueryError::Database(format!(
            "unsupported schema version {schema_version}; this Gambit supports version {SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

fn push_value(values: &mut Vec<Value>, value: Value) -> String {
    values.push(value);
    format!("?{}", values.len())
}

fn build_query(options: &QueryOptions) -> (String, Vec<String>, Vec<Value>, String) {
    let mut values = Vec::new();
    let mut predicates = Vec::new();
    let mut from = String::from("FROM games g JOIN sources s ON s.id = g.source_id");
    let mut position_ply = String::from("NULL");
    if let Some(position) = options.position {
        let parameter = push_value(&mut values, Value::Blob(position.position_key().to_vec()));
        write!(
            from,
            " JOIN (
                SELECT game_id, MIN(ply) AS ply
                FROM positions
                WHERE position_key = {parameter}
                GROUP BY game_id
              ) p ON p.game_id = g.id"
        )
        .expect("writing SQL to a string cannot fail");
        position_ply = String::from("p.ply");
    }

    if let Some(player) = options.player.as_deref() {
        let player = push_value(&mut values, Value::Blob(ascii_fold(player.as_bytes())));
        let opponent = options
            .opponent
            .as_deref()
            .map(|value| push_value(&mut values, Value::Blob(ascii_fold(value.as_bytes()))));
        let mut white = vec![format!("g.white_key = {player}")];
        let mut black = vec![
            format!("g.black_key = {player}"),
            format!("(g.white_key IS NULL OR g.white_key != {player})"),
        ];
        if let Some(opponent) = opponent {
            white.push(format!("g.black_key = {opponent}"));
            black.push(format!("g.white_key = {opponent}"));
        }
        if let Some(result) = options.result {
            let (white_result, black_result) = match result {
                ResultFilter::Win => (RESULT_WHITE_WINS, RESULT_BLACK_WINS),
                ResultFilter::Loss => (RESULT_BLACK_WINS, RESULT_WHITE_WINS),
                ResultFilter::Draw => (RESULT_DRAW, RESULT_DRAW),
                ResultFilter::Unfinished => (RESULT_UNFINISHED, RESULT_UNFINISHED),
            };
            white.push(format!("g.result = {white_result}"));
            black.push(format!("g.result = {black_result}"));
        }
        if let Some(minimum) = options.minimum_rating {
            let parameter = push_value(&mut values, Value::Integer(i64::from(minimum)));
            white.push(format!("g.white_elo >= {parameter}"));
            black.push(format!("g.black_elo >= {parameter}"));
        }
        if let Some(maximum) = options.maximum_rating {
            let parameter = push_value(&mut values, Value::Integer(i64::from(maximum)));
            white.push(format!("g.white_elo <= {parameter}"));
            black.push(format!("g.black_elo <= {parameter}"));
        }
        let player_predicate = match options.color {
            Some(PlayerColor::White) => format!("({})", white.join(" AND ")),
            Some(PlayerColor::Black) => format!("({})", black.join(" AND ")),
            None => format!("(({}) OR ({}))", white.join(" AND "), black.join(" AND ")),
        };
        predicates.push(player_predicate);
    } else if let Some(result) = options.result {
        let result = match result {
            ResultFilter::Draw => RESULT_DRAW,
            ResultFilter::Unfinished => RESULT_UNFINISHED,
            ResultFilter::Win | ResultFilter::Loss => {
                unreachable!("win and loss filters require a player")
            }
        };
        predicates.push(format!("g.result = {result}"));
    }

    if let Some(since) = options.since {
        let parameter = push_value(&mut values, Value::Integer(i64::from(since)));
        predicates.push(format!("g.played_on >= {parameter}"));
    }
    if let Some(until) = options.until {
        let parameter = push_value(&mut values, Value::Integer(i64::from(until)));
        predicates.push(format!("g.played_on <= {parameter}"));
    }
    (from, predicates, values, position_ply)
}

struct StoredGame {
    source: String,
    source_game: u64,
    pgn_zstd: Vec<u8>,
    pgn_bytes: u64,
    event: Option<Vec<u8>>,
    site: Option<Vec<u8>>,
    date: Option<Vec<u8>>,
    white: Option<Vec<u8>>,
    black: Option<Vec<u8>>,
    white_elo: Option<u32>,
    black_elo: Option<u32>,
    result: i64,
    mainline_plies: u64,
    position_ply: Option<u64>,
}

impl StoredGame {
    fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            source: row.get(1)?,
            source_game: to_u64(row.get(2)?)?,
            pgn_zstd: row.get(3)?,
            pgn_bytes: to_u64(row.get(4)?)?,
            event: row.get(5)?,
            site: row.get(6)?,
            date: row.get(7)?,
            white: row.get(8)?,
            black: row.get(9)?,
            white_elo: to_optional_u32(row.get(10)?)?,
            black_elo: to_optional_u32(row.get(11)?)?,
            result: row.get(12)?,
            mainline_plies: to_u64(row.get(13)?)?,
            position_ply: row.get::<_, Option<i64>>(14)?.map(to_u64).transpose()?,
        })
    }

    fn as_json(&self) -> MatchRecord<'_> {
        MatchRecord {
            schema_version: 1,
            source: &self.source,
            game: self.source_game,
            event: self.event.as_deref().map(lossy),
            site: self.site.as_deref().map(lossy),
            date: self.date.as_deref().map(lossy),
            white: self.white.as_deref().map(lossy),
            black: self.black.as_deref().map(lossy),
            white_elo: self.white_elo,
            black_elo: self.black_elo,
            result: decode_outcome(self.result),
            mainline_plies: self.mainline_plies,
            position_ply: self.position_ply,
        }
    }
}

fn to_u64(value: i64) -> Result<u64, rusqlite::Error> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn to_optional_u32(value: Option<i64>) -> Result<Option<u32>, rusqlite::Error> {
    value
        .map(|value| {
            u32::try_from(value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn decode_outcome(value: i64) -> Option<&'static str> {
    match value {
        RESULT_UNFINISHED => Some("unfinished"),
        RESULT_WHITE_WINS => Some("white_win"),
        RESULT_BLACK_WINS => Some("black_win"),
        RESULT_DRAW => Some("draw"),
        _ => None,
    }
}

fn lossy(value: &[u8]) -> Cow<'_, str> {
    String::from_utf8_lossy(value)
}

#[derive(Serialize)]
struct MatchRecord<'a> {
    schema_version: u8,
    source: &'a str,
    game: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    white: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    black: Option<Cow<'a, str>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_only_ascii_case() {
        assert_eq!(ascii_fold(b"DiegoGLozano"), b"diegoglozano");
        assert_eq!(ascii_fold(&[0xc3, 0x89]), [0xc3, 0x89]);
    }

    #[test]
    fn builds_player_relative_sql_without_losing_shared_parameters() {
        let options = QueryOptions {
            player: Some(String::from("Diego")),
            opponent: Some(String::from("Other")),
            result: Some(ResultFilter::Loss),
            minimum_rating: Some(1200),
            ..QueryOptions::default()
        };
        let (_, predicates, values, _) = build_query(&options);
        let sql = predicates.join(" AND ");
        assert!(sql.contains("g.white_key = ?1"));
        assert!(sql.contains("g.black_key = ?1"));
        assert!(sql.contains("g.white_key IS NULL OR g.white_key != ?1"));
        assert!(sql.contains("g.black_key = ?2"));
        assert!(sql.contains("g.white_key = ?2"));
        assert!(sql.contains("g.white_elo >= ?3"));
        assert!(sql.contains("g.black_elo >= ?3"));
        assert_eq!(values.len(), 3);
    }

    #[test]
    fn preserves_first_tag_semantics_for_invalid_ratings() {
        let indexed = IndexedGame::parse(
            b"[WhiteElo \"?\"]\n[WhiteElo \"2000\"]\n[BlackElo \"1800\"]\n\n*\n",
            "test",
            1,
        )
        .unwrap();
        assert_eq!(indexed.white_elo, None);
        assert_eq!(indexed.black_elo, Some(1800));
    }
}
