use std::borrow::Cow;
use std::fmt::{self, Write as _};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use gambit_chess::{Color, Piece, Position, Square};
use gambit_pgn::{Event, FrameError, GameReader, Outcome, Parser, ParserOptions, Tag};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, params, params_from_iter};
use serde::Serialize;

use crate::query::{
    PlayerColor, QueryError, QueryFailure, QueryFormat, QueryOptions, QuerySummary, ResultFilter,
};

const APPLICATION_ID: i64 = 0x474d_4254;
const SCHEMA_VERSION: i64 = 2;
const OLDEST_QUERYABLE_SCHEMA_VERSION: i64 = 1;
const COMPRESSION_LEVEL: i32 = 3;
const MAXIMUM_GAME_BYTES: usize = 16 * 1024 * 1024;

const RESULT_UNFINISHED: i64 = 0;
const RESULT_WHITE_WINS: i64 = 1;
const RESULT_BLACK_WINS: i64 = 2;
const RESULT_DRAW: i64 = 3;

const SCHEMA: &str = "
CREATE TABLE sources (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    fingerprint BLOB
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
    pub scanned_pgn_bytes: u64,
    pub database_bytes: u64,
    pub skipped_sources: u64,
    pub replaced_sources: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DatabaseResultCounts {
    pub white_wins: u64,
    pub black_wins: u64,
    pub draws: u64,
    pub unfinished: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DatabaseInfo {
    pub schema_version: u32,
    pub database_bytes: u64,
    pub sources: u64,
    pub fingerprinted_sources: u64,
    pub games: u64,
    pub positions: u64,
    pub mainline_plies: u64,
    pub pgn_bytes: u64,
    pub compressed_pgn_bytes: u64,
    pub earliest_date: Option<u32>,
    pub latest_date: Option<u32>,
    pub results: DatabaseResultCounts,
    pub integrity_checked: bool,
    pub integrity_issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GameSummary {
    pub id: i64,
    pub source: String,
    pub source_game: u64,
    pub event: Option<String>,
    pub site: Option<String>,
    pub date: Option<String>,
    pub white: Option<String>,
    pub black: Option<String>,
    pub white_elo: Option<u32>,
    pub black_elo: Option<u32>,
    pub result: Option<String>,
    pub mainline_plies: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct GamePage {
    pub total: u64,
    pub offset: u64,
    pub limit: u32,
    pub games: Vec<GameSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GameMove {
    pub ply: u32,
    pub san: String,
    pub from: String,
    pub to: String,
    pub board: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GameDetail {
    pub summary: GameSummary,
    pub pgn: String,
    pub initial_board: Option<String>,
    pub moves: Vec<GameMove>,
}

#[derive(Debug)]
pub enum LibraryError {
    Database(String),
    GameNotFound(i64),
    InvalidGame(String),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "failed to read Gambit database: {error}"),
            Self::GameNotFound(id) => write!(formatter, "game {id} was not found"),
            Self::InvalidGame(error) => write!(formatter, "failed to decode stored game: {error}"),
        }
    }
}

impl std::error::Error for LibraryError {}

impl DatabaseInfo {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.integrity_issues.is_empty())
    }
}

#[derive(Debug)]
pub enum InfoError {
    Io { context: String, error: io::Error },
    Database(String),
}

impl fmt::Display for InfoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, error } => write!(formatter, "{context}: {error}"),
            Self::Database(error) => {
                write!(formatter, "failed to inspect Gambit database: {error}")
            }
        }
    }
}

impl InfoError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Io { .. } | Self::Database(_) => 3,
        }
    }
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
    AmbiguousSource(String),
    SourceChanged(String),
    UnsupportedSchema(i64),
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
            Self::AmbiguousSource(source) => write!(
                formatter,
                "database contains more than one source named {source:?}; rebuild it before updating"
            ),
            Self::SourceChanged(source) => write!(
                formatter,
                "{source} changed while it was being indexed; retry the update"
            ),
            Self::UnsupportedSchema(version) => write!(
                formatter,
                "unsupported database schema version {version}; this Gambit supports versions {OLDEST_QUERYABLE_SCHEMA_VERSION} through {SCHEMA_VERSION}"
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
            Self::DestinationExists(_)
            | Self::Io { .. }
            | Self::Database(_)
            | Self::AmbiguousSource(_)
            | Self::SourceChanged(_)
            | Self::UnsupportedSchema(_)
            | Self::Limit(_) => 3,
        }
    }
}

impl From<rusqlite::Error> for IndexError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFingerprint {
    digest: [u8; 32],
    pub pgn_bytes: u64,
}

