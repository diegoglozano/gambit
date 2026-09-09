//! Desktop-owned background jobs. Library evidence is read-only; completed
//! analysis is stored separately through the coaching cache.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gambit_coaching::{
    CacheEntry, CacheKey, CacheStore, DEFAULT_NODES, EngineIdentity, EngineSession, EngineWorker,
    PracticeDisposition, ReviewGame, ReviewSummary, diagnose, summarize, verify_attempt,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
pub(super) struct Request {
    pub expected_path: PathBuf,
    pub game_ids: Vec<i64>,
    pub player: String,
    pub shared_ply: usize,
    /// False only loads existing results; it never starts the engine.
    pub analyze: bool,
}

impl Request {
    fn validate(&self) -> Result<(), String> {
        if self.game_ids.is_empty()
            || self.game_ids.len() > 6
            || self.game_ids.iter().collect::<HashSet<_>>().len() != self.game_ids.len()
            || self.player.trim().is_empty()
            || self.player.len() > 256
            || self.shared_ply > 1024
        {
            return Err("select one to six distinct games and an explicit player".into());
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Unseen,
    Analyzing,
    Ready,
    Unsupported,
    Failed,
}

#[derive(Clone, Serialize)]
pub(super) struct Game {
    id: i64,
    status: Status,
    record: Option<CacheEntry>,
    message: Option<String>,
}

#[derive(Clone, Serialize)]
pub(super) struct Snapshot {
    generation: u64,
    revision: u64,
    path: PathBuf,
    player: String,
    shared_ply: usize,
    running: bool,
    cancelled: bool,
    practice_busy: bool,
    games: Vec<Game>,
    message: Option<String>,
    summary: Option<ReviewSummary>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", content = "uci", rename_all = "snake_case")]
pub(super) enum PracticeAction {
    Attempt(String),
    Reveal,
    Done,
    Later,
    Replay,
}

type PracticeReply = std::sync::mpsc::Receiver<Result<Snapshot, String>>;

struct Job {
    session: EngineSession,
    practice: bool,
    thread: JoinHandle<()>,
}

#[derive(Default)]
pub(super) struct Service {
    engine: EngineWorker,
    generation: u64,
    current: Option<Arc<Mutex<Snapshot>>>,
    jobs: Vec<Job>,
}

impl Service {
    pub fn start(
        &mut self,
        request: Request,
        app_data: PathBuf,
        executable: PathBuf,
        publish: impl Fn(Snapshot) + Send + 'static,
    ) -> Result<Snapshot, String> {
        request.validate()?;
        if self.jobs.iter().any(|job| !job.thread.is_finished()) {
            return Err("wait for the current diagnosis to finish or cancel it".into());
        }
        for job in self.jobs.drain(..) {
            let _ = job.thread.join();
        }
        self.generation = self.generation.wrapping_add(1);
        let initial = Snapshot {
            generation: self.generation,
            revision: 0,
            path: request.expected_path.clone(),
            player: request.player.clone(),
            shared_ply: request.shared_ply,
            running: true,
            cancelled: false,
            practice_busy: false,
            games: request
                .game_ids
                .iter()
                .map(|&id| Game {
                    id,
                    status: Status::Unseen,
                    record: None,
                    message: None,
                })
                .collect(),
            message: None,
            summary: None,
        };
        let snapshot = Arc::new(Mutex::new(initial.clone()));
        let background = Arc::clone(&snapshot);
        let session = self.engine.session();
        let search = session.clone();
        let handle = thread::Builder::new()
            .name("review-diagnosis".into())
            .spawn(move || {
                let result = run_queue(
                    &request,
                    &app_data,
                    &executable,
                    &search,
                    &background,
                    &publish,
                );
                update(&background, &publish, |state| {
                    state.running = false;
                    state.cancelled = search.cancellation().is_cancelled();
                    if let Err(message) = result {
                        state.message = Some(message);
                    }
                    for game in &mut state.games {
                        if game.status == Status::Analyzing {
                            game.status = Status::Unseen;
                        }
                    }
                });
            })
            .map_err(|_| "could not start the local analysis worker")?;
        self.current = Some(Arc::clone(&snapshot));
        self.jobs.push(Job {
            session,
            practice: false,
            thread: handle,
        });
        Ok(initial)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn practice(
        &mut self,
        path: &std::path::Path,
        generation: u64,
        game_id: i64,
        action: PracticeAction,
        app_data: PathBuf,
        executable: PathBuf,
        publish: impl Fn(Snapshot) + Send + 'static,
    ) -> Result<PracticeReply, String> {
        let snapshot = self.snapshot()?.ok_or("open a diagnosis first")?;
        if snapshot.path != path || snapshot.generation != generation {
            return Err("the active diagnosis has changed".into());
        }
        if self
            .jobs
            .iter()
            .any(|job| job.practice && !job.thread.is_finished())
        {
            return Err("wait for the current practice action".into());
        }
        let index = snapshot
            .games
            .iter()
            .position(|game| game.id == game_id)
            .ok_or("this game is not in the active set")?;
        let record = snapshot.games[index]
            .record
            .clone()
            .ok_or("diagnose this game first")?;
        let background = Arc::clone(self.current.as_ref().ok_or("open a diagnosis first")?);
        let session = self.engine.session();
        let search = session.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = thread::Builder::new()
            .name("review-practice".into())
            .spawn(move || {
                update(&background, &publish, |s| s.practice_busy = true);
                let result =
                    practice_action(&snapshot, &app_data, &executable, &search, &record, action);
                update(&background, &publish, |s| {
                    s.practice_busy = false;
                    if let Ok(record) = &result {
                        s.games[index].record = Some(record.clone());
                    }
                });
                let reply = result.and_then(|_| {
                    background
                        .lock()
                        .map(|s| s.clone())
                        .map_err(|_| "analysis state is unavailable".into())
                });
                let _ = sender.send(reply);
            })
            .map_err(|_| "could not start the practice worker")?;
        // Reap completed jobs so repeated practice cannot grow thread metadata.
        let mut retained = Vec::new();
        for job in self.jobs.drain(..) {
            if job.thread.is_finished() {
                let _ = job.thread.join();
            } else {
                retained.push(job);
            }
        }
        self.jobs = retained;
        self.jobs.push(Job {
            session,
            practice: true,
            thread: handle,
        });
        Ok(receiver)
    }

    pub fn snapshot(&self) -> Result<Option<Snapshot>, String> {
        self.current
            .as_ref()
            .map(|state| {
                state
                    .lock()
                    .map(|s| s.clone())
                    .map_err(|_| "analysis state is unavailable".into())
            })
            .transpose()
    }

    pub fn cancel_request(
        &mut self,
        path: &std::path::Path,
        generation: u64,
    ) -> Result<(), String> {
        let current = self.snapshot()?.ok_or("there is no active diagnosis")?;
        if current.path != path || current.generation != generation {
            return Err("the active diagnosis has changed".into());
        }
        self.cancel(false);
        Ok(())
    }

    /// Nonblocking: normal game navigation does not call this. A library switch
    /// discards the visible snapshot, but keeps thread ownership until reaped.
    pub fn cancel(&mut self, clear: bool) {
        for job in &self.jobs {
            job.session.cancel();
        }
        if clear {
            self.current = None;
        }
    }

    pub fn shutdown(&mut self) {
        self.cancel(true);
        for job in self.jobs.drain(..) {
            let _ = job.thread.join();
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn update(
    state: &Mutex<Snapshot>,
    publish: &impl Fn(Snapshot),
    change: impl FnOnce(&mut Snapshot),
) {
    let snapshot = {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        change(&mut state);
        state.revision = state.revision.saturating_add(1);
        let records = state
            .games
            .iter()
            .filter_map(|game| game.record.clone())
            .collect::<Vec<_>>();
        state.summary = summarize(&records).ok();
        state.clone()
    };
    publish(snapshot);
}

fn practice_action(
    snapshot: &Snapshot,
    app_data: &std::path::Path,
    executable: &std::path::Path,
    session: &EngineSession,
    record: &CacheEntry,
    action: PracticeAction,
) -> Result<CacheEntry, String> {
    let store = CacheStore::for_library(app_data, &snapshot.path)
        .map_err(|_| "practice storage is unavailable")?;
    let id = record
        .key
        .game_id()
        .parse::<i64>()
        .map_err(|_| "invalid game identity")?;
    let detail =
        gambit::index::game(&snapshot.path, id).map_err(|_| "the source game is unavailable")?;
    let game = ReviewGame::parse(detail.pgn.as_bytes(), &snapshot.player)
        .map_err(|_| "the source game has changed")?;
    let identity = EngineIdentity::from_file("Stockfish 17.1", executable)
        .map_err(|_| "the local engine is unavailable")?;
    let key = CacheKey::new(
        record.key.game_id(),
        &game,
        snapshot.shared_ply,
        &identity,
        DEFAULT_NODES,
    )
    .map_err(|_| "invalid practice inputs")?;
    if key != record.key {
        return Err("the game or engine has changed; reload the diagnosis".into());
    }
    if session.cancellation().is_cancelled() {
        return Err("practice cancelled".into());
    }
    if let PracticeAction::Attempt(uci) = action {
        let attempt = verify_attempt(
            &record.diagnosis,
            &uci,
            session.cancellation(),
            |position| {
                session.analyze(executable, position, DEFAULT_NODES, Duration::from_secs(30))
            },
        )
        .map_err(|_| "the move could not be verified; please retry")?;
        if session.cancellation().is_cancelled() {
            return Err("practice cancelled".into());
        }
        return store
            .record_attempt(&record.key, attempt)
            .map_err(|_| "practice could not be saved".into());
    }
    store
        .update_practice(&record.key, |progress| match action {
            PracticeAction::Reveal => progress.revealed = true,
            PracticeAction::Done => progress.disposition = PracticeDisposition::Completed,
            PracticeAction::Later => progress.disposition = PracticeDisposition::AgainLater,
            PracticeAction::Replay => progress.disposition = PracticeDisposition::Active,
            PracticeAction::Attempt(_) => unreachable!(),
        })
        .map_err(|_| "this practice action cannot be saved yet".into())
}

fn run_queue(
    request: &Request,
    app_data: &std::path::Path,
    executable: &std::path::Path,
    session: &EngineSession,
    state: &Mutex<Snapshot>,
    publish: &impl Fn(Snapshot),
) -> Result<(), String> {
    let store = CacheStore::for_library(app_data, &request.expected_path)
        .map_err(|_| "local analysis storage is unavailable")?;
    // The packaged build is pinned; the executable hash also invalidates any
    // replaced binary. No probe/search is needed for read-only cache loading.
    let identity = EngineIdentity::from_file("Stockfish 17.1", executable)
        .map_err(|_| "the bundled local engine is unavailable; reinstall Gambit and retry")?;
    // Load every cached game before doing any searches, so later ready games
    // are immediately accessible while an earlier uncached game is analyzed.
    let mut pending = Vec::new();
    for (index, &id) in request.game_ids.iter().enumerate() {
        if session.cancellation().is_cancelled() {
            return Ok(());
        }
        let Ok(detail) = gambit::index::game(&request.expected_path, id) else {
            failed(
                state,
                publish,
                index,
                Status::Failed,
                "this game could not be read",
            );
            continue;
        };
        let Ok(game) = ReviewGame::parse(detail.pgn.as_bytes(), &request.player) else {
            failed(
                state,
                publish,
                index,
                Status::Unsupported,
                "standard chess and an unambiguous selected player are required",
            );
            continue;
        };
        let key = CacheKey::new(
            &id.to_string(),
            &game,
            request.shared_ply,
            &identity,
            DEFAULT_NODES,
        )
        .map_err(|_| "invalid analysis inputs")?;
        match store.load(&key) {
            Ok(Some(record)) => update(state, publish, |s| {
                s.games[index].status = Status::Ready;
                s.games[index].record = Some(record);
            }),
            Ok(None) => pending.push((index, game, key)),
            Err(_) => failed(
                state,
                publish,
                index,
                Status::Failed,
                "saved analysis could not be read; existing progress has been preserved",
            ),
        }
    }
    if !request.analyze {
        return Ok(());
    }
    for (index, game, key) in pending {
        if session.cancellation().is_cancelled() {
            break;
        }
        update(state, publish, |s| {
            s.games[index].status = Status::Analyzing;
        });
        let diagnosis = diagnose(
            &game,
            request.shared_ply,
            DEFAULT_NODES,
            session.cancellation(),
            |position| {
                session.analyze(executable, position, DEFAULT_NODES, Duration::from_secs(30))
            },
        );
        if session.cancellation().is_cancelled() {
            break;
        }
        match diagnosis {
            Ok(diagnosis) => match store.save_diagnosis(&key, diagnosis) {
                Ok(record) => update(state, publish, |s| {
                    s.games[index].status = Status::Ready;
                    s.games[index].record = Some(record);
                }),
                Err(_) => failed(
                    state,
                    publish,
                    index,
                    Status::Failed,
                    "analysis could not be saved; existing progress has been preserved",
                ),
            },
            Err(_) => failed(
                state,
                publish,
                index,
                Status::Failed,
                "local analysis failed for this game; retry is available",
            ),
        }
    }
    Ok(())
}

fn failed(
    state: &Mutex<Snapshot>,
    publish: &impl Fn(Snapshot),
    index: usize,
    status: Status,
    message: &str,
) {
    update(state, publish, |s| {
        s.games[index].status = status;
        s.games[index].message = Some(message.into());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish(service: &mut Service) -> Snapshot {
        for job in service.jobs.drain(..) {
            job.thread.join().unwrap();
        }
        service.snapshot().unwrap().unwrap()
    }

    #[test]
    #[allow(clippy::too_many_lines)] // End-to-end cache/practice/reopen lifecycle.
    fn queue_resumes_cached_results_and_isolates_failed_games() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library.gambit");
        let executable = root.path().join("engine");
        // Deliberately not executable: loading cached results and a game with
        // no eligible decisions must never try to launch it.
        std::fs::write(&executable, b"synthetic engine identity").unwrap();
        let pgn = b"[White \"A\"]\n[Black \"B\"]\n1.d4 d5 *";
        let mut builder = gambit::index::Builder::create(&library).unwrap();
        builder.add(&pgn[..], "synthetic.pgn").unwrap();
        builder.finish().unwrap();
        let original = std::fs::read(&library).unwrap();
        let request = Request {
            expected_path: library.clone(),
            game_ids: vec![1, 999],
            player: "A".into(),
            shared_ply: 4,
            analyze: false,
        };
        let mut service = Service::default();
        service
            .start(
                request.clone(),
                root.path().into(),
                executable.clone(),
                |_| {},
            )
            .unwrap();
        let loaded = finish(&mut service);
        assert_eq!(loaded.games[0].status, Status::Unseen);
        assert_eq!(loaded.games[1].status, Status::Failed);
        assert!(!root.path().join("coaching").exists());
        let mut analyze = request.clone();
        analyze.analyze = true;
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        service
            .start(
                analyze,
                root.path().into(),
                executable.clone(),
                move |snapshot| {
                    observed.lock().unwrap().push(snapshot);
                },
            )
            .unwrap();
        let analyzed = finish(&mut service);
        assert_eq!(analyzed.games[0].status, Status::Ready);
        assert_eq!(analyzed.games[1].status, Status::Failed);
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|s| s.running && s.games[0].status == Status::Ready)
        );
        assert!(
            service
                .practice(
                    &library,
                    analyzed.generation + 1,
                    1,
                    PracticeAction::Done,
                    root.path().into(),
                    executable.clone(),
                    |_| {}
                )
                .is_err()
        );
        let reply = service
            .practice(
                &library,
                analyzed.generation,
                1,
                PracticeAction::Done,
                root.path().into(),
                executable.clone(),
                |_| {},
            )
            .unwrap();
        let completed = reply.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        assert!(!completed.practice_busy);
        assert!(completed.revision > analyzed.revision);
        finish(&mut service);
        // Engine replacement invalidates even a non-search practice action.
        std::fs::write(&executable, b"changed engine").unwrap();
        let reply = service
            .practice(
                &library,
                analyzed.generation,
                1,
                PracticeAction::Replay,
                root.path().into(),
                executable.clone(),
                |_| {},
            )
            .unwrap();
        assert!(reply.recv_timeout(Duration::from_secs(5)).unwrap().is_err());
        finish(&mut service);
        std::fs::write(&executable, b"synthetic engine identity").unwrap();
        service.shutdown();
        let mut reopened = Service::default();
        reopened
            .start(request, root.path().into(), executable, |_| {})
            .unwrap();
        let loaded = finish(&mut reopened);
        assert!(
            reopened
                .cancel_request(&library, loaded.generation + 1)
                .is_err()
        );
        assert!(
            reopened
                .cancel_request(root.path(), loaded.generation)
                .is_err()
        );
        reopened
            .cancel_request(&library, loaded.generation)
            .unwrap();
        assert_eq!(loaded.games[0].status, Status::Ready);
        assert_eq!(
            loaded.games[0]
                .record
                .as_ref()
                .unwrap()
                .practice
                .disposition,
            gambit_coaching::PracticeDisposition::Completed
        );
        assert_eq!(std::fs::read(&library).unwrap(), original);
    }

    #[test]
    fn queue_requires_bounded_distinct_games_and_player() {
        let mut request = Request {
            expected_path: "library.gambit".into(),
            game_ids: vec![1, 2],
            player: "Player".into(),
            shared_ply: 4,
            analyze: false,
        };
        assert!(request.validate().is_ok());
        request.game_ids = vec![1, 1];
        assert!(request.validate().is_err());
        request.game_ids = (1..=7).collect();
        assert!(request.validate().is_err());
        request.game_ids.clear();
        assert!(request.validate().is_err());
        request.game_ids.push(1);
        request.player = " ".into();
        assert!(request.validate().is_err());
    }

    #[test]
    fn missing_engine_finishes_without_touching_library_or_cache() {
        let mut service = Service::default();
        let request = Request {
            expected_path: "missing-library.gambit".into(),
            game_ids: vec![1],
            player: "Player".into(),
            shared_ply: 4,
            analyze: true,
        };
        service
            .start(
                request,
                "missing-app-data".into(),
                "missing-engine".into(),
                |_| {},
            )
            .unwrap();
        for job in service.jobs.drain(..) {
            job.thread.join().unwrap();
        }
        let snapshot = service.snapshot().unwrap().unwrap();
        assert!(!snapshot.running);
        assert!(snapshot.message.is_some());
        assert_eq!(snapshot.games[0].status, Status::Unseen);
        service.cancel(true);
        assert!(service.snapshot().unwrap().is_none());
    }
}
