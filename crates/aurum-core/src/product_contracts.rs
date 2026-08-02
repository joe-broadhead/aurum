//! Authoritative product metadata snapshot for CLI/docs generation (JOE-2224).
//!
//! Offline, deterministic, secret-free. Built only from compile-time reviewed
//! registries and crate version metadata — never from live vendor probes.

use crate::error::{Result, UserError};
use crate::provider_platform::{
    list_provider_summaries, NetworkRequirement, ProviderRegistry, ProviderStability,
    ProviderSummary,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Product contracts schema version.
pub const PRODUCT_CONTRACTS_SCHEMA_VERSION: u32 = 1;

/// Auth environment variable names (never values).
fn auth_env_for(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "openai" => Some("OPENAI_API_KEY"),
        "elevenlabs" => Some("ELEVENLABS_API_KEY"),
        "xai" => Some("XAI_API_KEY"),
        "local" => None,
        _ => None,
    }
}

/// Reviewed default model id when known (single source for docs/CLI hints).
fn default_stt_model(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "local" => Some("base"),
        "openrouter" => Some("google/gemini-2.5-flash"),
        "openai" => Some("whisper-1"),
        "xai" => Some("xai-stt"),
        _ => None,
    }
}

fn default_tts_model(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "local" => Some("kitten-nano-int8"),
        "openrouter" => Some("hexgrad/kokoro-82m"),
        "openai" => Some("tts-1"),
        "elevenlabs" => Some("eleven_multilingual_v2"),
        "xai" => Some("xai-tts"),
        _ => None,
    }
}

/// One provider row in the product snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductProviderRecord {
    pub id: String,
    pub display_name: String,
    pub stt: bool,
    pub tts: bool,
    pub network: NetworkRequirement,
    pub stability: ProviderStability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_stt_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_tts_model: Option<String>,
    /// FFI never exposes remote execution in v0.0.x.
    pub ffi_remote: bool,
    pub local_only_ok: bool,
}

impl ProductProviderRecord {
    pub fn from_summary(s: &ProviderSummary) -> Self {
        let local_only_ok = matches!(s.network, NetworkRequirement::LocalOnly);
        Self {
            id: s.id.clone(),
            display_name: s.display_name.clone(),
            stt: s.stt,
            tts: s.tts,
            network: s.network,
            stability: s.stability,
            auth_env: auth_env_for(&s.id).map(str::to_string),
            default_stt_model: if s.stt {
                default_stt_model(&s.id).map(str::to_string)
            } else {
                None
            },
            default_tts_model: if s.tts {
                default_tts_model(&s.id).map(str::to_string)
            } else {
                None
            },
            ffi_remote: false,
            local_only_ok,
        }
    }
}

/// Deterministic product metadata snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductContractsSnapshot {
    pub schema_version: u32,
    pub aurum_version: String,
    pub providers: Vec<ProductProviderRecord>,
    /// STT-capable provider ids (sorted for stability beyond registry order).
    pub stt_provider_ids: Vec<String>,
    /// TTS-capable provider ids.
    pub tts_provider_ids: Vec<String>,
}

impl ProductContractsSnapshot {
    /// Build from a registry (typically [`ProviderRegistry::builtin`]).
    pub fn from_registry(registry: &ProviderRegistry) -> Self {
        let summaries = list_provider_summaries(registry);
        let providers: Vec<_> = summaries
            .iter()
            .map(ProductProviderRecord::from_summary)
            .collect();
        let mut stt_provider_ids: Vec<String> = providers
            .iter()
            .filter(|p| p.stt)
            .map(|p| p.id.clone())
            .collect();
        let mut tts_provider_ids: Vec<String> = providers
            .iter()
            .filter(|p| p.tts)
            .map(|p| p.id.clone())
            .collect();
        // Keep registration order in `providers`; sort id lists for stable CLI help.
        stt_provider_ids.sort();
        tts_provider_ids.sort();
        Self {
            schema_version: PRODUCT_CONTRACTS_SCHEMA_VERSION,
            aurum_version: env!("CARGO_PKG_VERSION").into(),
            providers,
            stt_provider_ids,
            tts_provider_ids,
        }
    }

