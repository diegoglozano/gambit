use gambit_coaching::{
    CacheError, CacheKey, CacheStore, Diagnosis, EngineIdentity, PracticeDisposition, ReviewGame,
    diagnose,
};
use gambit_engine::Cancellation;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const PGN: &str = "[White \"A\"]\n[Black \"B\"]\n1.d4 d5 *";

struct Fixture {
    root: tempfile::TempDir,
    library: PathBuf,
    executable: PathBuf,
    game: ReviewGame,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library.gambit");
        let executable = root.path().join("engine");
        fs::write(&library, b"immutable indexed evidence").unwrap();
        fs::write(&executable, b"engine binary fixture").unwrap();
        fs::write(
            root.path().join("session.json"),
            b"existing sync and review state",
        )
        .unwrap();
        Self {
            root,
            library,
            executable,
            game: ReviewGame::parse(PGN.as_bytes(), "A").unwrap(),
        }
    }
    fn identity(&self) -> EngineIdentity {
        EngineIdentity::from_file("fixture", &self.executable).unwrap()
    }
    fn store(&self) -> CacheStore {
        CacheStore::for_library(self.root.path(), &self.library).unwrap()
    }
    fn key(&self) -> CacheKey {
        CacheKey::new("game-1", &self.game, 100, &self.identity(), 100).unwrap()
    }
    fn diagnosis(&self) -> Diagnosis {
        diagnose(&self.game, 100, 100, &Cancellation::default(), |_| {
            panic!("no selected decisions")
        })
        .unwrap()
    }
}

fn record_file(root: &Path) -> PathBuf {
    let directory = fs::read_dir(root.join("coaching"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|e| e == "json"))
        .unwrap()
}

#[test]
fn roundtrip_preserves_practice_and_never_changes_library_metadata() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let key = fixture.key();
    assert!(store.load(&key).unwrap().is_none());
    assert!(!fixture.root.path().join("coaching").exists());
    store.save_diagnosis(&key, fixture.diagnosis()).unwrap();
    store
        .update_practice(&key, |p| p.disposition = PracticeDisposition::Completed)
        .unwrap();
    store.save_diagnosis(&key, fixture.diagnosis()).unwrap();
    let reopened = fixture.store().load(&key).unwrap().unwrap();
    assert_eq!(
        reopened.practice.disposition,
        PracticeDisposition::Completed
    );
    assert_eq!(
        fs::read(&fixture.library).unwrap(),
        b"immutable indexed evidence"
    );
    assert_eq!(
        fs::read(fixture.root.path().join("session.json")).unwrap(),
        b"existing sync and review state"
    );
}

#[test]
fn keys_invalidate_only_when_analysis_inputs_change() {
    let fixture = Fixture::new();
    let base = fixture.key();
    let decorated = ReviewGame::parse(
        b"[White \"A\"]\n[Black \"B\"]\n1.d4 {comment} (1.e4) d5 *",
        "a",
    )
    .unwrap();
    assert_eq!(
        base,
        CacheKey::new("game-1", &decorated, 100, &fixture.identity(), 100).unwrap()
    );
    let changed = ReviewGame::parse(b"[White \"A\"]\n[Black \"B\"]\n1.e4 e5 *", "A").unwrap();
    let black = ReviewGame::parse(PGN.as_bytes(), "B").unwrap();
    let swapped = ReviewGame::parse(b"[White \"B\"]\n[Black \"A\"]\n1.d4 d5 *", "A").unwrap();
    for key in [
        CacheKey::new("game-2", &fixture.game, 100, &fixture.identity(), 100).unwrap(),
        CacheKey::new("game-1", &changed, 100, &fixture.identity(), 100).unwrap(),
        CacheKey::new("game-1", &black, 100, &fixture.identity(), 100).unwrap(),
        CacheKey::new("game-1", &swapped, 100, &fixture.identity(), 100).unwrap(),
        CacheKey::new("game-1", &fixture.game, 99, &fixture.identity(), 100).unwrap(),
        CacheKey::new("game-1", &fixture.game, 100, &fixture.identity(), 200).unwrap(),
    ] {
        assert_ne!(key, base);
    }
    fs::write(
        &fixture.executable,
        b"updated binary, same reported version",
    )
    .unwrap();
    assert_ne!(fixture.key(), base);
    for field in [
        "schema",
        "selection_version",
        "evidence_version",
        "threads",
        "hash_mib",
        "multipv",
    ] {
        let mut value = serde_json::to_value(&base).unwrap();
        value[field] = serde_json::json!(99);
        let changed: CacheKey = serde_json::from_value(value).unwrap();
        assert_ne!(changed, base);
    }
}

