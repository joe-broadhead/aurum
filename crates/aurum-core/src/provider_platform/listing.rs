//! Lightweight provider listing for future `aurum providers` (JOE-1936).
//!
//! Secret-free, offline, registry-derived summaries. No credentials and no
//! network. Full CLI command is intentionally deferred.

use super::descriptor::{NetworkRequirement, ProviderDescriptor, ProviderStability};
use super::registry::ProviderRegistry;
use serde::{Deserialize, Serialize};

/// Schema version for provider list JSON (discovery DTO).
pub const PROVIDER_LIST_SCHEMA_VERSION: u32 = 1;

/// One compiled provider as seen through the registry (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderSummary {
    pub id: String,
    pub display_name: String,
    pub stt: bool,
    pub tts: bool,
    pub network: NetworkRequirement,
    pub stability: ProviderStability,
}

impl ProviderSummary {
    pub fn from_descriptor(d: &ProviderDescriptor) -> Self {
        Self {
            id: d.id.as_str().to_string(),
            display_name: d.display_name.to_string(),
            stt: d.operations.supports_stt(),
            tts: d.operations.supports_tts(),
            network: d.network,
            stability: d.stability,
        }
    }
}

/// Versioned list payload for JSON discovery (`aurum providers --json` later).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderList {
    pub schema_version: u32,
    pub providers: Vec<ProviderSummary>,
}

/// Enumerate compiled providers from a registry (registration order, offline).
pub fn list_provider_summaries(registry: &ProviderRegistry) -> Vec<ProviderSummary> {
    merge_provider_summaries(registry)
}

/// Merge STT/TTS factories that share a provider id into one summary row.
pub fn merge_provider_summaries(registry: &ProviderRegistry) -> Vec<ProviderSummary> {
    use std::collections::BTreeMap;

    let mut by_id: BTreeMap<String, ProviderSummary> = BTreeMap::new();

    for id in registry.list_stt_ids() {
        if let Ok(f) = registry.stt_factory(&id) {
            let d = f.descriptor();
            let entry = by_id
                .entry(id.as_str().to_string())
                .or_insert_with(|| ProviderSummary {
                    id: d.id.as_str().to_string(),
                    display_name: d.display_name.to_string(),
                    stt: false,
                    tts: false,
                    network: d.network,
                    stability: d.stability,
                });
            entry.stt = true;
            if matches!(d.network, NetworkRequirement::RequiresNetwork) {
                entry.network = NetworkRequirement::RequiresNetwork;
            }
        }
    }

    #[cfg(feature = "tts")]
    {
        for id in registry.list_tts_ids() {
            if let Ok(f) = registry.tts_factory(&id) {
                let d = f.descriptor();
                let entry =
                    by_id
                        .entry(id.as_str().to_string())
                        .or_insert_with(|| ProviderSummary {
                            id: d.id.as_str().to_string(),
                            display_name: d.display_name.to_string(),
                            stt: false,
                            tts: false,
                            network: d.network,
                            stability: d.stability,
                        });
                entry.tts = true;
                if matches!(d.network, NetworkRequirement::RequiresNetwork) {
                    entry.network = NetworkRequirement::RequiresNetwork;
                }
                if !entry.stt {
                    entry.display_name = d.display_name.to_string();
                }
            }
        }
    }

    let mut ordered = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for d in registry.descriptors() {
        let key = d.id.as_str();
        if seen.insert(key.to_string()) {
            if let Some(s) = by_id.remove(key) {
                ordered.push(s);
            }
        }
    }
    for (_, s) in by_id {
        ordered.push(s);
    }
    ordered
}

/// Versioned list for future CLI JSON output.
pub fn provider_list(registry: &ProviderRegistry) -> ProviderList {
    ProviderList {
        schema_version: PROVIDER_LIST_SCHEMA_VERSION,
        providers: list_provider_summaries(registry),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_is_offline_and_secret_free() {
        let reg = ProviderRegistry::builtin().unwrap();
        let list = provider_list(&reg);
        assert_eq!(list.schema_version, PROVIDER_LIST_SCHEMA_VERSION);
        let json = serde_json::to_string(&list).unwrap();
        assert!(!json.contains("sk-"));
        assert!(!json.contains("api_key"));

        let ids: Vec<_> = list.providers.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"local"));
        assert!(ids.contains(&"openrouter"));

        let local = list.providers.iter().find(|p| p.id == "local").unwrap();
        assert!(local.stt);
        #[cfg(feature = "tts")]
        assert!(local.tts);
        assert_eq!(local.network, NetworkRequirement::LocalOnly);

        let or = list
            .providers
            .iter()
            .find(|p| p.id == "openrouter")
            .unwrap();
        assert!(or.stt);
        assert!(!or.tts);
        assert_eq!(or.network, NetworkRequirement::RequiresNetwork);
    }
}
