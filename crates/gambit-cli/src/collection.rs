//! High-level collection operations shared by the CLI and desktop app.

use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::index::{self, IndexSummary, UpdateAction};
use crate::lichess::{self, UserGamesRequest};
use crate::query::{QueryOptions, parse_date};
use crate::sync;

#[derive(Clone, Debug)]
pub struct SyncRequest {
    pub username: String,
    pub destination: PathBuf,
    pub database: PathBuf,
    pub since: Option<u32>,
    pub token: Option<String>,
}

impl SyncRequest {
    pub fn with_since(
        username: impl Into<String>,
        destination: impl Into<PathBuf>,
        database: impl Into<PathBuf>,
        since: Option<&str>,
    ) -> Result<Self, CollectionError> {
        let since = since
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                parse_date(value.trim()).ok_or_else(|| {
                    CollectionError::InvalidRequest(String::from(
                        "since must be a complete date in YYYY-MM-DD form",
                    ))
                })
            })
            .transpose()?;
        Ok(Self {
            username: username.into(),
            destination: destination.into(),
            database: database.into(),
            since,
            token: None,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncReport {
    pub username: String,
    pub destination: String,
    pub database: String,
    pub received: u64,
    pub created: u64,
    pub updated: u64,
    pub unchanged: u64,
    pub refreshed_unfinished: usize,
    pub unfinished: usize,
    pub cursor_milliseconds: i64,
    pub index_mode: &'static str,
    pub index: IndexSummary,
}

#[derive(Debug)]
pub enum CollectionError {
    InvalidRequest(String),
    Clock(String),
    Lichess(lichess::LichessError),
    Sync(sync::SyncError),
    Index(index::IndexError),
}

impl fmt::Display for CollectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(error) | Self::Clock(error) => formatter.write_str(error),
            Self::Lichess(error) => write!(formatter, "Lichess sync failed: {error}"),
            Self::Sync(error) => write!(formatter, "collection sync failed: {error}"),
            Self::Index(error) => write!(formatter, "database update failed: {error}"),
        }
    }
}

impl std::error::Error for CollectionError {}

impl From<sync::SyncError> for CollectionError {
    fn from(error: sync::SyncError) -> Self {
        Self::Sync(error)
    }
}

impl From<index::IndexError> for CollectionError {
    fn from(error: index::IndexError) -> Self {
        Self::Index(error)
    }
}

/// Synchronizes one Lichess collection and transactionally maintains its database.
pub fn sync_lichess(request: &SyncRequest) -> Result<SyncReport, CollectionError> {
    if request.username.trim().is_empty() {
        return Err(CollectionError::InvalidRequest(String::from(
            "Lichess username cannot be empty",
        )));
    }
    let now_milliseconds = current_time_milliseconds()?;
    let plan = sync::prepare(
        &request.destination,
        request.username.trim(),
        now_milliseconds,
        request.since,
    )?;
    if request.since.is_some() && !plan.is_initial() {
        return Err(CollectionError::InvalidRequest(String::from(
            "since can only initialize a new sync destination",
        )));
    }

    let options = QueryOptions {
        since: plan.is_initial().then_some(plan.initial_since).flatten(),
        ..QueryOptions::default()
    };
    let api_request = UserGamesRequest {
        username: request.username.trim(),
        maximum_games: None,
        options: &options,
        since_timestamp: plan.since_timestamp,
        until_timestamp: Some(plan.until_timestamp),
        include_ongoing: true,
        oldest_first: true,
    };
    let mut response = lichess::user_games(&api_request, request.token.as_deref())
        .map_err(CollectionError::Lichess)?;
    sync::start(&plan)?;
    let mut summary = sync::ingest(response.body_mut().as_reader(), &plan, None)?;

    let refreshed_unfinished = plan.unfinished_game_ids.len();
    for game_id in &plan.unfinished_game_ids {
        let mut response = match lichess::game(game_id, request.token.as_deref()) {
            Ok(response) => response,
            Err(lichess::LichessError::GameNotFound(_)) => {
                summary.statuses.push(sync::GameStatus {
                    game_id: game_id.clone(),
                    unfinished: false,
                });
                continue;
            }
            Err(error) => return Err(CollectionError::Lichess(error)),
        };
        summary.add(sync::ingest(
            response.body_mut().as_reader(),
            &plan,
            Some(game_id),
        )?);
    }

    let statuses = std::mem::take(&mut summary.statuses);
    let unfinished = sync::finish(&plan, statuses)?;
    let (index_mode, index) = maintain_database(&request.destination, &request.database)?;
    Ok(SyncReport {
        username: request.username.trim().to_owned(),
        destination: request.destination.to_string_lossy().into_owned(),
        database: request.database.to_string_lossy().into_owned(),
        received: summary.received,
        created: summary.created,
        updated: summary.updated,
        unchanged: summary.unchanged,
        refreshed_unfinished,
        unfinished,
        cursor_milliseconds: plan.until_timestamp,
        index_mode,
        index,
    })
}

