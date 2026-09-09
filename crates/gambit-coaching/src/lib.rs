//! Local review evidence. No network access or implicit whole-library analysis.

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
