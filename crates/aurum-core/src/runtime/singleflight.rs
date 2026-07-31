//! Per-key singleflight loading (JOE-1597).

use crate::error::{ProviderError, Result};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Identity for a loadable model/session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoadKey {
    pub kind: &'static str,
    pub id: String,
    pub path: String,
}

impl LoadKey {
    pub fn stt(id: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            kind: "stt",
            id: id.into(),
            path: path.into(),
        }
    }

    pub fn tts(id: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            kind: "tts",
            id: id.into(),
            path: path.into(),
        }
    }
}

enum SlotState<T> {
    Loading { waiters: usize },
    Ready(Arc<T>),
    Failed { message: String, at: Instant },
}

/// Coalesce concurrent loads for the same key.
pub struct Singleflight<T> {
    inner: Mutex<HashMap<LoadKey, SlotState<T>>>,
    cv: Condvar,
    /// How long to keep a failed result before allowing retry.
    fail_ttl: Duration,
}

impl<T> Singleflight<T> {
    pub fn new(fail_ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            cv: Condvar::new(),
            fail_ttl,
        }
    }

    pub fn get_ready(&self, key: &LoadKey) -> Option<Arc<T>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.get(key) {
            Some(SlotState::Ready(v)) => Some(Arc::clone(v)),
            _ => None,
        }
    }

    pub fn contains_ready(&self, key: &LoadKey) -> bool {
        self.get_ready(key).is_some()
    }

    pub fn invalidate(&self, key: &LoadKey) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(key);
        self.cv.notify_all();
    }

    pub fn clear(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.clear();
        self.cv.notify_all();
    }

    pub fn ready_count(&self) -> usize {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .values()
            .filter(|s| matches!(s, SlotState::Ready(_)))
            .count()
    }

    /// Complete a coalesced load without retaining a permanent Ready slot
    /// (JOE-1646). The caller must have already published the value into the
    /// authoritative registry. Waiters wake, see no Loading slot, and re-query
    /// the registry.
    pub fn finish_load_published(&self, key: &LoadKey) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(key);
        self.cv.notify_all();
    }

    /// Mark a failed coalesced load.
    pub fn finish_load_failed(&self, key: &LoadKey, message: String) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(
            key.clone(),
            SlotState::Failed {
                message,
                at: Instant::now(),
            },
        );
        self.cv.notify_all();
    }

    /// Begin a coalesced load for registry-owned residency (JOE-1646).
    ///
    /// - [`BeginLoad::Leader`]: this caller must load, insert into the registry,
    ///   then call [`Self::finish_load_published`] or [`Self::finish_load_failed`].
    /// - [`BeginLoad::WaitDone`]: a concurrent load finished; re-check the registry.
    /// - [`BeginLoad::Failed`]: recent failure still in TTL window.
    pub fn begin_or_wait(&self, key: &LoadKey) -> BeginLoad {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(SlotState::Failed { at, .. }) = guard.get(key) {
                if at.elapsed() > self.fail_ttl {
                    guard.remove(key);
                }
            }
            match guard.get_mut(key) {
                Some(SlotState::Ready(_)) => {
                    // Legacy Ready path: treat as completed, clear and let caller
                    // re-query registry.
                    guard.remove(key);
                    self.cv.notify_all();
                    return BeginLoad::WaitDone;
                }
                Some(SlotState::Failed { message, .. }) => {
                    return BeginLoad::Failed(message.clone());
                }
                Some(SlotState::Loading { waiters }) => {
                    *waiters += 1;
                    guard = self.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
                    // After wake: loading finished (published or failed).
                    // Loop to observe Failed or absence → WaitDone.
                    if !matches!(guard.get(key), Some(SlotState::Loading { .. })) {
                        if let Some(SlotState::Failed { message, .. }) = guard.get(key) {
                            return BeginLoad::Failed(message.clone());
                        }
                        return BeginLoad::WaitDone;
                    }
                }
                None => {
                    guard.insert(key.clone(), SlotState::Loading { waiters: 0 });
                    return BeginLoad::Leader;
                }
            }
        }
    }

    /// Acquire leadership for `key` and return a panic-safe [`LeaderGuard`].
    ///
    /// `Ok(None)` means another load finished — re-check the registry.
    /// `Err` is a cached failure message still within TTL.
    pub fn begin_or_wait_guard(
        &self,
        key: LoadKey,
    ) -> std::result::Result<Option<LeaderGuard<'_, T>>, String> {
        match self.begin_or_wait(&key) {
            BeginLoad::Leader => Ok(Some(LeaderGuard {
                flight: self,
                key,
                finished: false,
            })),
            BeginLoad::WaitDone => Ok(None),
            BeginLoad::Failed(m) => Err(m),
        }
    }
}

