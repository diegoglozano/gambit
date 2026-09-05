use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gambit::collection::{self, SyncRequest};
use gambit::index::{self, DatabaseInfo, GameDetail, GamePage};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct AppState {
    database: Mutex<Option<PathBuf>>,
}

#[derive(Serialize)]
struct DatabaseSession {
    path: String,
    info: DatabaseInfo,
    page: GamePage,
}

#[derive(Deserialize)]
struct SyncInput {
    username: String,
    since: Option<String>,
}

#[tauri::command]
async fn choose_database(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<DatabaseSession>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("Gambit database", &["gambit"])
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| format!("selected item is not a local file: {error}"))?;
    let session = load_session(&path, None)?;
    set_database(&state, path)?;
    Ok(Some(session))
}

#[tauri::command]
async fn sync_user(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SyncInput,
) -> Result<DatabaseSession, String> {
    let username = validated_username(&input.username)?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to locate application data: {error}"))?
        .join("collections")
        .join(&username);
    let request = SyncRequest::with_since(
        &username,
        root.join("lichess"),
        root.join(format!("{username}.gambit")),
        input.since.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    let database = request.database.clone();
    tauri::async_runtime::spawn_blocking(move || collection::sync_lichess(&request))
        .await
        .map_err(|error| format!("sync task failed: {error}"))?
        .map_err(|error| error.to_string())?;
    let session = load_session(&database, Some(&username))?;
    set_database(&state, database)?;
    Ok(session)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_games(
    state: State<'_, AppState>,
    player: Option<String>,
    offset: u64,
    limit: u32,
) -> Result<GamePage, String> {
    let database = database(&state)?;
    index::list_games(&database, player.as_deref(), offset, limit)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_game(state: State<'_, AppState>, id: i64) -> Result<GameDetail, String> {
    let database = database(&state)?;
    index::game(&database, id).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_game_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://lichess.org/") {
        return Err(String::from("only Lichess game links can be opened"));
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|error| error.to_string())
}

fn load_session(path: &Path, player: Option<&str>) -> Result<DatabaseSession, String> {
    let info = index::info(path, false).map_err(|error| error.to_string())?;
    let page = index::list_games(path, player, 0, 100).map_err(|error| error.to_string())?;
    Ok(DatabaseSession {
        path: path.to_string_lossy().into_owned(),
        info,
        page,
    })
}

fn database(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    state
        .database
        .lock()
        .map_err(|_| String::from("database state is unavailable"))?
        .clone()
        .ok_or_else(|| String::from("open or sync a database first"))
}

fn set_database(state: &State<'_, AppState>, path: PathBuf) -> Result<(), String> {
    *state
        .database
        .lock()
        .map_err(|_| String::from("database state is unavailable"))? = Some(path);
    Ok(())
}

fn validated_username(username: &str) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty()
        || username.len() > 30
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(String::from("enter a valid Lichess username"));
    }
    Ok(username.to_owned())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the Gambit desktop application.
///
/// # Panics
///
/// Panics when the native application runtime cannot be initialized.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            choose_database,
            sync_user,
            list_games,
            get_game,
            open_game_url
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Gambit Desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_lichess_usernames_before_creating_paths() {
        assert_eq!(validated_username(" Diego-G_1 ").unwrap(), "Diego-G_1");
        assert!(validated_username("../games").is_err());
        assert!(validated_username("").is_err());
    }

    #[test]
    fn rejects_non_lichess_links() {
        assert!(open_game_url(String::from("https://example.com/game")).is_err());
    }
}
