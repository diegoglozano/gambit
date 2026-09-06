use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gambit::collection::{self, SyncRequest};
use gambit::index::{self, DatabaseInfo, GameDetail, GamePage};
use gambit::query::{self, PlayerColor, QueryFormat, QueryOptions, ResultFilter};
use gambit_chess::Position;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_updater::UpdaterExt;

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
    #[serde(default)]
    libraries: Vec<SavedLibrary>,
}

#[derive(Clone, Deserialize, Serialize)]
struct SavedLibrary {
    database: PathBuf,
    managed_user: Option<String>,
}

#[derive(Serialize)]
struct LibraryEntry {
    path: String,
    managed_user: Option<String>,
    exists: bool,
    active: bool,
}

#[derive(Deserialize)]
struct SyncInput {
    username: String,
    since: Option<String>,
    token: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct GameFilters {
    player: Option<String>,
    opponent: Option<String>,
    color: Option<String>,
    result: Option<String>,
    since: Option<String>,
    until: Option<String>,
    minimum_rating: Option<String>,
    maximum_rating: Option<String>,
    position: Option<String>,
}

#[derive(Serialize)]
struct ExportReport {
    path: String,
    games: u64,
    bytes: u64,
}

#[derive(Serialize)]
struct AvailableUpdate {
    current_version: String,
    version: String,
    notes: Option<String>,
    published_at: Option<String>,
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
    let managed_user = known_managed_user(&app, &path)?;
    let session = load_session(&path, managed_user.as_deref())?;
    remember_session(&app, &path, managed_user.as_deref())?;
    set_database(&state, path)?;
    Ok(Some(session))
}

#[tauri::command]
async fn open_database(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<DatabaseSession, String> {
    let path = PathBuf::from(path);
    let managed_user = known_managed_user(&app, &path)?;
    let session = load_session(&path, managed_user.as_deref())?;
    remember_session(&app, &path, managed_user.as_deref())?;
    set_database(&state, path)?;
    Ok(session)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_databases(app: AppHandle) -> Result<Vec<LibraryEntry>, String> {
    let saved = read_saved_session(&app)?;
    Ok(saved.map_or_else(Vec::new, |saved| {
        let active = saved.database;
        saved
            .libraries
            .into_iter()
            .map(|library| LibraryEntry {
                exists: library.database.is_file(),
                active: active == library.database,
                path: library.database.to_string_lossy().into_owned(),
                managed_user: library.managed_user,
            })
            .collect()
    }))
}

#[tauri::command]
async fn import_pgn(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<DatabaseSession>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("PGN game collections", &["pgn", "zst"])
        .blocking_pick_files();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let inputs = selected
        .into_iter()
        .map(|selected| {
            selected
                .into_path()
                .map_err(|error| format!("selected item is not a local file: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if inputs.is_empty() || inputs.iter().any(|path| !is_pgn_path(path)) {
        return Err(String::from("choose only .pgn or .pgn.zst files"));
    }
    let first = &inputs[0];
    let suggested_name = format!(
        "{}.gambit",
        first
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("games")
    );
    let mut save_dialog = app
        .dialog()
        .file()
        .add_filter("Gambit database", &["gambit"])
        .set_file_name(suggested_name);
    if let Some(parent) = first.parent() {
        save_dialog = save_dialog.set_directory(parent);
    }
    let Some(selected) = save_dialog.blocking_save_file() else {
        return Ok(None);
    };
    let mut database = selected
        .into_path()
        .map_err(|error| format!("selected destination is not a local file: {error}"))?;
    if !database
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gambit"))
    {
        database.set_extension("gambit");
    }

    let build_inputs = inputs;
    let build_database = database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        index::build_database_from_files(build_inputs, &build_database)
    })
    .await
    .map_err(|error| format!("index task failed: {error}"))?
    .map_err(|error| error.to_string())?;
    let session = load_session(&database, None)?;
    remember_session(&app, &database, None)?;
    set_database(&state, database)?;
    Ok(Some(session))
}

#[tauri::command]
async fn update_database(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<DatabaseSession>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("PGN game collections", &["pgn", "zst"])
        .blocking_pick_files();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let inputs = selected
        .into_iter()
        .map(|selected| {
            selected
                .into_path()
                .map_err(|error| format!("selected item is not a local file: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if inputs.is_empty() || inputs.iter().any(|path| !is_pgn_path(path)) {
        return Err(String::from("choose only .pgn or .pgn.zst files"));
    }
    let database = database(&state)?;
    let managed_user = known_managed_user(&app, &database)?;
    let update_database = database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        index::update_database_from_files(inputs, &update_database)
    })
    .await
    .map_err(|error| format!("index update task failed: {error}"))?
    .map_err(|error| error.to_string())?;
    let session = load_session(&database, managed_user.as_deref())?;
    remember_session(&app, &database, managed_user.as_deref())?;
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
    let mut request = SyncRequest::with_since(
        &username,
        root.join("lichess"),
        root.join(format!("{username}.gambit")),
        input.since.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    request.token = normalized_token(input.token);
    let database = request.database.clone();
    let progress_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        collection::sync_lichess_with_progress(&request, |progress| {
            let _ = progress_app.emit("sync-progress", progress);
        })
    })
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
    filters: GameFilters,
    offset: u64,
    limit: u32,
) -> Result<GamePage, String> {
    let database = database(&state)?;
    let options = query_options(&filters)?;
    index::search_games(&database, &options, offset, limit).map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn check_database(state: State<'_, AppState>) -> Result<DatabaseInfo, String> {
    let database = database(&state)?;
    index::info(&database, true).map_err(|error| error.to_string())
}

#[tauri::command]
async fn export_games(
    app: AppHandle,
    state: State<'_, AppState>,
    filters: GameFilters,
) -> Result<Option<ExportReport>, String> {
    let database = database(&state)?;
    let options = query_options(&filters)?;
    let suggested_name = database
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map_or_else(
            || String::from("games-export.pgn"),
            |stem| format!("{stem}-export.pgn"),
        );
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("PGN game collection", &["pgn"])
        .set_file_name(suggested_name);
    if let Some(parent) = database.parent() {
        dialog = dialog.set_directory(parent);
    }
    let Some(selected) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let mut destination = selected
        .into_path()
        .map_err(|error| format!("selected destination is not a local file: {error}"))?;
    if !extension_is(&destination, "pgn") {
        destination.set_extension("pgn");
    }
    let temporary = temporary_sibling(&destination);
    let export_database = database.clone();
    let export_temporary = temporary.clone();
    let summary = tauri::async_runtime::spawn_blocking(move || {
        let mut file = File::create(&export_temporary)
            .map_err(|error| format!("failed to create {}: {error}", export_temporary.display()))?;
        let summary = index::query(&export_database, &options, QueryFormat::Pgn, &mut file)
            .map_err(|failure| failure.error.to_string())?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("failed to flush {}: {error}", export_temporary.display()))?;
        Ok::<_, String>(summary)
    })
    .await
    .map_err(|error| format!("export task failed: {error}"))??;
    if let Err(error) = replace_file(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(Some(ExportReport {
        path: destination.to_string_lossy().into_owned(),
        games: summary.matches,
        bytes: summary.bytes,
    }))
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

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
async fn check_for_update(app: AppHandle) -> Result<Option<AvailableUpdate>, String> {
    let update = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?;
    Ok(update.map(|update| AvailableUpdate {
        current_version: update.current_version,
        version: update.version,
        notes: update.body,
        published_at: update.date.map(|date| date.to_string()),
    }))
}

#[tauri::command]
async fn install_update(app: AppHandle, expected_version: String) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("the update is no longer available"))?;
    if update.version != expected_version {
        return Err(format!(
            "Gambit {expected_version} was replaced by {}; check again before updating",
            update.version
        ));
    }
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn restart_app(app: AppHandle) {
    app.restart();
}

fn load_session(path: &Path, player: Option<&str>) -> Result<DatabaseSession, String> {
    let info = index::info(path, false).map_err(|error| error.to_string())?;
    let options = QueryOptions {
        player: player.map(str::to_owned),
        ..QueryOptions::default()
    };
    let page = index::search_games(path, &options, 0, 100).map_err(|error| error.to_string())?;
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

fn known_managed_user(app: &AppHandle, database: &Path) -> Result<Option<String>, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to locate application data: {error}"))?;
    Ok(read_saved_session_from(&app_data)?
        .and_then(|saved| {
            saved
                .libraries
                .into_iter()
                .find(|library| library.database == database)
        })
        .and_then(|library| library.managed_user))
}

fn read_saved_session(app: &AppHandle) -> Result<Option<SavedSession>, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to locate application data: {error}"))?;
    read_saved_session_from(&app_data)
}

fn read_saved_session_from(app_data: &Path) -> Result<Option<SavedSession>, String> {
    let path = app_data.join("session.json");
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    let mut saved: SavedSession = serde_json::from_slice(&contents)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if saved.version != 1 {
        return Err(format!(
            "{} uses an unsupported session version",
            path.display()
        ));
    }
    if !saved
        .libraries
        .iter()
        .any(|library| library.database == saved.database)
    {
        saved.libraries.insert(
            0,
            SavedLibrary {
                database: saved.database.clone(),
                managed_user: saved.managed_user.clone(),
            },
        );
    }
    Ok(Some(saved))
}

fn write_saved_session(
    app_data: &Path,
    database: &Path,
    managed_user: Option<&str>,
) -> Result<(), String> {
    const MAXIMUM_RECENT_LIBRARIES: usize = 12;
    fs::create_dir_all(app_data)
        .map_err(|error| format!("failed to create {}: {error}", app_data.display()))?;
    let path = app_data.join("session.json");
    let mut libraries = read_saved_session_from(app_data)?
        .map(|saved| saved.libraries)
        .unwrap_or_default();
    libraries.retain(|library| library.database != database);
    libraries.insert(
        0,
        SavedLibrary {
            database: database.to_owned(),
            managed_user: managed_user.map(str::to_owned),
        },
    );
    libraries.truncate(MAXIMUM_RECENT_LIBRARIES);
    let saved = SavedSession {
        version: 1,
        database: database.to_owned(),
        managed_user: managed_user.map(str::to_owned),
        libraries,
    };
    let contents = serde_json::to_vec_pretty(&saved)
        .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
    let temporary = temporary_sibling(&path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|error| format!("failed to create {}: {error}", temporary.display()))?;
        file.write_all(&contents)
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
        replace_file(&temporary, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn query_options(filters: &GameFilters) -> Result<QueryOptions, String> {
    let player = filter_text(filters.player.as_deref());
    let opponent = filter_text(filters.opponent.as_deref());
    let color = match filter_text(filters.color.as_deref()).as_deref() {
        None => None,
        Some("white") => Some(PlayerColor::White),
        Some("black") => Some(PlayerColor::Black),
        Some(_) => return Err(String::from("color must be white or black")),
    };
    let result = match filter_text(filters.result.as_deref()).as_deref() {
        None => None,
        Some("win") => Some(ResultFilter::Win),
        Some("loss") => Some(ResultFilter::Loss),
        Some("draw") => Some(ResultFilter::Draw),
        Some("unfinished") => Some(ResultFilter::Unfinished),
        Some(_) => {
            return Err(String::from(
                "result must be win, loss, draw, or unfinished",
            ));
        }
    };
    let since = parse_filter_date("start date", filters.since.as_deref())?;
    let until = parse_filter_date("end date", filters.until.as_deref())?;
    let minimum_rating = parse_filter_rating("minimum rating", filters.minimum_rating.as_deref())?;
    let maximum_rating = parse_filter_rating("maximum rating", filters.maximum_rating.as_deref())?;
    let position = filter_text(filters.position.as_deref())
        .map(|value| {
            Position::from_fen(value.as_bytes())
                .map_err(|error| format!("position must be a valid six-field FEN: {error}"))
        })
        .transpose()?;
    if player.is_none()
        && (opponent.is_some()
            || color.is_some()
            || minimum_rating.is_some()
            || maximum_rating.is_some()
            || matches!(result, Some(ResultFilter::Win | ResultFilter::Loss)))
    {
        return Err(String::from(
            "opponent, color, rating, and win/loss filters require a player",
        ));
    }
    if since.zip(until).is_some_and(|(start, end)| start > end) {
        return Err(String::from("start date must not be later than end date"));
    }
    if minimum_rating
        .zip(maximum_rating)
        .is_some_and(|(minimum, maximum)| minimum > maximum)
    {
        return Err(String::from(
            "minimum rating must not exceed maximum rating",
        ));
    }
    Ok(QueryOptions {
        player,
        opponent,
        color,
        result,
        since,
        until,
        minimum_rating,
        maximum_rating,
        position,
    })
}

fn filter_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_filter_date(label: &str, value: Option<&str>) -> Result<Option<u32>, String> {
    filter_text(value)
        .map(|value| {
            query::parse_date(&value)
                .ok_or_else(|| format!("{label} must be a real date in YYYY-MM-DD format"))
        })
        .transpose()
}

fn parse_filter_rating(label: &str, value: Option<&str>) -> Result<Option<u32>, String> {
    filter_text(value)
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| format!("{label} must be a non-negative integer"))
        })
        .transpose()
}

fn temporary_sibling(path: &Path) -> PathBuf {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("gambit-output");
    path.with_file_name(format!(".{filename}.tmp-{}", std::process::id()))
}

fn replace_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    match fs::rename(temporary, destination) {
        Ok(()) => Ok(()),
        Err(first_error) if destination.exists() => {
            fs::remove_file(destination)
                .map_err(|error| format!("failed to replace {}: {error}", destination.display()))?;
            fs::rename(temporary, destination).map_err(|error| {
                format!(
                    "failed to replace {} after rename failed: {first_error}: {error}",
                    destination.display()
                )
            })
        }
        Err(error) => Err(format!(
            "failed to move {} to {}: {error}",
            temporary.display(),
            destination.display()
        )),
    }
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

fn normalized_token(token: Option<String>) -> Option<String> {
    token.and_then(|token| {
        let token = token.trim();
        (!token.is_empty()).then(|| token.to_owned())
    })
}

fn is_pgn_path(path: &Path) -> bool {
    extension_is(path, "pgn")
        || (extension_is(path, "zst")
            && path
                .file_stem()
                .is_some_and(|stem| extension_is(Path::new(stem), "pgn")))
}

fn extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            choose_database,
            open_database,
            list_databases,
            import_pgn,
            update_database,
            restore_session,
            sync_user,
            list_games,
            check_database,
            export_games,
            get_game,
            open_game_url,
            app_version,
            check_for_update,
            install_update,
            restart_app
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
    fn ignores_blank_tokens_and_trims_supplied_tokens() {
        assert_eq!(normalized_token(None), None);
        assert_eq!(normalized_token(Some(String::from("  "))), None);
        assert_eq!(
            normalized_token(Some(String::from(" lip_example \n"))).as_deref(),
            Some("lip_example")
        );
    }

    #[test]
    fn rejects_non_lichess_links() {
        assert!(open_game_url(String::from("https://example.com/game")).is_err());
    }

    #[test]
    fn accepts_only_supported_pgn_file_names() {
        assert!(is_pgn_path(Path::new("games.pgn")));
        assert!(is_pgn_path(Path::new("games.PGN.ZST")));
        assert!(!is_pgn_path(Path::new("games.zst")));
        assert!(!is_pgn_path(Path::new("games.gambit")));
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
        assert_eq!(saved.libraries.len(), 1);
        assert_eq!(saved.libraries[0].database, saved.database);
        assert_eq!(saved.libraries[0].managed_user.as_deref(), Some("diego"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recent_libraries_are_deduplicated_and_most_recent_first() {
        let root = std::env::temp_dir().join(format!(
            "gambit-desktop-recent-libraries-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let first = root.join("first.gambit");
        let second = root.join("second.gambit");

        write_saved_session(&root, &first, Some("first-player")).unwrap();
        write_saved_session(&root, &second, None).unwrap();
        write_saved_session(&root, &first, Some("updated-player")).unwrap();

        let saved = read_saved_session_from(&root).unwrap().unwrap();
        assert_eq!(saved.database, first);
        assert_eq!(saved.libraries.len(), 2);
        assert_eq!(saved.libraries[0].database, first);
        assert_eq!(
            saved.libraries[0].managed_user.as_deref(),
            Some("updated-player")
        );
        assert_eq!(saved.libraries[1].database, second);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn converts_desktop_filters_to_cli_query_options() {
        let filters = GameFilters {
            player: Some(String::from("  Diego  ")),
            opponent: Some(String::from("Opponent")),
            color: Some(String::from("white")),
            result: Some(String::from("win")),
            since: Some(String::from("2026-09-01")),
            until: Some(String::from("2026-09-30")),
            minimum_rating: Some(String::from("1200")),
            maximum_rating: Some(String::from("1300")),
            position: Some(String::from(
                "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
            )),
        };

        let options = query_options(&filters).unwrap();

        assert_eq!(options.player.as_deref(), Some("Diego"));
        assert_eq!(options.opponent.as_deref(), Some("Opponent"));
        assert_eq!(options.color, Some(PlayerColor::White));
        assert_eq!(options.result, Some(ResultFilter::Win));
        assert_eq!(options.since, Some(20_260_901));
        assert_eq!(options.until, Some(20_260_930));
        assert_eq!(options.minimum_rating, Some(1200));
        assert_eq!(options.maximum_rating, Some(1300));
        assert!(options.position.is_some());
    }

    #[test]
    fn rejects_player_relative_filters_without_a_player() {
        let filters = GameFilters {
            opponent: Some(String::from("Opponent")),
            ..GameFilters::default()
        };
        assert!(
            query_options(&filters)
                .unwrap_err()
                .contains("require a player")
        );

        let filters = GameFilters {
            since: Some(String::from("2026-10-01")),
            until: Some(String::from("2026-09-01")),
            ..GameFilters::default()
        };
        assert!(
            query_options(&filters)
                .unwrap_err()
                .contains("must not be later")
        );
    }
}