/// Result of [`Singleflight::begin_or_wait`].
#[derive(Debug)]
pub enum BeginLoad {
    /// This caller is responsible for loading and finishing the slot.
    Leader,
    /// A concurrent load completed; re-check the registry (or retry).
    WaitDone,
    /// A recent failure is still cached.
    Failed(String),
}

/// RAII leader for registry-publication loads (JOE-1646).
///
/// If dropped without [`LeaderGuard::success`] or [`LeaderGuard::fail`], the key is
/// marked Failed so waiters are not stuck in Loading forever (including panic unwind).
pub struct LeaderGuard<'a, T> {
    flight: &'a Singleflight<T>,
    key: LoadKey,
    finished: bool,
}

impl<'a, T> LeaderGuard<'a, T> {
    pub fn key(&self) -> &LoadKey {
        &self.key
    }

    pub fn success(mut self) {
        self.flight.finish_load_published(&self.key);
        self.finished = true;
    }

    pub fn fail(mut self, message: impl Into<String>) {
        self.flight.finish_load_failed(&self.key, message.into());
        self.finished = true;
    }
}

impl<T> Drop for LeaderGuard<'_, T> {
    fn drop(&mut self) {
        if !self.finished {
            self.flight.finish_load_failed(
                &self.key,
                "loader panicked or abandoned before publish".into(),
            );
        }
    }
}

impl<T> Default for Singleflight<T> {
    fn default() -> Self {
        Self::new(Duration::from_secs(2))
    }
}

