//! Shared admission control for background diagnosis and practice workers.
//!
//! A library owns a session. Cancelling that session invalidates queued work as
//! well as the current search. A replacement session shares the same engine
//! gate, so a library switch cannot overlap old and new engine processes.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gambit_engine::{Analysis, Cancellation, Error, GamePosition};

#[derive(Clone, Default)]
pub struct EngineWorker {
    gate: Arc<Mutex<()>>,
}

impl EngineWorker {
    /// Create a cancellable scope. Keep one scope per active library/job and
    /// cancel it on replacement, sleep, or exit. Never call searches on the UI
    /// thread.
    #[must_use]
    pub fn session(&self) -> EngineSession {
        EngineSession {
            gate: Arc::clone(&self.gate),
            cancellation: Cancellation::default(),
        }
    }
}

#[derive(Clone)]
pub struct EngineSession {
    gate: Arc<Mutex<()>>,
    cancellation: Cancellation,
}

impl EngineSession {
    #[must_use]
    pub fn cancellation(&self) -> &Cancellation {
        &self.cancellation
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Cancel and wait for any admitted search to finish reaping its child.
    /// This is a blocking lifecycle operation, not a navigation operation.
    /// Waiting work checks cancellation after admission and cannot launch a
    /// child after this returns.
    ///
    /// # Errors
    /// Returns an error if a worker panicked while holding the engine gate.
    pub fn cancel_and_wait(&self) -> Result<(), Error> {
        self.cancel();
        let _guard = self
            .gate
            .lock()
            .map_err(|_| Error::Protocol("engine worker is unavailable"))?;
        Ok(())
    }

    /// Analyze one position while exclusively owning the shared engine slot.
    /// Release the slot between searches, allowing practice to interleave with
    /// a longer diagnosis without ever sharing an engine process.
    ///
    /// # Errors
    /// Returns cancellation, engine failures, or a poisoned worker error.
    pub fn analyze(
        &self,
        executable: &Path,
        position: &GamePosition,
        nodes: u64,
        timeout: Duration,
    ) -> Result<Analysis, Error> {
        self.exclusive(|| {
            gambit_engine::analyze_game(executable, position, nodes, &self.cancellation, timeout)
        })
    }

    fn exclusive<T>(&self, operation: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        if self.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let _guard = self
            .gate
            .lock()
            .map_err(|_| Error::Protocol("engine worker is unavailable"))?;
        if self.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        operation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn concurrent_sessions_never_overlap() {
        let worker = EngineWorker::default();
        let active = AtomicUsize::new(0);
        thread::scope(|scope| {
            for _ in 0..8 {
                let session = worker.session();
                let active = &active;
                scope.spawn(move || {
                    for _ in 0..100 {
                        session
                            .exclusive(|| {
                                assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                                thread::yield_now();
                                assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                                Ok(())
                            })
                            .unwrap();
                    }
                });
            }
        });
    }

    #[test]
    fn library_switch_cancels_active_and_waiting_old_work() {
        let worker = EngineWorker::default();
        let old = worker.session();
        let waiting = old.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        thread::scope(|scope| {
            scope.spawn(|| {
                old.exclusive(|| {
                    entered_tx.send(()).unwrap();
                    while !old.cancellation().is_cancelled() {
                        thread::yield_now();
                    }
                    Err::<(), _>(Error::Cancelled)
                })
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let queued =
                scope.spawn(move || waiting.exclusive::<()>(|| panic!("stale search ran")));
            old.cancel_and_wait().unwrap();
            assert!(matches!(queued.join().unwrap(), Err(Error::Cancelled)));
            // New library is admitted only after the old search has finished.
            worker.session().exclusive(|| Ok(())).unwrap();
        });
        assert!(matches!(old.exclusive(|| Ok(())), Err(Error::Cancelled)));
    }

    #[test]
    fn cancelling_one_job_does_not_cancel_another() {
        let worker = EngineWorker::default();
        let diagnosis = worker.session();
        let practice = worker.session();
        diagnosis.cancel();
        assert!(matches!(
            diagnosis.exclusive(|| Ok(())),
            Err(Error::Cancelled)
        ));
        practice.exclusive(|| Ok(())).unwrap();
    }

    #[test]
    fn failure_releases_slot_but_panic_closes_admission() {
        let worker = EngineWorker::default();
        let session = worker.session();
        assert!(session.exclusive::<()>(|| Err(Error::Exited)).is_err());
        session.exclusive(|| Ok(())).unwrap();
        let panicking = session.clone();
        assert!(
            thread::spawn(move || panicking.exclusive::<()>(|| panic!("worker panic")))
                .join()
                .is_err()
        );
        assert!(matches!(
            session.exclusive(|| Ok(())),
            Err(Error::Protocol(_))
        ));
        assert!(session.cancel_and_wait().is_err());
    }
}
