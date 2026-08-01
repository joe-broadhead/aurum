//! Immutable-after-build provider registry (JOE-1933).

use super::descriptor::ProviderDescriptor;
use super::factory::TranscriptionProviderFactory;
use super::id::ProviderId;
use crate::error::{Result, UserError};
use std::collections::BTreeMap;
use std::sync::Arc;

#[cfg(feature = "tts")]
use super::factory::SynthesisProviderFactory;

/// Engine-owned registry of compiled provider factories.
///
/// Built via [`ProviderRegistryBuilder`], then frozen. No unsynchronized global
/// mutable plugin table.
#[derive(Clone)]
pub struct ProviderRegistry {
    stt: BTreeMap<String, Arc<dyn TranscriptionProviderFactory>>,
    #[cfg(feature = "tts")]
    tts: BTreeMap<String, Arc<dyn SynthesisProviderFactory>>,
    /// Deterministic enumeration order (registration order).
    order: Vec<ProviderId>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("ProviderRegistry");
        d.field("stt", &self.stt.keys().collect::<Vec<_>>());
        #[cfg(feature = "tts")]
        d.field("tts", &self.tts.keys().collect::<Vec<_>>());
        d.field("order", &self.order).finish()
    }
}

impl ProviderRegistry {
    pub fn builder() -> ProviderRegistryBuilder {
        ProviderRegistryBuilder::default()
    }

    /// Built-in product registry (local STT/TTS + OpenRouter STT).
    pub fn builtin() -> Result<Self> {
        super::builtin::build_builtin_registry()
    }

    pub fn stt_factory(&self, id: &ProviderId) -> Result<&dyn TranscriptionProviderFactory> {
        self.stt
            .get(id.as_str())
            .map(|f| f.as_ref())
            .ok_or_else(|| unknown_or_unsupported(id, "stt"))
    }

    #[cfg(feature = "tts")]
    pub fn tts_factory(&self, id: &ProviderId) -> Result<&dyn SynthesisProviderFactory> {
        self.tts
            .get(id.as_str())
            .map(|f| f.as_ref())
            .ok_or_else(|| unknown_or_unsupported(id, "tts"))
    }

    /// Descriptors in registration order (one entry per registered id).
    pub fn descriptors(&self) -> Vec<&ProviderDescriptor> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in &self.order {
            if !seen.insert(id.as_str()) {
                continue;
            }
            if let Some(f) = self.stt.get(id.as_str()) {
                out.push(f.descriptor());
                continue;
            }
            #[cfg(feature = "tts")]
            if let Some(f) = self.tts.get(id.as_str()) {
                out.push(f.descriptor());
            }
        }
        out
    }

    pub fn list_stt_ids(&self) -> Vec<ProviderId> {
        self.stt.keys().map(|k| ProviderId::must(k)).collect()
    }

    #[cfg(feature = "tts")]
    pub fn list_tts_ids(&self) -> Vec<ProviderId> {
        self.tts.keys().map(|k| ProviderId::must(k)).collect()
    }

    pub fn known_provider_hint(&self) -> String {
        let mut ids: Vec<&str> = self.order.iter().map(|p| p.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.join(", ")
    }
}

fn unknown_or_unsupported(id: &ProviderId, op: &str) -> crate::error::TranscriptionError {
    UserError::InvalidProvider {
        provider: format!(
            "{id} (no {op} factory registered; known: use ProviderRegistry::descriptors)"
        ),
    }
    .into()
}

/// Builder that rejects duplicate operation identities.
#[derive(Default)]
pub struct ProviderRegistryBuilder {
    // maps hold trait objects — Debug is manual if needed
    stt: BTreeMap<String, Arc<dyn TranscriptionProviderFactory>>,
    #[cfg(feature = "tts")]
    tts: BTreeMap<String, Arc<dyn SynthesisProviderFactory>>,
    order: Vec<ProviderId>,
}

impl ProviderRegistryBuilder {
    pub fn register_stt(mut self, factory: Arc<dyn TranscriptionProviderFactory>) -> Result<Self> {
        let desc = factory.descriptor();
        if !desc.operations.supports_stt() {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "factory '{}' cannot register as STT: operations.stt is false",
                    desc.id
                ),
            }
            .into());
        }
        let key = desc.id.as_str().to_string();
        if self.stt.contains_key(&key) {
            return Err(UserError::InvalidConfig {
                reason: format!("duplicate STT provider registration: {key}"),
            }
            .into());
        }
        if !self.order.iter().any(|p| p.as_str() == key) {
            self.order.push(desc.id.clone());
        }
        self.stt.insert(key, factory);
        Ok(self)
    }

    #[cfg(feature = "tts")]
    pub fn register_tts(mut self, factory: Arc<dyn SynthesisProviderFactory>) -> Result<Self> {
        let desc = factory.descriptor();
        if !desc.operations.supports_tts() {
            return Err(UserError::InvalidConfig {
                reason: format!(
                    "factory '{}' cannot register as TTS: operations.tts is false",
                    desc.id
                ),
            }
            .into());
        }
        let key = desc.id.as_str().to_string();
        if self.tts.contains_key(&key) {
            return Err(UserError::InvalidConfig {
                reason: format!("duplicate TTS provider registration: {key}"),
            }
            .into());
        }
        if !self.order.iter().any(|p| p.as_str() == key) {
            self.order.push(desc.id.clone());
        }
        self.tts.insert(key, factory);
        Ok(self)
    }

    pub fn build(self) -> ProviderRegistry {
        ProviderRegistry {
            stt: self.stt,
            #[cfg(feature = "tts")]
            tts: self.tts,
            order: self.order,
        }
    }
}