#[test]
fn libraries_are_isolated_and_stale_results_are_not_reused() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let key = fixture.key();
    store.save_diagnosis(&key, fixture.diagnosis()).unwrap();
    let second = fixture.root.path().join("other.gambit");
    fs::write(&second, b"other evidence").unwrap();
    let other = CacheStore::for_library(fixture.root.path(), &second).unwrap();
    assert!(other.load(&key).unwrap().is_none());
    let changed = CacheKey::new("game-1", &fixture.game, 101, &fixture.identity(), 100).unwrap();
    assert!(store.load(&changed).unwrap().is_none());
    assert!(store.load(&key).unwrap().is_some());
    #[cfg(unix)]
    {
        let alias = fixture.root.path().join("alias.gambit");
        std::os::unix::fs::symlink(&fixture.library, &alias).unwrap();
        assert!(
            CacheStore::for_library(fixture.root.path(), &alias)
                .unwrap()
                .load(&key)
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn invalid_updates_and_corrupt_files_preserve_existing_data() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let key = fixture.key();
    store.save_diagnosis(&key, fixture.diagnosis()).unwrap();
    let path = record_file(fixture.root.path());
    let original = fs::read(&path).unwrap();
    assert!(store.update_practice(&key, |p| p.revealed = true).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    let mut inconsistent = fixture.diagnosis();
    inconsistent.nodes = 200;
    assert!(store.save_diagnosis(&key, inconsistent).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::write(&path, b"{interrupted").unwrap();
    assert!(matches!(store.load(&key), Err(CacheError::Corrupt)));
    assert!(store.save_diagnosis(&key, fixture.diagnosis()).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{interrupted");
    fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(matches!(store.load(&key), Err(CacheError::Corrupt)));
}

#[test]
fn concurrent_readers_see_whole_records_during_atomic_updates() {
    let fixture = Fixture::new();
    let store = Arc::new(fixture.store());
    let key = fixture.key();
    store.save_diagnosis(&key, fixture.diagnosis()).unwrap();
    let reader_store = Arc::clone(&store);
    let reader_key = key.clone();
    let reader = std::thread::spawn(move || {
        for _ in 0..100 {
            assert!(reader_store.load(&reader_key).unwrap().is_some());
        }
    });
    for i in 0..20 {
        store
            .update_practice(&key, |p| {
                p.disposition = if i % 2 == 0 {
                    PracticeDisposition::Completed
                } else {
                    PracticeDisposition::Active
                }
            })
            .unwrap();
    }
    reader.join().unwrap();
    let directory = record_file(fixture.root.path())
        .parent()
        .unwrap()
        .to_owned();
    assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
}

#[test]
fn turning_point_progress_roundtrips_and_illegal_cached_notation_is_rejected() {
    use gambit_chess::MoveList;
    use gambit_engine::{Analysis, Score, ScoreBound, ScoreSource};
    let fixture = Fixture::new();
    let key = CacheKey::new("lesson", &fixture.game, 0, &fixture.identity(), 100).unwrap();
    let diagnosis = diagnose(&fixture.game, 0, 100, &Cancellation::default(), |history| {
        let board = history.position();
        let mut legal = MoveList::default();
        board.generate_legal_moves(&mut legal);
        let chess_move = legal.as_slice()[0];
        Ok(Analysis {
            engine: "fixture".into(),
            nodes_budget: 100,
            score: Score::Centipawns(if history.moves().is_empty() { 0 } else { 200 }),
            score_bound: ScoreBound::Exact,
            score_source: ScoreSource::LatestReport,
            depth: Some(12),
            best_move: Some(chess_move.to_uci()),
            pv: vec![chess_move.to_uci()],
            pv_san: vec![board.to_san(chess_move).unwrap()],
        })
    })
    .unwrap();
    assert!(matches!(
        diagnosis.outcome,
        gambit_coaching::DiagnosisOutcome::TurningPoint(_)
    ));
    let store = fixture.store();
    store.save_diagnosis(&key, diagnosis.clone()).unwrap();
    assert!(
        store
            .update_practice(&key, |p| p.disposition = PracticeDisposition::Completed)
            .is_err()
    );
    store
        .update_practice(&key, |p| {
            p.revealed = true;
            p.disposition = PracticeDisposition::Completed;
        })
        .unwrap();
    assert!(
        fixture
            .store()
            .load(&key)
            .unwrap()
            .unwrap()
            .practice
            .revealed
    );
    let other = CacheKey::new("other-lesson", &fixture.game, 0, &fixture.identity(), 100).unwrap();
    let mut corrupt = diagnosis;
    if let gambit_coaching::DiagnosisOutcome::TurningPoint(point) = &mut corrupt.outcome {
        point.best_san = "not legal chess notation".into();
        point.pv_san[0] = point.best_san.clone();
    }
    assert!(store.save_diagnosis(&other, corrupt).is_err());
    assert!(store.load(&other).unwrap().is_none());
}