struct SourceFingerprinter {
    hasher: blake3::Hasher,
}

impl SourceFingerprinter {
    fn new() -> Self {
        Self {
            hasher: blake3::Hasher::new_derive_key("gambit source fingerprint v1"),
        }
    }

    fn observe(&mut self, game: &[u8]) {
        let length = u64::try_from(game.len()).expect("PGN game length fits u64");
        self.hasher.update(&length.to_le_bytes());
        self.hasher.update(game);
    }

    fn finish(self, pgn_bytes: u64) -> SourceFingerprint {
        SourceFingerprint {
            digest: *self.hasher.finalize().as_bytes(),
            pgn_bytes,
        }
    }
}

pub fn fingerprint<R: Read>(reader: R, source: &str) -> Result<SourceFingerprint, IndexError> {
    let mut reader = GameReader::new(reader);
    let mut fingerprinter = SourceFingerprinter::new();
    loop {
        let game = reader.read_game().map_err(|error| IndexError::Frame {
            source: source.to_owned(),
            error,
        })?;
        let Some(game) = game else {
            return Ok(fingerprinter.finish(reader.bytes_read()));
        };
        fingerprinter.observe(game);
    }
}

pub struct Builder {
    connection: Connection,
    pending: PendingDatabase,
    summary: IndexSummary,
}

/// Builds a new Gambit database from one or more PGN or `.pgn.zst` files.
pub fn build_database_from_files<I, P>(
    paths: I,
    destination: &Path,
) -> Result<IndexSummary, IndexError>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut builder = Builder::create(destination)?;
    for path in paths {
        let path = path.as_ref();
        let source = path.to_string_lossy().into_owned();
        let file = File::open(path).map_err(|error| IndexError::Io {
            context: format!("failed to open {source}"),
            error,
        })?;
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zst"))
        {
            let decoder =
                zstd::stream::read::Decoder::new(file).map_err(|error| IndexError::Io {
                    context: format!("failed to initialize zstd decoder for {source}"),
                    error,
                })?;
            builder.add(decoder, &source)?;
        } else {
            builder.add(file, &source)?;
        }
    }
    builder.finish()
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
                scanned_pgn_bytes: 0,
                database_bytes: 0,
                skipped_sources: 0,
                replaced_sources: 0,
            },
        })
    }

    pub fn add<R: Read>(&mut self, reader: R, source: &str) -> Result<(), IndexError> {
        let fingerprint = write_source(&self.connection, &mut self.summary, reader, source)?;
        self.summary.scanned_pgn_bytes += fingerprint.pgn_bytes;
        Ok(())
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
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pending.temporary)
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

