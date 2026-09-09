use crate::{Diagnosis, DiagnosisOutcome, ReviewGame};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const CACHE_SCHEMA: u32 = 1;
const MAX_RECORD_BYTES: u64 = 1024 * 1024;
// Serialize read/modify/write operations across stores in this desktop process.
static WRITES: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub enum CacheError {
    Io(io::Error),
    Corrupt,
    InvalidKey,
    InconsistentEvidence,
    Missing,
}
impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CacheError {}
impl From<io::Error> for CacheError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Clone)]
pub struct EngineIdentity {
    name: String,
    binary_hash: String,
}

impl EngineIdentity {
    /// Hash the actual local executable, not only its reported version string.
    ///
    /// # Errors
    /// Rejects missing/oversized files, invalid names, or read failures.
    pub fn from_file(name: &str, executable: &Path) -> Result<Self, CacheError> {
        if name.is_empty() || name.len() > 256 {
            return Err(CacheError::InvalidKey);
        }
        let file = File::open(executable)?;
        if file.metadata()?.len() > 512 * 1024 * 1024 {
            return Err(CacheError::InvalidKey);
        }
        let mut reader = file.take(512 * 1024 * 1024 + 1);
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0; 16 * 1024];
        let mut total = 0;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            total += count;
            if total > 512 * 1024 * 1024 {
                return Err(CacheError::InvalidKey);
            }
            hasher.update(&buffer[..count]);
        }
        Ok(Self {
            name: name.into(),
            binary_hash: hasher.finalize().to_hex().to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheKey {
    schema: u32,
    selection_version: u32,
    evidence_version: u32,
    practice_version: u32,
    game_id: String,
    mainline_hash: String,
    player: String,
    player_color: String,
    shared_ply: usize,
    engine_name: String,
    engine_binary_hash: String,
    nodes: u64,
    threads: u8,
    hash_mib: u16,
    multipv: u8,
    architecture: String,
}

impl CacheKey {
    #[must_use]
    pub fn game_id(&self) -> &str {
        &self.game_id
    }

    /// Identify exactly the analysis inputs. Comments/variations do not change
    /// the canonical mainline; setup FEN, player, shared ply and engine policy do.
    ///
    /// # Errors
    /// Rejects an empty/oversized game identity or invalid analysis budget.
    pub fn new(
        game_id: &str,
        game: &ReviewGame,
        shared_ply: usize,
        engine: &EngineIdentity,
        nodes: u64,
    ) -> Result<Self, CacheError> {
        if game_id.is_empty() || game_id.len() > 256 || nodes == 0 {
            return Err(CacheError::InvalidKey);
        }
        let mut hash = blake3::Hasher::new();
        framed(&mut hash, game.initial_fen().as_bytes());
        for (uci, _) in game.mainline() {
            framed(&mut hash, uci.as_bytes());
        }
        Ok(Self {
            schema: CACHE_SCHEMA,
            selection_version: crate::score::SELECTION_VERSION,
            evidence_version: gambit_engine::EVIDENCE_VERSION,
            practice_version: crate::practice::PRACTICE_VERSION,
            game_id: game_id.into(),
            mainline_hash: hash.finalize().to_hex().to_string(),
            player: game.player_name().into(),
            player_color: if game.player() == gambit_chess::Color::White {
                "white"
            } else {
                "black"
            }
            .into(),
            shared_ply,
            engine_name: engine.name.clone(),
            engine_binary_hash: engine.binary_hash.clone(),
            nodes,
            threads: 1,
            hash_mib: 16,
            multipv: 1,
            architecture: std::env::consts::ARCH.into(),
        })
    }

    fn digest(&self) -> Result<String, CacheError> {
        let bytes = serde_json::to_vec(self).map_err(|_| CacheError::InvalidKey)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
}

fn framed(hash: &mut blake3::Hasher, bytes: &[u8]) {
    hash.update(&(bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

/// Durable outcome flags; attempts are added by the practice service separately.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PracticeProgress {
    pub revealed: bool,
    pub solution: SolutionStatus,
    pub disposition: PracticeDisposition,
    #[serde(default)]
    pub total_attempts: u32,
    /// Keep the latest 100 attempts; retain the total count across older history.
    #[serde(default)]
    pub attempts: Vec<crate::Attempt>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolutionStatus {
    #[default]
    Unsolved,
    WithoutReveal,
    AfterHint,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PracticeDisposition {
    #[default]
    Active,
    Completed,
    AgainLater,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheEntry {
    pub key: CacheKey,
    pub diagnosis: Diagnosis,
    pub practice: PracticeProgress,
}

/// A separate per-library cache. Never writes indexed PGN, sync/session metadata,
/// or the legacy review-progress file. Stale-key records are retained, not reused.
#[derive(Debug)]
pub struct CacheStore {
    directory: PathBuf,
}

impl CacheStore {
    /// Resolve aliases to the same library while isolating distinct library paths.
    /// Construction is read-only; the cache directory is created on the first save.
    ///
    /// # Errors
    /// Rejects a missing/non-file library or a path that cannot be represented.
    pub fn for_library(app_data: &Path, library: &Path) -> Result<Self, CacheError> {
        let library = library.canonicalize()?;
        if !library.is_file() {
            return Err(CacheError::InvalidKey);
        }
        let identity = library.to_str().ok_or(CacheError::InvalidKey)?;
        Ok(Self {
            directory: app_data
                .join("coaching")
                .join(blake3::hash(identity.as_bytes()).to_hex().as_str()),
        })
    }

    fn path(&self, key: &CacheKey) -> Result<PathBuf, CacheError> {
        Ok(self.directory.join(format!("{}.json", key.digest()?)))
    }

    /// Read only a complete record matching every requested input.
    ///
    /// # Errors
    /// Returns corruption instead of trusting truncated, oversized or mismatched data.
    pub fn load(&self, key: &CacheKey) -> Result<Option<CacheEntry>, CacheError> {
        let file = match File::open(self.path(key)?) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(CacheError::Corrupt);
        }
        let record: CacheEntry = serde_json::from_slice(&bytes).map_err(|_| CacheError::Corrupt)?;
        if record.key != *key {
            return Err(CacheError::Corrupt);
        }
        validate(&record)?;
        Ok(Some(record))
    }

    /// Atomically save a completed game, preserving practice on an identical retry.
    ///
    /// # Errors
    /// Rejects inconsistent results or corrupt existing progress, without replacing it.
    pub fn save_diagnosis(
        &self,
        key: &CacheKey,
        diagnosis: Diagnosis,
    ) -> Result<CacheEntry, CacheError> {
        let _guard = WRITES.lock().map_err(|_| CacheError::Corrupt)?;
        let existing = self.load(key)?;
        if existing
            .as_ref()
            .is_some_and(|entry| entry.diagnosis != diagnosis)
        {
            return Err(CacheError::InconsistentEvidence);
        }
        let record = CacheEntry {
            key: key.clone(),
            diagnosis,
            practice: existing.map_or_else(PracticeProgress::default, |entry| entry.practice),
        };
        self.write(&record)?;
        Ok(record)
    }

    /// Apply one trusted backend practice update without racing an analysis save.
    /// The desktop must not accept arbitrary solved flags from frontend input.
    ///
    /// # Errors
    /// Returns missing/corrupt data or an atomic-write error without partial updates.
    pub fn update_practice(
        &self,
        key: &CacheKey,
        update: impl FnOnce(&mut PracticeProgress),
    ) -> Result<CacheEntry, CacheError> {
        let _guard = WRITES.lock().map_err(|_| CacheError::Corrupt)?;
        let mut record = self.load(key)?.ok_or(CacheError::Missing)?;
        update(&mut record.practice);
        self.write(&record)?;
        Ok(record)
    }

    /// Record feedback produced by the backend verifier, not a frontend verdict.
    ///
    /// # Errors
    /// Rejects invalid/missing exercise evidence or a failed atomic save.
    pub fn record_attempt(
        &self,
        key: &CacheKey,
        attempt: crate::Attempt,
    ) -> Result<CacheEntry, CacheError> {
        self.update_practice(key, |progress| {
            if attempt.verdict == crate::AttemptVerdict::Strong
                && progress.solution == SolutionStatus::Unsolved
            {
                progress.solution = if progress.revealed {
                    SolutionStatus::AfterHint
                } else {
                    SolutionStatus::WithoutReveal
                };
            }
            progress.total_attempts = progress.total_attempts.saturating_add(1);
            if progress.attempts.len() == 100 {
                progress.attempts.remove(0);
            }
            progress.attempts.push(attempt);
            progress.disposition = PracticeDisposition::Active;
        })
    }

    fn write(&self, record: &CacheEntry) -> Result<(), CacheError> {
        validate(record)?;
        let bytes = serde_json::to_vec(record).map_err(|_| CacheError::Corrupt)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(CacheError::Corrupt);
        }
        fs::create_dir_all(&self.directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(self.path(&record.key)?)
            .map_err(|e| CacheError::Io(e.error))?;
        // Persist the rename as well as the contents on supported Unix filesystems.
        #[cfg(unix)]
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
}

fn validate(record: &CacheEntry) -> Result<(), CacheError> {
    let key = &record.key;
    let diagnosis = &record.diagnosis;
    if key.schema != CACHE_SCHEMA
        || key.selection_version != crate::score::SELECTION_VERSION
        || key.evidence_version != gambit_engine::EVIDENCE_VERSION
        || key.practice_version != crate::practice::PRACTICE_VERSION
        || key.nodes != diagnosis.nodes
        || diagnosis.selection_version != key.selection_version
        || diagnosis.evidence_version != key.evidence_version
        || diagnosis.shared_ply != key.shared_ply
        || diagnosis.analyzed_moves > 1024
        || diagnosis.inconclusive_moves > diagnosis.analyzed_moves
        || diagnosis
            .engine
            .as_ref()
            .is_some_and(|engine| *engine != key.engine_name)
        || (diagnosis.analyzed_moves > 0 && diagnosis.engine.is_none())
    {
        return Err(CacheError::InconsistentEvidence);
    }
    if record.practice.attempts.len() > 100
        || u64::from(record.practice.total_attempts) < record.practice.attempts.len() as u64
    {
        return Err(CacheError::Corrupt);
    }
    if let DiagnosisOutcome::TurningPoint(point) = &diagnosis.outcome {
        if point.ply == 0
            || point.ply > 1024
            || point.history.len() != point.ply - 1
            || point.ply <= key.shared_ply
            || point.best_uci == point.played_uci
            || point.pv.first() != Some(&point.best_uci)
            || point.pv_san.first() != Some(&point.best_san)
            || point.pv.len() != point.pv_san.len()
            || point.pv.len() > 32
            || crate::assess(point.before, point.after)
                != crate::Assessment::TurningPoint(point.loss)
        {
            return Err(CacheError::InconsistentEvidence);
        }
        let mut history = gambit_engine::GamePosition::from_fen(&point.initial_fen)
            .map_err(|_| CacheError::Corrupt)?;
        for m in &point.history {
            history.play_uci(m).map_err(|_| CacheError::Corrupt)?;
        }
        if history.position().to_fen() != point.position_fen {
            return Err(CacheError::Corrupt);
        }
        let color = if history.position().side_to_move() == gambit_chess::Color::White {
            "white"
        } else {
            "black"
        };
        if key.player_color != color {
            return Err(CacheError::Corrupt);
        }
        validate_attempts(&history, &point.best_uci, &record.practice.attempts)?;
        let mut played = history.position();
        replay_notation(&mut played, &point.played_uci, &point.played_san)?;
        let mut variation = history.position();
        for (uci, san) in point.pv.iter().zip(&point.pv_san) {
            replay_notation(&mut variation, uci, san)?;
        }
        if record.practice.disposition == PracticeDisposition::Completed
            && !record.practice.revealed
            && record.practice.solution == SolutionStatus::Unsolved
        {
            return Err(CacheError::InconsistentEvidence);
        }
    } else if record.practice.revealed
        || record.practice.solution != SolutionStatus::Unsolved
        || record.practice.disposition == PracticeDisposition::AgainLater
        || record.practice.total_attempts != 0
        || !record.practice.attempts.is_empty()
    {
        return Err(CacheError::InconsistentEvidence);
    }
    if record.practice.solution == SolutionStatus::AfterHint && !record.practice.revealed {
        return Err(CacheError::InconsistentEvidence);
    }
    Ok(())
}

fn validate_attempts(
    history: &gambit_engine::GamePosition,
    best_uci: &str,
    attempts: &[crate::Attempt],
) -> Result<(), CacheError> {
    for attempt in attempts {
        if attempt.uci.len() > 5 {
            return Err(CacheError::Corrupt);
        }
        let mut attempted = history.clone();
        let legal = attempted.play_uci(&attempt.uci).is_ok();
        match (attempt.reference, attempt.evaluation) {
            (None, None) if !legal && attempt.verdict == crate::AttemptVerdict::Illegal => {}
            (Some(reference), Some(evaluation)) if legal => {
                let verdict = if attempt.uci == best_uci
                    && reference == evaluation
                    && !matches!(reference.score, crate::PlayerScore::MateAgainst(_))
                {
                    crate::AttemptVerdict::Strong
                } else {
                    crate::assess_attempt(reference, evaluation)
                };
                if verdict != attempt.verdict {
                    return Err(CacheError::Corrupt);
                }
            }
            _ => return Err(CacheError::Corrupt),
        }
    }
    Ok(())
}

fn replay_notation(
    position: &mut gambit_chess::Position,
    uci: &str,
    san: &str,
) -> Result<(), CacheError> {
    let mut moves = gambit_chess::MoveList::default();
    position.generate_legal_moves(&mut moves);
    let chess_move = moves
        .as_slice()
        .iter()
        .find(|m| m.to_uci() == uci)
        .copied()
        .ok_or(CacheError::Corrupt)?;
    if position
        .to_san(chess_move)
        .map_err(|_| CacheError::Corrupt)?
        != san
    {
        return Err(CacheError::Corrupt);
    }
    position.play_unchecked(chess_move);
    Ok(())
}