    pub fn builtin() -> Result<Self> {
        let reg = ProviderRegistry::builtin().map_err(|e| UserError::Other {
            message: format!("builtin registry for product contracts: {e}"),
        })?;
        Ok(Self::from_registry(&reg))
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("serialize product contracts: {e}"),
            }
            .into()
        })
    }

    /// Markdown provider matrix fragment (generated).
    pub fn to_markdown_matrix(&self) -> String {
        let mut out = String::new();
        out.push_str("<!-- GENERATED by aurum-core product_contracts — do not edit by hand -->\n");
        out.push_str(&format!(
            "<!-- schema_version={} aurum_version={} -->\n\n",
            self.schema_version, self.aurum_version
        ));
        out.push_str("# Provider matrix (generated)\n\n");
        out.push_str(
            "Snapshot of compiled builtin registry capabilities. Defaults are **local** for STT and TTS. Remote rows require deliberate selection and a key.\n\n",
        );
        out.push_str(
            "| Provider | STT | TTS | Auth env | Default STT model | Default TTS model | Local-only OK | FFI remote |\n",
        );
        out.push_str(
            "|----------|-----|-----|----------|-------------------|-------------------|---------------|------------|\n",
        );
        for p in &self.providers {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
                p.id,
                if p.stt { "yes" } else { "—" },
                if p.tts { "yes" } else { "—" },
                p.auth_env
                    .as_deref()
                    .map(|e| format!("`{e}`"))
                    .unwrap_or_else(|| "—".into()),
                p.default_stt_model
                    .as_deref()
                    .map(|m| format!("`{m}`"))
                    .unwrap_or_else(|| "—".into()),
                p.default_tts_model
                    .as_deref()
                    .map(|m| format!("`{m}`"))
                    .unwrap_or_else(|| "—".into()),
                if p.local_only_ok { "yes" } else { "no" },
                if p.ffi_remote { "yes" } else { "no" },
            ));
        }
        out.push_str(
            "\nFFI remote execution remains **false** until a separate approved design.\n",
        );
        out.push_str(&format!(
            "\nSTT providers: {}  \nTTS providers: {}\n",
            self.stt_provider_ids
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", "),
            self.tts_provider_ids
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
        out
    }

    /// Comma-separated STT provider ids for help text.
    pub fn stt_provider_help(&self) -> String {
        self.stt_provider_ids.join("|")
    }

    pub fn tts_provider_help(&self) -> String {
        self.tts_provider_ids.join("|")
    }

    /// Validate invariants (no secrets, FFI remote false, unique ids).
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONTRACTS_SCHEMA_VERSION {
            return Err(UserError::Other {
                message: format!(
                    "product contracts schema_version {} != {PRODUCT_CONTRACTS_SCHEMA_VERSION}",
                    self.schema_version
                ),
            }
            .into());
        }
        let mut seen = BTreeMap::new();
        for p in &self.providers {
            if seen.insert(p.id.clone(), ()).is_some() {
                return Err(UserError::Other {
                    message: format!("duplicate provider id {}", p.id),
                }
                .into());
            }
            if p.ffi_remote {
                return Err(UserError::Other {
                    message: format!("provider {} claims ffi_remote=true (forbidden)", p.id),
                }
                .into());
            }
            if let Some(ref env) = p.auth_env {
                if env.contains("sk-") || env.len() > 64 {
                    return Err(UserError::Other {
                        message: "auth_env looks like a secret value".into(),
                    }
                    .into());
                }
            }
        }
        let json = self.to_json_pretty()?;
        for forbidden in ["sk-", "api_key\":", "Bearer ", "/Users/"] {
            if json.contains(forbidden) {
                return Err(UserError::Other {
                    message: format!(
                        "product contracts JSON contains forbidden fragment {forbidden:?}"
                    ),
                }
                .into());
            }
        }
        Ok(())
    }
}

/// STT provider ids accepted by CLI/batch (registry-derived, not hard-coded).
pub fn registered_stt_provider_ids() -> Result<Vec<String>> {
    Ok(ProductContractsSnapshot::builtin()?.stt_provider_ids)
}

/// TTS provider ids accepted by CLI (registry-derived).
pub fn registered_tts_provider_ids() -> Result<Vec<String>> {
    Ok(ProductContractsSnapshot::builtin()?.tts_provider_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_deterministic_and_secret_free() {
        let a = ProductContractsSnapshot::builtin().unwrap();
        let b = ProductContractsSnapshot::builtin().unwrap();
        assert_eq!(a.to_json_pretty().unwrap(), b.to_json_pretty().unwrap());
        a.validate().unwrap();
        assert!(a.stt_provider_ids.contains(&"local".into()));
        assert!(a.providers.iter().all(|p| !p.ffi_remote));
        let md = a.to_markdown_matrix();
        assert!(md.contains("GENERATED"));
        assert!(md.contains("`local`"));
    }

    #[test]
    fn stt_ids_sorted_unique() {
        let s = ProductContractsSnapshot::builtin().unwrap();
        let mut sorted = s.stt_provider_ids.clone();
        sorted.sort();
        assert_eq!(s.stt_provider_ids, sorted);
        let mut set = std::collections::BTreeSet::new();
        for id in &s.stt_provider_ids {
            assert!(set.insert(id));
        }
    }
}