fn write_source<R: Read>(
    connection: &Connection,
    summary: &mut IndexSummary,
    reader: R,
    source: &str,
) -> Result<SourceFingerprint, IndexError> {
    connection.execute("INSERT INTO sources (name) VALUES (?1)", [source])?;
    let source_id = connection.last_insert_rowid();
    summary.sources += 1;

    let mut reader = GameReader::new(reader);
    let mut fingerprinter = SourceFingerprinter::new();
    let mut insert_game = connection.prepare_cached(
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
    let mut insert_position = connection
        .prepare_cached("INSERT INTO positions (position_key, game_id, ply) VALUES (?1, ?2, ?3)")?;
    let mut source_game = 0_u64;
    loop {
        let game = reader.read_game().map_err(|error| IndexError::Frame {
            source: source.to_owned(),
            error,
        })?;
        let Some(game) = game else {
            let fingerprint = fingerprinter.finish(reader.bytes_read());
            connection.execute(
                "UPDATE sources SET fingerprint = ?1 WHERE id = ?2",
                params![fingerprint.digest.as_slice(), source_id],
            )?;
            summary.pgn_bytes += fingerprint.pgn_bytes;
            return Ok(fingerprint);
        };
        fingerprinter.observe(game);
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
        let game_id = connection.last_insert_rowid();
        for (position_key, ply) in indexed.positions {
            insert_position.execute(params![position_key.as_slice(), game_id, i64::from(ply)])?;
            summary.positions += 1;
        }
        summary.games += 1;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateAction {
    Unchanged,
    Write,
}

pub struct Updater {
    connection: Connection,
    path: PathBuf,
    summary: IndexSummary,
}

impl Updater {
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let application_id: i64 =
            connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
        if application_id != APPLICATION_ID {
            return Err(IndexError::Limit("file is not a Gambit database"));
        }
        let schema_version: i64 =
            connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(OLDEST_QUERYABLE_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&schema_version) {
            return Err(IndexError::UnsupportedSchema(schema_version));
        }
        connection.execute_batch(
            "PRAGMA journal_mode = DELETE;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             BEGIN IMMEDIATE;",
        )?;
        if schema_version == 1 {
            connection.execute_batch(
                "ALTER TABLE sources ADD COLUMN fingerprint BLOB;
                 PRAGMA user_version = 2;",
            )?;
        }
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            summary: IndexSummary {
                schema_version: u32::try_from(SCHEMA_VERSION).expect("schema version fits u32"),
                sources: 0,
                games: 0,
                positions: 0,
                pgn_bytes: 0,
                scanned_pgn_bytes: 0,
                database_bytes: 0,
                skipped_sources: 0,
                replaced_sources: 0,
            },
        })
    }

    pub fn prepare(
        &mut self,
        source: &str,
        fingerprint: &SourceFingerprint,
    ) -> Result<UpdateAction, IndexError> {
        self.summary.scanned_pgn_bytes += fingerprint.pgn_bytes;
        let matches = {
            let mut statement = self.connection.prepare(
                "SELECT id, fingerprint FROM sources WHERE name = ?1 ORDER BY id LIMIT 2",
            )?;
            let rows = statement.query_map([source], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let Some((source_id, stored_digest)) = matches.first() else {
            return Ok(UpdateAction::Write);
        };
        if matches.len() > 1 {
            return Err(IndexError::AmbiguousSource(source.to_owned()));
        }
        let stored_fingerprint = match stored_digest {
            Some(digest) if digest.len() == 32 => digest.clone(),
            Some(_) => {
                return Err(IndexError::Limit(
                    "database contains an invalid source fingerprint",
                ));
            }
            None => {
                let recovered = self.stored_fingerprint(*source_id)?;
                self.connection.execute(
                    "UPDATE sources SET fingerprint = ?1 WHERE id = ?2",
                    params![recovered.digest.as_slice(), source_id],
                )?;
                recovered.digest.to_vec()
            }
        };
        if stored_fingerprint == fingerprint.digest {
            self.summary.skipped_sources += 1;
            return Ok(UpdateAction::Unchanged);
        }

        self.connection.execute(
            "DELETE FROM positions WHERE game_id IN (SELECT id FROM games WHERE source_id = ?1)",
            [source_id],
        )?;
        self.connection
            .execute("DELETE FROM games WHERE source_id = ?1", [source_id])?;
        self.connection
            .execute("DELETE FROM sources WHERE id = ?1", [source_id])?;
        self.summary.replaced_sources += 1;
        Ok(UpdateAction::Write)
    }

    fn stored_fingerprint(&self, source_id: i64) -> Result<SourceFingerprint, IndexError> {
        let mut fingerprinter = SourceFingerprinter::new();
        let mut pgn_bytes = 0_u64;
        let mut statement = self.connection.prepare(
            "SELECT pgn_zstd, pgn_bytes FROM games WHERE source_id = ?1 ORDER BY source_game",
        )?;
        let mut rows = statement.query([source_id])?;
        while let Some(row) = rows.next()? {
            let compressed: Vec<u8> = row.get(0)?;
            let bytes: i64 = row.get(1)?;
            let bytes = usize::try_from(bytes)
                .map_err(|_| IndexError::Limit("database contains an invalid stored PGN size"))?;
            if bytes > MAXIMUM_GAME_BYTES {
                return Err(IndexError::Limit(
                    "stored PGN exceeds the 16 MiB game limit",
                ));
            }
            let game =
                zstd::bulk::decompress(&compressed, bytes).map_err(|error| IndexError::Io {
                    context: String::from("failed to decompress stored PGN"),
                    error,
                })?;
            if game.len() != bytes {
                return Err(IndexError::Limit(
                    "stored PGN size does not match its metadata",
                ));
            }
            fingerprinter.observe(&game);
            pgn_bytes += u64::try_from(bytes).expect("usize fits u64 on supported targets");
        }
        Ok(fingerprinter.finish(pgn_bytes))
    }

    pub fn add<R: Read>(
        &mut self,
        reader: R,
        source: &str,
        expected: &SourceFingerprint,
    ) -> Result<(), IndexError> {
        let actual = write_source(&self.connection, &mut self.summary, reader, source)?;
        if actual.digest != expected.digest {
            return Err(IndexError::SourceChanged(source.to_owned()));
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<IndexSummary, IndexError> {
        self.connection.execute_batch("COMMIT; PRAGMA optimize;")?;
        self.connection
            .close()
            .map_err(|(_, error)| IndexError::Database(error))?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .and_then(|file| file.sync_all())
            .map_err(|error| IndexError::Io {
                context: String::from("failed to sync updated database"),
                error,
            })?;
        self.summary.database_bytes = fs::metadata(&self.path)
            .map_err(|error| IndexError::Io {
                context: String::from("failed to inspect updated database"),
                error,
            })?
            .len();
        Ok(self.summary)
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
    if !(OLDEST_QUERYABLE_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&schema_version) {
        return Err(QueryError::Database(format!(
            "unsupported schema version {schema_version}; this Gambit supports versions {OLDEST_QUERYABLE_SCHEMA_VERSION} through {SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

pub fn info(path: &Path, check_integrity: bool) -> Result<DatabaseInfo, InfoError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(info_database_error)?;
    let schema_version = info_schema_version(&connection)?;
    let sources: i64 = connection
        .query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))
        .map_err(info_database_error)?;
    let fingerprinted_sources = if schema_version >= 2 {
        connection
            .query_row(
                "SELECT COUNT(*) FROM sources WHERE fingerprint IS NOT NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(info_database_error)?
    } else {
        0
    };
    let positions: i64 = connection
        .query_row("SELECT COUNT(*) FROM positions", [], |row| row.get(0))
        .map_err(info_database_error)?;
    let aggregate = read_info_aggregate(&connection)?;
    let mut integrity_issues = Vec::new();
    if check_integrity {
        collect_integrity_issues(&connection, schema_version, &mut integrity_issues)?;
    }
    let database_bytes = fs::metadata(path)
        .map_err(|error| InfoError::Io {
            context: format!("failed to inspect {}", path.display()),
            error,
        })?
        .len();
    Ok(DatabaseInfo {
        schema_version: info_u32(schema_version, "schema version")?,
        database_bytes,
        sources: info_u64(sources, "source count")?,
        fingerprinted_sources: info_u64(fingerprinted_sources, "fingerprinted source count")?,
        games: info_u64(aggregate.games, "game count")?,
        positions: info_u64(positions, "position count")?,
        mainline_plies: info_u64(aggregate.mainline_plies, "mainline ply count")?,
        pgn_bytes: info_u64(aggregate.pgn_bytes, "PGN byte count")?,
        compressed_pgn_bytes: info_u64(
            aggregate.compressed_pgn_bytes,
            "compressed PGN byte count",
        )?,
        earliest_date: aggregate
            .earliest_date
            .map(|value| info_u32(value, "earliest date"))
            .transpose()?,
        latest_date: aggregate
            .latest_date
            .map(|value| info_u32(value, "latest date"))
            .transpose()?,
        results: DatabaseResultCounts {
            white_wins: info_u64(aggregate.white_wins, "white-win count")?,
            black_wins: info_u64(aggregate.black_wins, "black-win count")?,
            draws: info_u64(aggregate.draws, "draw count")?,
            unfinished: info_u64(aggregate.unfinished, "unfinished count")?,
        },
        integrity_checked: check_integrity,
        integrity_issues,
    })
}

fn info_schema_version(connection: &Connection) -> Result<i64, InfoError> {
    let application_id: i64 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(info_database_error)?;
    if application_id != APPLICATION_ID {
        return Err(InfoError::Database(String::from(
            "file is not a Gambit database",
        )));
    }
    let schema_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(info_database_error)?;
    if !(OLDEST_QUERYABLE_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&schema_version) {
        return Err(InfoError::Database(format!(
            "unsupported schema version {schema_version}; this Gambit supports versions {OLDEST_QUERYABLE_SCHEMA_VERSION} through {SCHEMA_VERSION}"
        )));
    }
    Ok(schema_version)
}

struct InfoAggregate {
    games: i64,
    mainline_plies: i64,
    pgn_bytes: i64,
    compressed_pgn_bytes: i64,
    earliest_date: Option<i64>,
    latest_date: Option<i64>,
    white_wins: i64,
    black_wins: i64,
    draws: i64,
    unfinished: i64,
}

fn read_info_aggregate(connection: &Connection) -> Result<InfoAggregate, InfoError> {
    connection
        .query_row(
            "SELECT
                COUNT(*),
                COALESCE(SUM(mainline_plies), 0),
                COALESCE(SUM(pgn_bytes), 0),
                COALESCE(SUM(LENGTH(pgn_zstd)), 0),
                MIN(played_on),
                MAX(played_on),
                COALESCE(SUM(result = 1), 0),
                COALESCE(SUM(result = 2), 0),
                COALESCE(SUM(result = 3), 0),
                COALESCE(SUM(result = 0), 0)
             FROM games",
            [],
            |row| {
                Ok(InfoAggregate {
                    games: row.get(0)?,
                    mainline_plies: row.get(1)?,
                    pgn_bytes: row.get(2)?,
                    compressed_pgn_bytes: row.get(3)?,
                    earliest_date: row.get(4)?,
                    latest_date: row.get(5)?,
                    white_wins: row.get(6)?,
                    black_wins: row.get(7)?,
                    draws: row.get(8)?,
                    unfinished: row.get(9)?,
                })
            },
        )
        .map_err(info_database_error)
}

fn collect_integrity_issues(
    connection: &Connection,
    schema_version: i64,
    issues: &mut Vec<String>,
) -> Result<(), InfoError> {
    const MAXIMUM_ISSUES: usize = 10;
    let mut statement = connection
        .prepare("PRAGMA quick_check(10)")
        .map_err(info_database_error)?;
    let checks = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(info_database_error)?;
    for check in checks {
        let check = check.map_err(info_database_error)?;
        if check != "ok" {
            issues.push(check);
        }
    }
    drop(statement);

    let mut statement = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(info_database_error)?;
    let mut rows = statement.query([]).map_err(info_database_error)?;
    while issues.len() < MAXIMUM_ISSUES {
        let Some(row) = rows.next().map_err(info_database_error)? else {
            break;
        };
        let table: String = row.get(0).map_err(info_database_error)?;
        let row_id: Option<i64> = row.get(1).map_err(info_database_error)?;
        let parent: String = row.get(2).map_err(info_database_error)?;
        issues.push(format!(
            "foreign key violation in {table} row {} referencing {parent}",
            row_id.map_or_else(|| String::from("unknown"), |value| value.to_string())
        ));
    }
    if rows.next().map_err(info_database_error)?.is_some() {
        issues.push(String::from("additional integrity issues omitted"));
    }
    drop(rows);
    drop(statement);
    if issues.len() < MAXIMUM_ISSUES {
        check_stored_pgn(connection, schema_version, issues, MAXIMUM_ISSUES)?;
    }
    Ok(())
}

struct CheckedSource {
    id: i64,
    name: String,
    expected_digest: Option<Vec<u8>>,
    fingerprinter: SourceFingerprinter,
    valid: bool,
}

fn check_stored_pgn(
    connection: &Connection,
    schema_version: i64,
    issues: &mut Vec<String>,
    maximum_issues: usize,
) -> Result<(), InfoError> {
    let fingerprint = if schema_version >= 2 {
        "s.fingerprint"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT s.id, s.name, {fingerprint}, g.source_game, g.pgn_zstd, g.pgn_bytes
         FROM sources s
         LEFT JOIN games g ON g.source_id = s.id
         ORDER BY s.id, g.source_game"
    );
    let mut statement = connection.prepare(&sql).map_err(info_database_error)?;
    let mut rows = statement.query([]).map_err(info_database_error)?;
    let mut source: Option<CheckedSource> = None;
    while issues.len() < maximum_issues {
        let Some(row) = rows.next().map_err(info_database_error)? else {
            break;
        };
        let source_id: i64 = row.get(0).map_err(info_database_error)?;
        if source.as_ref().is_some_and(|source| source.id != source_id) {
            finish_checked_source(source.take().expect("checked source exists"), issues);
            if issues.len() >= maximum_issues {
                break;
            }
        }
        if source.is_none() {
            source = Some(CheckedSource {
                id: source_id,
                name: row.get(1).map_err(info_database_error)?,
                expected_digest: row.get(2).map_err(info_database_error)?,
                fingerprinter: SourceFingerprinter::new(),
                valid: true,
            });
        }
        let Some(source_game) = row.get::<_, Option<i64>>(3).map_err(info_database_error)? else {
            continue;
        };
        let compressed: Vec<u8> = row.get(4).map_err(info_database_error)?;
        let stored_bytes: i64 = row.get(5).map_err(info_database_error)?;
        let Some(bytes) = usize::try_from(stored_bytes)
            .ok()
            .filter(|bytes| *bytes <= MAXIMUM_GAME_BYTES)
        else {
            issues.push(format!(
                "{} game {source_game} has an invalid stored PGN size",
                source.as_ref().expect("checked source exists").name
            ));
            source.as_mut().expect("checked source exists").valid = false;
            continue;
        };
        match zstd::bulk::decompress(&compressed, bytes) {
            Ok(game) if game.len() == bytes => source
                .as_mut()
                .expect("checked source exists")
                .fingerprinter
                .observe(&game),
            Ok(_) => {
                issues.push(format!(
                    "{} game {source_game} PGN size does not match its metadata",
                    source.as_ref().expect("checked source exists").name
                ));
                source.as_mut().expect("checked source exists").valid = false;
            }
            Err(error) => {
                issues.push(format!(
                    "{} game {source_game} has invalid compressed PGN: {error}",
                    source.as_ref().expect("checked source exists").name
                ));
                source.as_mut().expect("checked source exists").valid = false;
            }
        }
    }
    if let Some(source) = source {
        finish_checked_source(source, issues);
    }
    Ok(())
}

fn finish_checked_source(source: CheckedSource, issues: &mut Vec<String>) {
    let Some(expected) = source.expected_digest else {
        return;
    };
    if expected.len() != 32 {
        issues.push(format!("{} has an invalid source fingerprint", source.name));
        return;
    }
    if source.valid && source.fingerprinter.finish(0).digest != expected.as_slice() {
        issues.push(format!("{} source fingerprint does not match", source.name));
    }
}

fn info_database_error(error: rusqlite::Error) -> InfoError {
    let message = format!("database error: {error}");
    drop(error);
    InfoError::Database(message)
}

fn info_u64(value: i64, field: &str) -> Result<u64, InfoError> {
    u64::try_from(value)
        .map_err(|_| InfoError::Database(format!("database contains an invalid {field}")))
}

fn info_u32(value: i64, field: &str) -> Result<u32, InfoError> {
    u32::try_from(value)
        .map_err(|_| InfoError::Database(format!("database contains an invalid {field}")))
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

/// Returns a newest-first page of games for the desktop and other structured clients.
///
/// `player` is matched case-insensitively against either player name. The page size is
/// clamped to 200 records so callers cannot accidentally materialize an entire corpus.
pub fn list_games(
    path: &Path,
    player: Option<&str>,
    offset: u64,
    limit: u32,
) -> Result<GamePage, LibraryError> {
    let connection = open_library_database(path)?;
    let player_key = player.map(|value| Value::Blob(ascii_fold(value.as_bytes())));
    let predicate = if player_key.is_some() {
        " WHERE g.white_key = ?1 OR g.black_key = ?1"
    } else {
        ""
    };
    let values = player_key.into_iter().collect::<Vec<_>>();
    let count_sql = format!("SELECT COUNT(*) FROM games g{predicate}");
    let total: i64 = connection
        .query_row(&count_sql, params_from_iter(values.iter()), |row| {
            row.get(0)
        })
        .map_err(library_database_error)?;

    let limit = limit.clamp(1, 200);
    let sql = format!(
        "SELECT {QUERY_COLUMNS}, NULL AS position_ply
         FROM games g JOIN sources s ON s.id = g.source_id
         {predicate}
         ORDER BY g.played_on IS NULL, g.played_on DESC, g.id DESC
         LIMIT ?{} OFFSET ?{}",
        values.len() + 1,
        values.len() + 2
    );
    let mut page_values = values;
    page_values.push(Value::Integer(i64::from(limit)));
    page_values.push(Value::Integer(i64::try_from(offset).unwrap_or(i64::MAX)));
    let mut statement = connection.prepare(&sql).map_err(library_database_error)?;
    let rows = statement
        .query_map(params_from_iter(page_values), StoredGame::from_row)
        .map_err(library_database_error)?;
    let games = rows
        .map(|row| row.map(|game| game.summary()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(library_database_error)?;
    Ok(GamePage {
        total: u64::try_from(total).unwrap_or(0),
        offset,
        limit,
        games,
    })
}

/// Reads one stored game and returns its PGN plus every legal mainline board state.
pub fn game(path: &Path, id: i64) -> Result<GameDetail, LibraryError> {
    let connection = open_library_database(path)?;
    let sql = format!(
        "SELECT {QUERY_COLUMNS}, NULL AS position_ply
         FROM games g JOIN sources s ON s.id = g.source_id
         WHERE g.id = ?1"
    );
    let stored = connection
        .query_row(&sql, [id], StoredGame::from_row)
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => LibraryError::GameNotFound(id),
            error => library_database_error(error),
        })?;
    let maximum = usize::try_from(stored.pgn_bytes)
        .map_err(|_| LibraryError::InvalidGame(String::from("invalid stored PGN size")))?;
    if maximum > MAXIMUM_GAME_BYTES {
        return Err(LibraryError::InvalidGame(String::from(
            "stored PGN exceeds the 16 MiB game limit",
        )));
    }
    let pgn = zstd::bulk::decompress(&stored.pgn_zstd, maximum)
        .map_err(|error| LibraryError::InvalidGame(error.to_string()))?;
    if pgn.len() != maximum {
        return Err(LibraryError::InvalidGame(String::from(
            "stored PGN size does not match its metadata",
        )));
    }
    let (initial_board, moves) = game_boards(&pgn)?;
    Ok(GameDetail {
        summary: stored.summary(),
        pgn: String::from_utf8_lossy(&pgn).into_owned(),
        initial_board,
        moves,
    })
}

fn open_library_database(path: &Path) -> Result<Connection, LibraryError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(library_database_error)?;
    validate_database(&connection).map_err(|error| LibraryError::Database(error.to_string()))?;
    Ok(connection)
}

#[allow(clippy::needless_pass_by_value)]
fn library_database_error(error: rusqlite::Error) -> LibraryError {
    LibraryError::Database(error.to_string())
}

fn game_boards(pgn: &[u8]) -> Result<(Option<String>, Vec<GameMove>), LibraryError> {
    let mut fen = None;
    let mut variant = None;
    let mut position = Position::initial();
    let mut initial_board = None;
    let mut moves = Vec::new();
    let mut variation_depth = 0_u32;
    let mut board_supported = true;
    for event in Parser::with_options(pgn, ParserOptions::STRICT) {
        let event = event.map_err(|error| LibraryError::InvalidGame(error.to_string()))?;
        match event {
            Event::Tag(tag) if tag.name() == b"FEN" && fen.is_none() => {
                fen = Some(tag.value().into_owned());
            }
            Event::Tag(tag) if tag.name() == b"Variant" && variant.is_none() => {
                variant = Some(tag.value().into_owned());
            }
            Event::MovetextStart { .. } => {
                board_supported = variant
                    .as_deref()
                    .is_none_or(|value| value.eq_ignore_ascii_case(b"standard"));
                if board_supported {
                    position = fen
                        .as_deref()
                        .map_or_else(|| Ok(Position::initial()), Position::from_fen)
                        .map_err(|error| LibraryError::InvalidGame(error.to_string()))?;
                    initial_board = Some(encode_board(position));
                }
            }
            Event::San(token) if variation_depth == 0 && board_supported => {
                let chess_move = position
                    .play_san(token.as_bytes())
                    .map_err(|error| LibraryError::InvalidGame(error.to_string()))?;
                let ply = u32::try_from(moves.len() + 1)
                    .map_err(|_| LibraryError::InvalidGame(String::from("too many moves")))?;
                moves.push(GameMove {
                    ply,
                    san: String::from_utf8_lossy(token.as_bytes()).into_owned(),
                    from: chess_move.from().to_string(),
                    to: chess_move.to().to_string(),
                    board: encode_board(position),
                });
            }
            Event::VariationStart(_) => variation_depth += 1,
            Event::VariationEnd(_) => variation_depth = variation_depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok((initial_board, moves))
}

fn encode_board(position: Position) -> String {
    let mut board = String::with_capacity(64);
    for index in 0..64 {
        let square = Square::from_index(index).expect("board index is a square");
        let symbol = position.piece_at(square).map_or('.', |(color, piece)| {
            let symbol = match piece {
                Piece::Pawn => 'p',
                Piece::Knight => 'n',
                Piece::Bishop => 'b',
                Piece::Rook => 'r',
                Piece::Queen => 'q',
                Piece::King => 'k',
            };
            if color == Color::White {
                symbol.to_ascii_uppercase()
            } else {
                symbol
            }
        });
        board.push(symbol);
    }
    board
}

struct StoredGame {
    id: i64,
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
            id: row.get(0)?,
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

    fn summary(&self) -> GameSummary {
        GameSummary {
            id: self.id,
            source: self.source.clone(),
            source_game: self.source_game,
            event: self.event.as_deref().map(lossy).map(Cow::into_owned),
            site: self.site.as_deref().map(lossy).map(Cow::into_owned),
            date: self.date.as_deref().map(lossy).map(Cow::into_owned),
            white: self.white.as_deref().map(lossy).map(Cow::into_owned),
            black: self.black.as_deref().map(lossy).map(Cow::into_owned),
            white_elo: self.white_elo,
            black_elo: self.black_elo,
            result: decode_outcome(self.result).map(str::to_owned),
            mainline_plies: self.mainline_plies,
        }
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

    #[test]
    fn fingerprints_framed_games_deterministically() {
        let compact = fingerprint(&b"1. e4 *\n\n1. d4 *\n"[..], "first").unwrap();
        let same = fingerprint(&b"1. e4 *\n\n1. d4 *\n"[..], "second").unwrap();
        let changed = fingerprint(&b"1. e4 *\n\n1. c4 *\n"[..], "changed").unwrap();
        assert_eq!(compact.digest, same.digest);
        assert_ne!(compact.digest, changed.digest);
    }

    #[test]
    fn builds_database_from_plain_and_compressed_files() {
        let root = std::env::temp_dir().join(format!("gambit-build-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let plain = root.join("plain.pgn");
        let compressed = root.join("compressed.pgn.zst");
        let database = root.join("games.gambit");
        fs::write(&plain, b"1. e4 *\n").unwrap();
        fs::write(
            &compressed,
            zstd::stream::encode_all(&b"1. d4 *\n"[..], 1).unwrap(),
        )
        .unwrap();

        let summary = build_database_from_files([&plain, &compressed], &database).unwrap();

        assert_eq!(summary.sources, 2);
        assert_eq!(summary.games, 2);
        assert_eq!(info(&database, true).unwrap().games, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn library_api_pages_games_and_returns_mainline_boards() {
        let path =
            std::env::temp_dir().join(format!("gambit-library-api-{}.gambit", std::process::id()));
        let _ = fs::remove_file(&path);
        let pgn = b"[Event \"Friendly\"]\n[Site \"https://lichess.org/abcdefgh\"]\n[Date \"2026.09.05\"]\n[White \"DiegoGLozano\"]\n[Black \"Opponent\"]\n[WhiteElo \"1234\"]\n[BlackElo \"1200\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n";
        let mut builder = Builder::create(&path).unwrap();
        builder.add(&pgn[..], "friendly.pgn").unwrap();
        builder.finish().unwrap();

        let page = list_games(&path, Some("diegoglozano"), 0, 50).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.games.len(), 1);
        assert_eq!(page.games[0].white.as_deref(), Some("DiegoGLozano"));
        assert_eq!(page.games[0].result.as_deref(), Some("white_win"));

        let detail = game(&path, page.games[0].id).unwrap();
        assert_eq!(detail.moves.len(), 3);
        assert_eq!(detail.moves[0].san, "e4");
        assert_eq!(detail.moves[0].from, "e2");
        assert_eq!(detail.moves[0].to, "e4");
        assert_eq!(detail.moves[0].board.as_bytes()[28], b'P');
        assert!(detail.pgn.contains("Friendly"));
        assert_eq!(detail.initial_board.as_deref().map(str::len), Some(64));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn updater_migrates_schema_one_and_recovers_source_fingerprints() {
        let path = std::env::temp_dir().join(format!(
            "gambit-schema-one-migration-{}.gambit",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let connection = Connection::open(&path).unwrap();
        let schema_one = SCHEMA.replace(
            "    name        TEXT NOT NULL,\n    fingerprint BLOB\n",
            "    name        TEXT NOT NULL\n",
        );
        connection
            .execute_batch(&format!(
                "PRAGMA application_id = {APPLICATION_ID};
                 PRAGMA user_version = 1;
                 {schema_one}"
            ))
            .unwrap();
        connection
            .execute("INSERT INTO sources (id, name) VALUES (1, 'games.pgn')", [])
            .unwrap();
        let game = b"1. e4 *\n";
        let mut reader = GameReader::new(&game[..]);
        let stored_game = reader.read_game().unwrap().unwrap().to_vec();
        let compressed = zstd::bulk::compress(&stored_game, COMPRESSION_LEVEL).unwrap();
        connection
            .execute(
                "INSERT INTO games (
                    source_id, source_game, pgn_zstd, pgn_bytes, result, mainline_plies
                 ) VALUES (1, 1, ?1, ?2, 0, 1)",
                params![compressed, i64::try_from(stored_game.len()).unwrap()],
            )
            .unwrap();
        connection.close().unwrap();

        let mut output = Vec::new();
        let summary = query(
            &path,
            &QueryOptions::default(),
            QueryFormat::Count,
            &mut output,
        )
        .unwrap();
        assert_eq!(summary.matches, 1);
        let database_info = info(&path, false).unwrap();
        assert_eq!(database_info.schema_version, 1);
        assert_eq!(database_info.games, 1);
        assert_eq!(database_info.fingerprinted_sources, 0);

        let expected = fingerprint(&game[..], "games.pgn").unwrap();
        let mut updater = Updater::open(&path).unwrap();
        assert_eq!(
            updater.prepare("games.pgn", &expected).unwrap(),
            UpdateAction::Unchanged
        );
        let summary = updater.finish().unwrap();
        assert_eq!(summary.skipped_sources, 1);

        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        let digest: Vec<u8> = connection
            .query_row("SELECT fingerprint FROM sources", [], |row| row.get(0))
            .unwrap();
        assert_eq!(digest, expected.digest);
        connection.close().unwrap();
        fs::remove_file(path).unwrap();
    }
}
