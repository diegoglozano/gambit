//! Local review evidence. No network access or implicit whole-library analysis.

mod worker;
pub use worker::{EngineSession, EngineWorker};

mod replay;
pub use replay::{Decision, InputError, ReviewGame};

mod score;
pub use score::{Assessment, Evaluation, Loss, PlayerScore, assess};

mod diagnosis;
pub use diagnosis::{
    DEFAULT_NODES, Diagnosis, DiagnosisError, DiagnosisOutcome, TurningPoint, diagnose,
};

mod cache;
pub use cache::{
    CacheEntry, CacheError, CacheKey, CacheStore, EngineIdentity, PracticeDisposition,
    PracticeProgress, SolutionStatus,
};

mod practice;
pub use practice::{Attempt, AttemptVerdict, PracticeError, assess_attempt, verify_attempt};

mod summary;
pub use summary::{
    LossStatistics, RepeatedChoice, RepeatedPosition, ReviewSummary, SummaryError, summarize,
};
