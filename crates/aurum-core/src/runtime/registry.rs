//! Weighted model residency and eviction (JOE-1598).

use crate::error::{ProviderError, Result};
use crate::runtime::singleflight::LoadKey;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Conservative memory weight for a loaded model/session.
#[derive(Debug, Clone, Copy)]
pub struct ResidencyWeight {
    pub bytes: u64,
}

/// Configuration for the shared model registry.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    pub max_resident_bytes: u64,
    pub max_entries: usize,
    pub idle_ttl: Option<Duration>,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            max_resident_bytes: 3 * 1024 * 1024 * 1024, // 3 GiB
            max_entries: 8,
            idle_ttl: Some(Duration::from_secs(30 * 60)),
        }
    }
}

/// One resident entry.
#[derive(Debug)]
pub struct RegistryEntry<T> {
    pub key: LoadKey,
    pub value: Arc<T>,
    pub weight: ResidencyWeight,
    /// Monotonic millis of last use (for LRU).
    last_used_ms: AtomicU64,
    pub active_refs: AtomicU64,
    pub pinned: bool,
}

impl<T> RegistryEntry<T> {
    fn touch(&self) {
        self.last_used_ms.store(now_ms(), Ordering::Relaxed);
    }

    fn last_used_instant(&self) -> Instant {
        let ms = self.last_used_ms.load(Ordering::Relaxed);
        Instant::now()
            .checked_sub(Duration::from_millis(now_ms().saturating_sub(ms)))
            .unwrap_or_else(Instant::now)
    }

    pub fn last_used_ms(&self) -> u64 {
        self.last_used_ms.load(Ordering::Relaxed)
    }
}

/// Shared resident model registry with weighted LRU eviction.
pub struct ModelRegistry<T> {
    config: RegistryConfig,
    inner: Mutex<HashMap<LoadKey, Arc<RegistryEntry<T>>>>,
    total_weight: AtomicU64,
}