/// Builds or incrementally updates a database from a managed sync destination.
pub fn maintain_database(
    source: &Path,
    database: &Path,
) -> Result<(&'static str, IndexSummary), CollectionError> {
    let games_directory = source.join("games");
    let mut paths = fs::read_dir(&games_directory)
        .map_err(|error| index::IndexError::Io {
            context: format!("failed to inspect {}", games_directory.display()),
            error,
        })?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| index::IndexError::Io {
                    context: format!("failed to inspect {}", games_directory.display()),
                    error,
                })
        })
        .filter_map(|entry| match entry {
            Ok(path) if is_pgn(&path) => Some(Ok(path)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort_unstable();

    if database.exists() {
        let mut updater = index::Updater::open(database)?;
        for path in &paths {
            let source_name = path.to_string_lossy().into_owned();
            let fingerprint = index::fingerprint(open(path)?, &source_name)?;
            if updater.prepare(&source_name, &fingerprint)? == UpdateAction::Write {
                updater.add(open(path)?, &source_name, &fingerprint)?;
            }
        }
        Ok(("update", updater.finish()?))
    } else {
        let mut builder = index::Builder::create(database)?;
        for path in &paths {
            let source_name = path.to_string_lossy().into_owned();
            builder.add(open(path)?, &source_name)?;
        }
        Ok(("build", builder.finish()?))
    }
}

fn open(path: &Path) -> Result<File, index::IndexError> {
    File::open(path).map_err(|error| index::IndexError::Io {
        context: format!("failed to open {}", path.display()),
        error,
    })
}

fn is_pgn(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pgn"))
}

fn current_time_milliseconds() -> Result<i64, CollectionError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CollectionError::Clock(error.to_string()))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| CollectionError::Clock(String::from("system time is out of range")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_database_builds_then_updates_incrementally() {
        let root =
            std::env::temp_dir().join(format!("gambit-collection-service-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let games = root.join("sync/games");
        let database = root.join("library.gambit");
        fs::create_dir_all(&games).unwrap();

        let (mode, summary) = maintain_database(&root.join("sync"), &database).unwrap();
        assert_eq!(mode, "build");
        assert_eq!(summary.games, 0);

        fs::write(
            games.join("game.pgn"),
            b"[White \"Diego\"]\n[Black \"Other\"]\n[Result \"*\"]\n\n1. e4 *\n",
        )
        .unwrap();
        let (mode, summary) = maintain_database(&root.join("sync"), &database).unwrap();
        assert_eq!(mode, "update");
        assert_eq!(summary.games, 1);
        assert_eq!(summary.skipped_sources, 0);

        let (_, summary) = maintain_database(&root.join("sync"), &database).unwrap();
        assert_eq!(summary.games, 0);
        assert_eq!(summary.skipped_sources, 1);

        fs::remove_dir_all(root).unwrap();
    }
}
