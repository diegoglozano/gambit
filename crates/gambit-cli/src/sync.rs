use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use gambit_pgn::{Event, FrameError, GameReader, Outcome, Parser, ParserOptions};
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = ".gambit-sync.json";
const GAMES_DIRECTORY: &str = "games";
const STATE_SCHEMA_VERSION: u8 = 1;
const CURSOR_OVERLAP_MILLISECONDS: i64 = 5 * 60 * 1_000;

#[derive(Clone, Debug)]
pub struct SyncPlan {
    destination: PathBuf,
    username: String,
    needs_initialization: bool,
    pub since_timestamp: Option<i64>,
    pub until_timestamp: i64,
    pub unfinished_game_ids: Vec<String>,
    pub initial_since: Option<u32>,
}

impl SyncPlan {
    pub const fn is_initial(&self) -> bool {
        self.since_timestamp.is_none()
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameStatus {
    pub game_id: String,
    pub unfinished: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IngestSummary {
    pub received: u64,
    pub created: u64,
    pub updated: u64,
    pub unchanged: u64,
    pub statuses: Vec<GameStatus>,
}

impl IngestSummary {
    pub fn add(&mut self, mut other: Self) {
        self.received += other.received;
        self.created += other.created;
        self.updated += other.updated;
        self.unchanged += other.unchanged;
        self.statuses.append(&mut other.statuses);
    }
}

#[derive(Debug)]
pub enum SyncError {
    Io {
        action: String,
        error: io::Error,
    },
    State(String),
    Frame(FrameError),
    Parse {
        game: u64,
        error: gambit_pgn::ParseError,
    },
    MissingGameId {
        game: u64,
    },
    UnexpectedGameId {
        expected: String,
        actual: String,
    },
    UnexpectedGameCount {
        expected: usize,
        actual: u64,
    },
}

impl fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { action, error } => write!(formatter, "{action}: {error}"),
            Self::State(message) => formatter.write_str(message),
            Self::Frame(error) => error.fmt(formatter),
            Self::Parse { game, error } => write!(formatter, "game {game}: {error}"),
            Self::MissingGameId { game } => {
                write!(
                    formatter,
                    "game {game} has no recognizable Lichess Site URL"
                )
            }
            Self::UnexpectedGameId { expected, actual } => write!(
                formatter,
                "Lichess returned game {actual:?} while refreshing {expected:?}"
            ),
            Self::UnexpectedGameCount { expected, actual } => write!(
                formatter,
                "expected {expected} game from Lichess, received {actual}"
            ),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct SyncState {
    schema_version: u8,
    source: String,
    username: String,
    initial_since: Option<u32>,
    cursor_milliseconds: Option<i64>,
    unfinished_game_ids: Vec<String>,
}

pub fn prepare(
    destination: &Path,
    username: &str,
    now_milliseconds: i64,
    requested_since: Option<u32>,
) -> Result<SyncPlan, SyncError> {
    let state_path = destination.join(STATE_FILE);
    let state = if state_path.exists() {
        read_state(&state_path)?
    } else {
        if destination.exists() {
            let mut entries = fs::read_dir(destination).map_err(|error| SyncError::Io {
                action: format!("failed to inspect {}", destination.display()),
                error,
            })?;
            if entries
                .next()
                .transpose()
                .map_err(|error| SyncError::Io {
                    action: format!("failed to inspect {}", destination.display()),
                    error,
                })?
                .is_some()
            {
                return Err(SyncError::State(format!(
                    "{} is not an empty directory or a Gambit sync destination",
                    destination.display()
                )));
            }
        }
        SyncState {
            schema_version: STATE_SCHEMA_VERSION,
            source: String::from("lichess"),
            username: username.to_owned(),
            initial_since: requested_since,
            cursor_milliseconds: None,
            unfinished_game_ids: Vec::new(),
        }
    };

    validate_state(&state, username, destination)?;
    if state.cursor_milliseconds.is_none()
        && requested_since.is_some()
        && requested_since != state.initial_since
    {
        return Err(SyncError::State(format!(
            "{} was initialized with a different --since boundary",
            destination.display()
        )));
    }
    let needs_initialization = !state_path.exists();
    let since_timestamp = state
        .cursor_milliseconds
        .map(|cursor| cursor.saturating_sub(CURSOR_OVERLAP_MILLISECONDS))
        .map(|since| since.min(now_milliseconds));
    let mut unfinished_game_ids = state.unfinished_game_ids;
    unfinished_game_ids.sort_unstable();
    unfinished_game_ids.dedup();
    Ok(SyncPlan {
        destination: destination.to_path_buf(),
        username: state.username,
        needs_initialization,
        since_timestamp,
        until_timestamp: now_milliseconds,
        unfinished_game_ids,
        initial_since: state.initial_since,
    })
}

pub fn start(plan: &SyncPlan) -> Result<(), SyncError> {
    fs::create_dir_all(&plan.destination).map_err(|error| SyncError::Io {
        action: format!("failed to create {}", plan.destination.display()),
        error,
    })?;
    if plan.needs_initialization {
        let state = SyncState {
            schema_version: STATE_SCHEMA_VERSION,
            source: String::from("lichess"),
            username: plan.username.clone(),
            initial_since: plan.initial_since,
            cursor_milliseconds: None,
            unfinished_game_ids: Vec::new(),
        };
        write_state(&plan.destination.join(STATE_FILE), &state)?;
    }
    fs::create_dir_all(plan.destination.join(GAMES_DIRECTORY)).map_err(|error| SyncError::Io {
        action: format!(
            "failed to create {}/{}",
            plan.destination.display(),
            GAMES_DIRECTORY
        ),
        error,
    })?;
    Ok(())
}

pub fn ingest<R: Read>(
    reader: R,
    plan: &SyncPlan,
    expected_game_id: Option<&str>,
) -> Result<IngestSummary, SyncError> {
    let mut reader = GameReader::new(reader);
    let mut summary = IngestSummary::default();
    while let Some(game) = reader.read_game().map_err(SyncError::Frame)? {
        summary.received += 1;
        let (game_id, unfinished) = inspect_game(game, summary.received)?;
        if let Some(expected) = expected_game_id {
            if game_id != expected {
                return Err(SyncError::UnexpectedGameId {
                    expected: expected.to_owned(),
                    actual: game_id,
                });
            }
        }
        match store_game(plan.destination(), &game_id, game)? {
            StoreResult::Created => summary.created += 1,
            StoreResult::Updated => summary.updated += 1,
            StoreResult::Unchanged => summary.unchanged += 1,
        }
        summary.statuses.push(GameStatus {
            game_id,
            unfinished,
        });
    }
    if expected_game_id.is_some() && summary.received != 1 {
        return Err(SyncError::UnexpectedGameCount {
            expected: 1,
            actual: summary.received,
        });
    }
    Ok(summary)
}

pub fn finish(
    plan: &SyncPlan,
    statuses: impl IntoIterator<Item = GameStatus>,
) -> Result<usize, SyncError> {
    let mut latest = BTreeMap::new();
    for status in statuses {
        latest.insert(status.game_id, status.unfinished);
    }
    let unfinished_game_ids = latest
        .into_iter()
        .filter_map(|(game_id, unfinished)| unfinished.then_some(game_id))
        .collect();
    let state = SyncState {
        schema_version: STATE_SCHEMA_VERSION,
        source: String::from("lichess"),
        username: plan.username.clone(),
        initial_since: plan.initial_since,
        cursor_milliseconds: Some(plan.until_timestamp),
        unfinished_game_ids,
    };
    let unfinished = state.unfinished_game_ids.len();
    write_state(&plan.destination.join(STATE_FILE), &state)?;
    Ok(unfinished)
}

fn validate_state(state: &SyncState, username: &str, destination: &Path) -> Result<(), SyncError> {
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err(SyncError::State(format!(
            "{} uses unsupported sync schema version {}",
            destination.display(),
            state.schema_version
        )));
    }
    if state.source != "lichess" {
        return Err(SyncError::State(format!(
            "{} is not a Lichess sync destination",
            destination.display()
        )));
    }
    if !state.username.eq_ignore_ascii_case(username) {
        return Err(SyncError::State(format!(
            "{} belongs to Lichess user {:?}, not {username:?}",
            destination.display(),
            state.username
        )));
    }
    if let Some(game_id) = state
        .unfinished_game_ids
        .iter()
        .find(|game_id| !is_game_id(game_id.as_bytes()))
    {
        return Err(SyncError::State(format!(
            "{} contains invalid unfinished game ID {game_id:?}",
            destination.display()
        )));
    }
    Ok(())
}

fn read_state(path: &Path) -> Result<SyncState, SyncError> {
    let file = File::open(path).map_err(|error| SyncError::Io {
        action: format!("failed to open {}", path.display()),
        error,
    })?;
    serde_json::from_reader(file)
        .map_err(|error| SyncError::State(format!("failed to parse {}: {error}", path.display())))
}

fn write_state(path: &Path, state: &SyncState) -> Result<(), SyncError> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|error| SyncError::Io {
                action: format!("failed to create {}", temporary.display()),
                error,
            })?;
        serde_json::to_writer_pretty(&mut file, state)
            .map_err(|error| SyncError::State(format!("failed to encode sync state: {error}")))?;
        file.write_all(b"\n").map_err(|error| SyncError::Io {
            action: format!("failed to write {}", temporary.display()),
            error,
        })?;
        file.sync_all().map_err(|error| SyncError::Io {
            action: format!("failed to flush {}", temporary.display()),
            error,
        })?;
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn inspect_game(game: &[u8], number: u64) -> Result<(String, bool), SyncError> {
    let mut game_id = None;
    let mut outcome = None;
    let mut variation_depth = 0_u32;
    for event in Parser::with_options(game, ParserOptions::STRICT) {
        match event.map_err(|error| SyncError::Parse {
            game: number,
            error,
        })? {
            Event::Tag(tag) if tag.name() == b"Site" && game_id.is_none() => {
                game_id = game_id_from_site(tag.value().as_ref());
            }
            Event::VariationStart(_) => variation_depth += 1,
            Event::VariationEnd(_) => variation_depth -= 1,
            Event::Outcome { outcome: value, .. } if variation_depth == 0 => outcome = Some(value),
            _ => {}
        }
    }
    let game_id = game_id.ok_or(SyncError::MissingGameId { game: number })?;
    Ok((game_id, outcome == Some(Outcome::Unknown)))
}

fn game_id_from_site(site: &[u8]) -> Option<String> {
    let remainder = site.strip_prefix(b"https://lichess.org/")?;
    let game_id = remainder.get(..8)?;
    is_game_id(game_id).then(|| String::from_utf8_lossy(game_id).into_owned())
}

fn is_game_id(game_id: &[u8]) -> bool {
    game_id.len() == 8 && game_id.iter().all(u8::is_ascii_alphanumeric)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoreResult {
    Created,
    Updated,
    Unchanged,
}

fn store_game(destination: &Path, game_id: &str, game: &[u8]) -> Result<StoreResult, SyncError> {
    let path = destination
        .join(GAMES_DIRECTORY)
        .join(game_filename(game_id));
    let mut contents = Vec::with_capacity(game.len() + 1);
    contents.extend_from_slice(game);
    if !contents.ends_with(b"\n") {
        contents.push(b'\n');
    }
    let result = match fs::read(&path) {
        Ok(existing) if existing == contents => return Ok(StoreResult::Unchanged),
        Ok(_) => StoreResult::Updated,
        Err(error) if error.kind() == io::ErrorKind::NotFound => StoreResult::Created,
        Err(error) => {
            return Err(SyncError::Io {
                action: format!("failed to read {}", path.display()),
                error,
            });
        }
    };
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, contents).map_err(|error| SyncError::Io {
        action: format!("failed to write {}", temporary.display()),
        error,
    })?;
    if let Err(error) = replace_file(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(result)
}

fn game_filename(game_id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut filename = String::with_capacity(game_id.len() * 2 + 4);
    for byte in game_id.bytes() {
        filename.push(char::from(HEX[usize::from(byte >> 4)]));
        filename.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    filename.push_str(".pgn");
    filename
}

fn replace_file(temporary: &Path, destination: &Path) -> Result<(), SyncError> {
    match fs::rename(temporary, destination) {
        Ok(()) => Ok(()),
        Err(first_error) if destination.exists() => {
            fs::remove_file(destination).map_err(|error| SyncError::Io {
                action: format!("failed to replace {}", destination.display()),
                error,
            })?;
            fs::rename(temporary, destination).map_err(|error| SyncError::Io {
                action: format!(
                    "failed to replace {} after rename failed: {first_error}",
                    destination.display()
                ),
                error,
            })
        }
        Err(error) => Err(SyncError::Io {
            action: format!(
                "failed to move {} to {}",
                temporary.display(),
                destination.display()
            ),
            error,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "gambit-sync-test-{}-{sequence}",
                std::process::id()
            ));
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn stores_games_by_id_and_resumes_with_an_overlap() {
        let directory = TestDirectory::new();
        let first = prepare(&directory.0, "DiegoGLozano", 2_000_000, Some(20_260_101)).unwrap();
        assert!(first.is_initial());
        assert!(!directory.0.exists());
        start(&first).unwrap();
        let retry = prepare(&directory.0, "diegoglozano", 2_500_000, None).unwrap();
        assert_eq!(retry.initial_since, Some(20_260_101));
        let changed =
            prepare(&directory.0, "diegoglozano", 2_500_000, Some(20_260_102)).unwrap_err();
        assert!(changed.to_string().contains("different --since boundary"));
        let pgn =
            b"[Site \"https://lichess.org/AbCd1234\"]\n[White \"A\"]\n[Black \"B\"]\n\n1. e4 *\n";
        let ingested = ingest(&pgn[..], &first, None).unwrap();
        assert_eq!(ingested.received, 1);
        assert_eq!(ingested.created, 1);
        assert!(ingested.statuses[0].unfinished);
        finish(&first, ingested.statuses).unwrap();

        let second = prepare(&directory.0, "diegoglozano", 3_000_000, None).unwrap();
        assert_eq!(second.since_timestamp, Some(1_700_000));
        assert_eq!(second.unfinished_game_ids, ["AbCd1234"]);
        let ingested = ingest(&pgn[..], &second, Some("AbCd1234")).unwrap();
        assert_eq!(ingested.unchanged, 1);
        assert_eq!(
            fs::read_to_string(directory.0.join("games/4162436431323334.pgn")).unwrap(),
            String::from_utf8_lossy(pgn)
        );

        let finished =
            b"[Site \"https://lichess.org/AbCd1234\"]\n[White \"A\"]\n[Black \"B\"]\n\n1. e4 1-0\n";
        let refreshed = ingest(&finished[..], &second, Some("AbCd1234")).unwrap();
        assert_eq!(refreshed.updated, 1);
        assert!(!refreshed.statuses[0].unfinished);
        finish(&second, refreshed.statuses).unwrap();
        let third = prepare(&directory.0, "diegoglozano", 4_000_000, None).unwrap();
        assert!(third.unfinished_game_ids.is_empty());
    }

    #[test]
    fn refuses_to_claim_a_nonempty_unmanaged_directory() {
        let directory = TestDirectory::new();
        fs::create_dir_all(&directory.0).unwrap();
        fs::write(directory.0.join("keep.txt"), b"mine").unwrap();
        let error = prepare(&directory.0, "diegoglozano", 1, None).unwrap_err();
        assert!(error.to_string().contains("not an empty directory"));
        assert_eq!(fs::read(directory.0.join("keep.txt")).unwrap(), b"mine");
    }

    #[test]
    fn rejects_games_without_a_lichess_id() {
        let directory = TestDirectory::new();
        let plan = prepare(&directory.0, "diegoglozano", 1, None).unwrap();
        start(&plan).unwrap();
        let error = ingest(&b"[Site \"elsewhere\"]\n\n1. e4 *\n"[..], &plan, None).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no recognizable Lichess Site URL")
        );
    }

    #[test]
    fn filenames_preserve_case_sensitive_game_ids_portably() {
        assert_eq!(game_filename("AbCd1234"), "4162436431323334.pgn");
        assert_eq!(game_filename("aBcD1234"), "6142634431323334.pgn");
        assert_ne!(game_filename("AbCd1234"), game_filename("aBcD1234"));
    }
}
