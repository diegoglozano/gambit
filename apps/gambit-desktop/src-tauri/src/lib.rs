use std::fs;
use std::io::ErrorKind;
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
    managed_user: Option<String>,
    info: DatabaseInfo,
    page: GamePage,
}

#[derive(Deserialize, Serialize)]
struct SavedSession {
    version: u8,
    database: PathBuf,
    managed_user: Option<String>,
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
    remember_session(&app, &path, None)?;
    set_database(&state, path)?;
    Ok(Some(session))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn restore_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<DatabaseSession>, String> {
    let Some(saved) = read_saved_session(&app)? else {
        return Ok(None);
    };
    let session = load_session(&saved.database, saved.managed_user.as_deref())?;
    set_database(&state, saved.database)?;
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
    remember_session(&app, &database, Some(&username))?;
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
        managed_user: player.map(str::to_owned),
        info,
        page,
    })
}

fn remember_session(
    app: &AppHandle,
    database: &Path,
    managed_user: Option<&str>,
) -> Result<(), String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to locate application data: {error}"))?;
    write_saved_session(&app_data, database, managed_user)
}

fn read_saved_session(app: &AppHandle) -> Result<Option<SavedSession>, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to locate application data: {error}"))?;
    let path = app_data.join("session.json");
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    let saved: SavedSession = serde_json::from_slice(&contents)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if saved.version != 1 {
        return Err(format!(
            "{} uses an unsupported session version",
            path.display()
        ));
    }
    Ok(Some(saved))
}

fn write_saved_session(
    app_data: &Path,
    database: &Path,
    managed_user: Option<&str>,
) -> Result<(), String> {
    fs::create_dir_all(app_data)
        .map_err(|error| format!("failed to create {}: {error}", app_data.display()))?;
    let path = app_data.join("session.json");
    let saved = SavedSession {
        version: 1,
        database: database.to_owned(),
        managed_user: managed_user.map(str::to_owned),
    };
    let contents = serde_json::to_vec_pretty(&saved)
        .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
    fs::write(&path, contents)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
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
            restore_session,
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

    #[test]
    fn saved_session_round_trips_managed_library() {
        let root =
            std::env::temp_dir().join(format!("gambit-desktop-session-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let database = root.join("collections/diego/diego.gambit");

        write_saved_session(&root, &database, Some("diego")).unwrap();
        let contents = fs::read(root.join("session.json")).unwrap();
        let saved: SavedSession = serde_json::from_slice(&contents).unwrap();

        assert_eq!(saved.version, 1);
        assert_eq!(saved.database, database);
        assert_eq!(saved.managed_user.as_deref(), Some("diego"));
        fs::remove_dir_all(root).unwrap();
    }
}