impl<T: Send + Sync + 'static> Singleflight<T> {
    /// Load `key` using `loader` exactly once for concurrent callers.
    ///
    /// The registry mutex is **not** held during `loader`.
    /// Leader panics are converted to a failed slot so waiters never hang.
    pub fn get_or_load<F>(&self, key: LoadKey, loader: F) -> Result<Arc<T>>
    where
        F: FnOnce() -> Result<T>,
    {
        // Local role without nesting T in an enum (avoids E0401).
        let mut leader = false;
        let early: Option<Result<Arc<T>>> = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                // Drop stale failures so retries can proceed.
                if let Some(SlotState::Failed { at, .. }) = guard.get(&key) {
                    if at.elapsed() > self.fail_ttl {
                        guard.remove(&key);
                    }
                }

                match guard.get_mut(&key) {
                    Some(SlotState::Ready(v)) => {
                        break Some(Ok(Arc::clone(v)));
                    }
                    Some(SlotState::Failed { message, .. }) => {
                        break Some(Err(ProviderError::ModelLoad {
                            model: key.id.clone(),
                            reason: message.clone(),
                        }
                        .into()));
                    }
                    Some(SlotState::Loading { waiters }) => {
                        *waiters += 1;
                        guard = self.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
                        // Re-check after wake.
                    }
                    None => {
                        guard.insert(key.clone(), SlotState::Loading { waiters: 0 });
                        leader = true;
                        break None;
                    }
                }
            }
        };

        if let Some(r) = early {
            return r;
        }
        debug_assert!(leader);

        // Catch panics so the key is never stuck in Loading.
        let result = match catch_unwind(AssertUnwindSafe(loader)) {
            Ok(r) => r,
            Err(_) => Err(ProviderError::ModelLoad {
                model: key.id.clone(),
                reason: "loader panicked".into(),
            }
            .into()),
        };
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(value) => {
                let arc = Arc::new(value);
                guard.insert(key, SlotState::Ready(Arc::clone(&arc)));
                self.cv.notify_all();
                Ok(arc)
            }
            Err(e) => {
                let message = e.to_string();
                guard.insert(
                    key,
                    SlotState::Failed {
                        message,
                        at: Instant::now(),
                    },
                );
                self.cv.notify_all();
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[test]
    fn single_loader_for_many_waiters() {
        let sf = Arc::new(Singleflight::<u32>::default());
        let loads = Arc::new(AtomicUsize::new(0));
        let key = LoadKey::stt("m1", "/tmp/m1");
        let mut handles = vec![];
        for _ in 0..16 {
            let sf = Arc::clone(&sf);
            let loads = Arc::clone(&loads);
            let key = key.clone();
            handles.push(thread::spawn(move || {
                sf.get_or_load(key, || {
                    loads.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(30));
                    Ok(42)
                })
                .unwrap()
            }));
        }
        for h in handles {
            assert_eq!(*h.join().unwrap(), 42);
        }
        assert_eq!(loads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failure_delivered_to_waiters() {
        let sf = Arc::new(Singleflight::<u32>::new(Duration::from_secs(10)));
        let key = LoadKey::stt("bad", "/tmp/bad");
        let sf2 = Arc::clone(&sf);
        let key2 = key.clone();
        let leader = thread::spawn(move || {
            sf2.get_or_load(key2, || {
                thread::sleep(Duration::from_millis(20));
                Err(ProviderError::ModelLoad {
                    model: "bad".into(),
                    reason: "boom".into(),
                }
                .into())
            })
        });
        thread::sleep(Duration::from_millis(5));
        let waiter = sf.get_or_load(key, || Ok(1));
        assert!(leader.join().unwrap().is_err());
        assert!(waiter.is_err());
    }

    #[test]
    fn panic_does_not_stick_loading() {
        let sf = Singleflight::<u32>::new(Duration::from_millis(50));
        let key = LoadKey::stt("panic", "/tmp/p");
        let err = sf.get_or_load(key.clone(), || panic!("boom"));
        assert!(err.is_err());
        // After fail_ttl, a new load can proceed.
        thread::sleep(Duration::from_millis(60));
        let v = sf.get_or_load(key, || Ok(7)).unwrap();
        assert_eq!(*v, 7);
    }

    #[test]
    fn leader_guard_drop_unblocks_waiters() {
        let sf = Arc::new(Singleflight::<u32>::new(Duration::from_millis(80)));
        let key = LoadKey::stt("abandon", "/tmp/a");
        let sf2 = Arc::clone(&sf);
        let key2 = key.clone();
        let leader = thread::spawn(move || {
            let g = sf2.begin_or_wait_guard(key2).unwrap().expect("leader");
            // Simulate panic/abandon: drop guard without success/fail.
            drop(g);
        });
        thread::sleep(Duration::from_millis(10));
        let waiter = thread::spawn({
            let sf = Arc::clone(&sf);
            let key = key.clone();
            move || sf.begin_or_wait(&key)
        });
        leader.join().unwrap();
        match waiter.join().unwrap() {
            BeginLoad::Failed(m) => assert!(m.contains("abandon") || m.contains("panic")),
            other => panic!("expected Failed after abandon, got {other:?}"),
        }
        // After TTL a new leader can proceed and publish successfully.
        thread::sleep(Duration::from_millis(100));
        let g = sf
            .begin_or_wait_guard(key.clone())
            .unwrap()
            .expect("leader2");
        g.success();
        // Key removed after publish — next caller becomes leader again (or WaitDone
        // if a concurrent waiter saw the wake). Either is fine; not Loading.
        match sf.begin_or_wait(&key) {
            BeginLoad::Leader | BeginLoad::WaitDone => {}
            BeginLoad::Failed(m) => panic!("unexpected Failed: {m}"),
        }
    }
}