impl<T> ModelRegistry<T> {
    pub fn new(config: RegistryConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(HashMap::new()),
            total_weight: AtomicU64::new(0),
        }
    }

    pub fn config(&self) -> &RegistryConfig {
        &self.config
    }

    pub fn total_weight(&self) -> u64 {
        self.total_weight.load(Ordering::SeqCst)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Insert or refresh an entry. Evicts idle entries if needed.
    ///
    /// New inserts start with `active_refs == 0`. Prefer [`Self::insert_and_pin`]
    /// when the caller will immediately use the value (JOE-1646).
    pub fn insert(
        &self,
        key: LoadKey,
        value: Arc<T>,
        weight: ResidencyWeight,
    ) -> Result<Arc<RegistryEntry<T>>> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.get(&key) {
            existing.touch();
            return Ok(Arc::clone(existing));
        }
        self.evict_locked(&mut guard, weight.bytes, 1)?;
        let entry = Arc::new(RegistryEntry {
            key: key.clone(),
            value,
            weight,
            last_used_ms: AtomicU64::new(now_ms()),
            active_refs: AtomicU64::new(0),
            pinned: false,
        });
        guard.insert(key, Arc::clone(&entry));
        self.total_weight.fetch_add(weight.bytes, Ordering::SeqCst);
        Ok(entry)
    }

    /// Insert (or refresh) and return a pin under one lock — no eviction window
    /// between publication and active lease (JOE-1646).
    pub fn insert_and_pin(
        &self,
        key: LoadKey,
        value: Arc<T>,
        weight: ResidencyWeight,
    ) -> Result<RegistryPin<T>> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.get(&key) {
            existing.touch();
            existing.active_refs.fetch_add(1, Ordering::SeqCst);
            return Ok(RegistryPin {
                entry: Arc::clone(existing),
            });
        }
        self.evict_locked(&mut guard, weight.bytes, 1)?;
        let entry = Arc::new(RegistryEntry {
            key: key.clone(),
            value,
            weight,
            last_used_ms: AtomicU64::new(now_ms()),
            active_refs: AtomicU64::new(1), // already leased to caller
            pinned: false,
        });
        guard.insert(key, Arc::clone(&entry));
        self.total_weight.fetch_add(weight.bytes, Ordering::SeqCst);
        Ok(RegistryPin { entry })
    }

    pub fn get(&self, key: &LoadKey) -> Option<Arc<RegistryEntry<T>>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.get(key).map(|e| {
            e.touch();
            Arc::clone(e)
        })
    }

    /// Pin for an active operation (prevents eviction).
    ///
    /// Prefer [`Self::get_and_pin`] when the caller needs the value only while
    /// pinned (atomic lookup+lease).
    pub fn pin(&self, key: &LoadKey) -> Option<RegistryPin<T>> {
        self.get_and_pin(key)
    }

    /// Atomic lookup + pin under the registry lock (JOE-1646).
    ///
    /// There is no window where the entry can be evicted after the caller has
    /// observed it but before `active_refs` is incremented.
    pub fn get_and_pin(&self, key: &LoadKey) -> Option<RegistryPin<T>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.get(key)?;
        entry.touch();
        entry.active_refs.fetch_add(1, Ordering::SeqCst);
        Some(RegistryPin {
            entry: Arc::clone(entry),
        })
    }

    /// Drop **idle** entries only (JOE-1646 third-pass).
    ///
    /// Entries with `active_refs > 0` or `pinned` stay in the map with their
    /// weight so residency accounting remains truthful and a same-key reload
    /// cannot multiply native sessions while an operation still holds a pin.
    ///
    /// Returns `(removed, retained_active)`.
    pub fn clear_idle(&self) -> (usize, usize) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let before = guard.len();
        let mut removed_weight = 0u64;
        guard.retain(|_, e| {
            let active = e.active_refs.load(Ordering::SeqCst) > 0 || e.pinned;
            if !active {
                removed_weight = removed_weight.saturating_add(e.weight.bytes);
            }
            active
        });
        if removed_weight > 0 {
            self.total_weight
                .fetch_sub(removed_weight, Ordering::SeqCst);
        }
        let retained = guard.len();
        let removed = before.saturating_sub(retained);
        (removed, retained)
    }

    /// Clear idle entries only — never forcibly drops active leases.
    pub fn clear(&self) {
        let _ = self.clear_idle();
    }

    /// Remove a specific key if not active.
    pub fn try_unload(&self, key: &LoadKey) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = guard.get(key) {
            if e.active_refs.load(Ordering::SeqCst) > 0 || e.pinned {
                return false;
            }
            let w = e.weight.bytes;
            guard.remove(key);
            self.total_weight.fetch_sub(w, Ordering::SeqCst);
            return true;
        }
        false
    }

    fn evict_locked(
        &self,
        guard: &mut HashMap<LoadKey, Arc<RegistryEntry<T>>>,
        need_bytes: u64,
        need_slots: usize,
    ) -> Result<()> {
        // TTL pass.
        if let Some(ttl) = self.config.idle_ttl {
            let now = Instant::now();
            let expired: Vec<LoadKey> = guard
                .iter()
                .filter(|(_, e)| {
                    e.active_refs.load(Ordering::SeqCst) == 0
                        && !e.pinned
                        && now.duration_since(e.last_used_instant()) > ttl
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in expired {
                if let Some(e) = guard.remove(&k) {
                    self.total_weight
                        .fetch_sub(e.weight.bytes, Ordering::SeqCst);
                }
            }
        }

        while guard.len() + need_slots > self.config.max_entries
            || self.total_weight.load(Ordering::SeqCst) + need_bytes
                > self.config.max_resident_bytes
        {
            // Deterministic weighted LRU: idle, unpinned, oldest last_used.
            let victim = guard
                .iter()
                .filter(|(_, e)| e.active_refs.load(Ordering::SeqCst) == 0 && !e.pinned)
                .min_by_key(|(_, e)| e.last_used_ms.load(Ordering::Relaxed))
                .map(|(k, _)| k.clone());
            let Some(v) = victim else {
                if need_bytes > self.config.max_resident_bytes {
                    return Err(ProviderError::Overload {
                        reason: format!(
                            "model weight {need_bytes} exceeds residency budget {}",
                            self.config.max_resident_bytes
                        ),
                    }
                    .into());
                }
                return Err(ProviderError::Overload {
                    reason: "model residency budget exhausted (no idle entries to evict)".into(),
                }
                .into());
            };
            if let Some(e) = guard.remove(&v) {
                self.total_weight
                    .fetch_sub(e.weight.bytes, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Vec<RegistrySnapshot> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = now_ms();
        guard
            .values()
            .map(|e| {
                let last = e.last_used_ms.load(Ordering::Relaxed);
                RegistrySnapshot {
                    id: e.key.id.clone(),
                    kind: e.key.kind,
                    weight_bytes: e.weight.bytes,
                    active_refs: e.active_refs.load(Ordering::SeqCst),
                    idle_secs: now.saturating_sub(last) / 1000,
                    pinned: e.pinned,
                }
            })
            .collect()
    }
}

fn now_ms() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Diagnostic view of a resident entry.
#[derive(Debug, Clone)]
pub struct RegistrySnapshot {
    pub id: String,
    pub kind: &'static str,
    pub weight_bytes: u64,
    pub active_refs: u64,
    pub idle_secs: u64,
    pub pinned: bool,
}

/// RAII pin on a registry entry.
pub struct RegistryPin<T> {
    entry: Arc<RegistryEntry<T>>,
}

impl<T> RegistryPin<T> {
    pub fn value(&self) -> &Arc<T> {
        &self.entry.value
    }

    pub fn entry(&self) -> &RegistryEntry<T> {
        &self.entry
    }
}

impl<T> Drop for RegistryPin<T> {
    fn drop(&mut self) {
        self.entry.active_refs.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str) -> LoadKey {
        LoadKey::stt(id, format!("/tmp/{id}"))
    }

    #[test]
    fn evicts_idle_when_over_entry_cap() {
        let reg = ModelRegistry::new(RegistryConfig {
            max_entries: 2,
            max_resident_bytes: 10_000,
            idle_ttl: None,
        });
        reg.insert(key("a"), Arc::new(1u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        reg.insert(key("b"), Arc::new(2u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        assert_eq!(reg.len(), 2);
        reg.insert(key("c"), Arc::new(3u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        assert_eq!(reg.len(), 2);
        assert!(reg.get(&key("c")).is_some());
    }

    #[test]
    fn active_not_evicted() {
        let reg = ModelRegistry::new(RegistryConfig {
            max_entries: 1,
            max_resident_bytes: 10_000,
            idle_ttl: None,
        });
        reg.insert(key("a"), Arc::new(1u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        let pin = reg.pin(&key("a")).unwrap();
        let err = reg
            .insert(key("b"), Arc::new(2u32), ResidencyWeight { bytes: 100 })
            .unwrap_err();
        assert!(err.to_string().contains("residency") || err.to_string().contains("overload"));
        drop(pin);
        reg.insert(key("b"), Arc::new(2u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        assert!(reg.get(&key("b")).is_some());
    }

    #[test]
    fn lru_prefers_older() {
        let reg = ModelRegistry::new(RegistryConfig {
            max_entries: 2,
            max_resident_bytes: 10_000,
            idle_ttl: None,
        });
        reg.insert(key("a"), Arc::new(1u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        reg.insert(key("b"), Arc::new(2u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        // Touch b so a is older.
        let _ = reg.get(&key("b"));
        reg.insert(key("c"), Arc::new(3u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        assert!(reg.get(&key("a")).is_none());
        assert!(reg.get(&key("b")).is_some());
        assert!(reg.get(&key("c")).is_some());
    }

    #[test]
    fn get_and_pin_prevents_eviction() {
        let reg = ModelRegistry::new(RegistryConfig {
            max_entries: 1,
            max_resident_bytes: 10_000,
            idle_ttl: None,
        });
        let pin = reg
            .insert_and_pin(key("a"), Arc::new(1u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        assert_eq!(**pin.value(), 1);
        let err = reg
            .insert(key("b"), Arc::new(2u32), ResidencyWeight { bytes: 100 })
            .unwrap_err();
        assert!(err.to_string().contains("residency") || err.to_string().contains("overload"));
        drop(pin);
        reg.insert(key("b"), Arc::new(2u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        assert!(reg.get(&key("a")).is_none());
    }

    #[test]
    fn insert_and_pin_existing_increments_active() {
        let reg = ModelRegistry::new(RegistryConfig::default());
        reg.insert(key("a"), Arc::new(7u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        let p1 = reg
            .insert_and_pin(key("a"), Arc::new(999u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        // Existing value retained (not replaced with 999).
        assert_eq!(**p1.value(), 7);
        let p2 = reg.get_and_pin(&key("a")).unwrap();
        assert_eq!(p1.entry().active_refs.load(Ordering::SeqCst), 2);
        drop(p1);
        drop(p2);
        assert_eq!(
            reg.get(&key("a"))
                .unwrap()
                .active_refs
                .load(Ordering::SeqCst),
            0
        );
    }

    #[test]
    fn clear_idle_retains_active_and_blocks_reload_multiplication() {
        let reg = ModelRegistry::new(RegistryConfig {
            max_entries: 8,
            max_resident_bytes: 10_000,
            idle_ttl: None,
        });
        // Idle entry is removable.
        reg.insert(key("idle"), Arc::new(1u32), ResidencyWeight { bytes: 100 })
            .unwrap();
        // Active lease must survive clear.
        let lease = reg
            .insert_and_pin(
                key("active"),
                Arc::new(42u32),
                ResidencyWeight { bytes: 200 },
            )
            .unwrap();
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.total_weight(), 300);

        let (removed, retained) = reg.clear_idle();
        assert_eq!(removed, 1);
        assert_eq!(retained, 1);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.total_weight(), 200);
        assert!(reg.get(&key("idle")).is_none());
        // Same-key lookup still hits the active session (no second load).
        let again = reg.get_and_pin(&key("active")).unwrap();
        assert_eq!(**again.value(), 42);
        assert_eq!(lease.entry().active_refs.load(Ordering::SeqCst), 2);
        drop(again);
        drop(lease);
        // Now idle → clear removes it.
        let (removed, retained) = reg.clear_idle();
        assert_eq!(removed, 1);
        assert_eq!(retained, 0);
        assert!(reg.is_empty());
        assert_eq!(reg.total_weight(), 0);
    }
}
