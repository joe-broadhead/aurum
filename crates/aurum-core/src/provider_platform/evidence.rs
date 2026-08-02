//! Provider support-tier evidence, freshness, and demotion policy (JOE-2223).
//!
//! `supported` is an operational claim: reviewed factory + mocks + **fresh**
//! protected inference evidence (≤30 days). Mocks alone never promote a remote
//! route. Evidence never retains payloads, keys, or private voice IDs.

use crate::error::{Result, UserError};
use crate::provider_platform::{list_provider_summaries, ProviderRegistry, ProviderStability};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Evidence record schema version.
pub const PROVIDER_EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Maximum age (seconds) of a passing protected smoke for a `supported` route.
pub const SUPPORTED_EVIDENCE_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60; // 30 days

/// Product support tier (code/docs/CLI). Distinct from registry
/// [`ProviderStability`] which describes implementation maturity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportTier {
    /// Meets full entry rules including fresh protected evidence.
    Supported,
    /// Implemented + mocks; evidence missing/stale/limited. Never a default.
    Experimental,
    /// Requires deliberate selection; hidden from normal recommendation flows.
    ExplicitOnly,
}

impl SupportTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Experimental => "experimental",
            Self::ExplicitOnly => "explicit_only",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "supported" | "stable" => Ok(Self::Supported),
            "experimental" => Ok(Self::Experimental),
            "explicit_only" | "explicit-only" | "explicit" => Ok(Self::ExplicitOnly),
            other => Err(UserError::InvalidConfig {
                reason: format!(
                    "unknown support tier '{other}' (use supported|experimental|explicit_only)"
                ),
            }
            .into()),
        }
    }
}

/// Operation covered by an evidence record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOperation {
    Stt,
    Tts,
}

impl EvidenceOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stt => "stt",
            Self::Tts => "tts",
        }
    }
}

/// Closed failure categories (no free-form vendor bodies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceFailureCategory {
    #[default]
    None,
    Auth,
    RateLimit,
    Quota,
    Network,
    ModelUnavailable,
    ProtocolDrift,
    AccountGuardrail,
    InvalidPayload,
    Timeout,
    Other,
}

/// Machine-readable provider evidence record (redacted).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderEvidenceRecord {
    pub schema_version: u32,
    pub provider_id: String,
    pub operation: EvidenceOperation,
    pub model_id: String,
    /// Reviewed voice alias (never private ElevenLabs IDs in public evidence).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_alias: Option<String>,
    pub support_tier: SupportTier,
    /// Full git commit when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aurum_commit: Option<String>,
    pub aurum_version: String,
    /// Protocol/endpoint contract label (e.g. `openai_stt_v1`).
    pub protocol_contract: String,
    /// UTC unix seconds when the protected smoke executed.
    pub executed_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<String>,
    pub auth_ok: bool,
    pub passed: bool,
    #[serde(default)]
    pub failure_category: EvidenceFailureCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoded_bytes: Option<u64>,
    /// Non-empty result without payload (text chars or audio samples count only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_units: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate_hz: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_kind: Option<String>,
    #[serde(default)]
    pub timestamps_reliable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_snapshot_digest: Option<String>,
    /// Explicit expiry (unix). If absent, freshness uses max age from execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl ProviderEvidenceRecord {
    pub fn validate_schema(&self) -> Result<()> {
        if self.schema_version != PROVIDER_EVIDENCE_SCHEMA_VERSION {
            return Err(UserError::Other {
                message: format!(
                    "unsupported provider evidence schema_version {} (expected {PROVIDER_EVIDENCE_SCHEMA_VERSION})",
                    self.schema_version
                ),
            }
            .into());
        }
        if self.provider_id.trim().is_empty() || self.model_id.trim().is_empty() {
            return Err(UserError::Other {
                message: "provider evidence requires non-empty provider_id and model_id".into(),
            }
            .into());
        }
        if self.executed_at_unix == 0 {
            return Err(UserError::Other {
                message: "provider evidence requires executed_at_unix".into(),
            }
            .into());
        }
        // Privacy: notes must not look like secrets/payloads.
        if let Some(ref n) = self.notes {
            for bad in ["sk-", "Bearer ", "BEGIN_", "transcript=", "pcm="] {
                if n.contains(bad) {
                    return Err(UserError::Other {
                        message: format!(
                            "provider evidence notes contain forbidden fragment {bad:?}"
                        ),
                    }
                    .into());
                }
            }
        }
        Ok(())
    }

    pub fn route_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.provider_id,
            self.operation.as_str(),
            self.model_id,
            self.voice_alias.as_deref().unwrap_or("-")
        )
    }

    /// Freshness relative to `now_unix` (typically wall clock).
    pub fn is_fresh(&self, now_unix: u64) -> bool {
        if let Some(exp) = self.expires_at_unix {
            return now_unix <= exp;
        }
        now_unix.saturating_sub(self.executed_at_unix) <= SUPPORTED_EVIDENCE_MAX_AGE_SECS
    }

    /// A route may claim `supported` only with a fresh **passing** record.
    pub fn qualifies_as_supported(&self, now_unix: u64) -> bool {
        matches!(self.support_tier, SupportTier::Supported)
            && self.passed
            && self.auth_ok
            && self.is_fresh(now_unix)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read provider evidence {}: {e}", path.display()),
        })?;
        if data.len() > 256 * 1024 {
            return Err(UserError::Other {
                message: "provider evidence file exceeds 256 KiB bound".into(),
            }
            .into());
        }
        let rec: Self = serde_json::from_str(&data).map_err(|e| UserError::Other {
            message: format!("parse provider evidence: {e}"),
        })?;
        rec.validate_schema()?;
        Ok(rec)
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("serialize provider evidence: {e}"),
            }
            .into()
        })
    }
}

/// Reviewed claim that a route is product-supported (must be backed by evidence).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupportedRouteClaim {
    pub provider_id: String,
    pub operation: EvidenceOperation,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_alias: Option<String>,
    /// When true, missing/stale evidence fails the release gate.
    #[serde(default = "default_true")]
    pub required_for_release: bool,
}

fn default_true() -> bool {
    true
}

impl SupportedRouteClaim {
    pub fn route_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.provider_id,
            self.operation.as_str(),
            self.model_id,
            self.voice_alias.as_deref().unwrap_or("-")
        )
    }
}

/// Versioned index of claims + optional evidence directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderEvidenceIndex {
    pub schema_version: u32,
    pub aurum_version: String,
    /// Routes that product code/docs currently claim as `supported`.
    pub supported_claims: Vec<SupportedRouteClaim>,
    /// Routes intentionally experimental (documentation only; do not block release).
    #[serde(default)]
    pub experimental_routes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl ProviderEvidenceIndex {
    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read evidence index {}: {e}", path.display()),
        })?;
        let idx: Self = serde_json::from_str(&data).map_err(|e| UserError::Other {
            message: format!("parse evidence index: {e}"),
        })?;
        if idx.schema_version != PROVIDER_EVIDENCE_SCHEMA_VERSION {
            return Err(UserError::Other {
                message: format!(
                    "unsupported evidence index schema_version {}",
                    idx.schema_version
                ),
            }
            .into());
        }
        Ok(idx)
    }
}

/// One gate finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceGateFinding {
    pub severity: String,
    pub route: String,
    pub code: String,
    pub message: String,
}

/// Result of evaluating supported claims against evidence files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceGateReport {
    pub passed: bool,
    pub now_unix: u64,
    pub findings: Vec<EvidenceGateFinding>,
}

/// Load all `*.json` evidence records under `dir` (non-recursive).
pub fn load_evidence_dir(dir: &Path) -> Result<Vec<ProviderEvidenceRecord>> {
    if !dir.is_dir() {
        return Err(UserError::Other {
            message: format!("evidence directory missing: {}", dir.display()),
        }
        .into());
    }
    let mut out = Vec::new();
    for ent in fs::read_dir(dir).map_err(|e| UserError::Other {
        message: format!("read evidence dir: {e}"),
    })? {
        let ent = ent.map_err(|e| UserError::Other {
            message: format!("read evidence entry: {e}"),
        })?;
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("index.json") {
            continue;
        }
        out.push(ProviderEvidenceRecord::load(&path)?);
    }
    Ok(out)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Evaluate release readiness for supported remote routes.
///
/// Local routes never require protected network evidence.
pub fn evaluate_supported_evidence_gate(
    index: &ProviderEvidenceIndex,
    records: &[ProviderEvidenceRecord],
    now: Option<u64>,
) -> EvidenceGateReport {
    let now_unix = now.unwrap_or_else(now_unix);
    let mut by_route: BTreeMap<String, Vec<&ProviderEvidenceRecord>> = BTreeMap::new();
    for r in records {
        by_route.entry(r.route_key()).or_default().push(r);
    }

    let mut findings = Vec::new();
    for claim in &index.supported_claims {
        if !claim.required_for_release {
            continue;
        }
        // Local is always supported without remote smoke.
        if claim.provider_id == "local" {
            findings.push(EvidenceGateFinding {
                severity: "pass".into(),
                route: claim.route_key(),
                code: "local_supported".into(),
                message: "local route does not require protected network evidence".into(),
            });
            continue;
        }

        let key = claim.route_key();
        let Some(list) = by_route.get(&key) else {
            findings.push(EvidenceGateFinding {
                severity: "fail".into(),
                route: key,
                code: "missing_evidence".into(),
                message: "no evidence record for supported claim; demote, restore, or remove"
                    .into(),
            });
            continue;
        };

        let best = list
            .iter()
            .filter(|r| r.passed && r.auth_ok)
            .max_by_key(|r| r.executed_at_unix);
        match best {
            None => findings.push(EvidenceGateFinding {
                severity: "fail".into(),
                route: key,
                code: "no_passing_evidence".into(),
                message: "evidence exists but no passing/auth_ok record".into(),
            }),
            Some(r) if !r.is_fresh(now_unix) => findings.push(EvidenceGateFinding {
                severity: "fail".into(),
                route: key,
                code: "stale_evidence".into(),
                message: format!(
                    "latest passing evidence is older than {} days (executed_at_unix={})",
                    SUPPORTED_EVIDENCE_MAX_AGE_SECS / 86400,
                    r.executed_at_unix
                ),
            }),
            Some(r) if !matches!(r.support_tier, SupportTier::Supported) => {
                findings.push(EvidenceGateFinding {
                    severity: "fail".into(),
                    route: key,
                    code: "tier_mismatch".into(),
                    message: format!(
                        "claim is supported but evidence tier is {}",
                        r.support_tier.as_str()
                    ),
                });
            }
            Some(_) => findings.push(EvidenceGateFinding {
                severity: "pass".into(),
                route: key,
                code: "ok".into(),
                message: "fresh passing evidence present".into(),
            }),
        }
    }

    let passed = findings.iter().all(|f| f.severity != "fail");
    EvidenceGateReport {
        passed,
        now_unix,
        findings,
    }
}

/// Catalogue drift: reviewed model IDs that discovery no longer lists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogueDriftReport {
    pub provider_id: String,
    pub missing_from_discovery: Vec<String>,
    pub unexpected_in_discovery: Vec<String>,
}

/// Compare a reviewed allowlist to a discovery set (never auto-trust discovery).
pub fn detect_catalogue_drift(
    provider_id: &str,
    reviewed: &[String],
    discovered: &[String],
) -> CatalogueDriftReport {
    let rev: std::collections::BTreeSet<_> = reviewed.iter().cloned().collect();
    let disc: std::collections::BTreeSet<_> = discovered.iter().cloned().collect();
    CatalogueDriftReport {
        provider_id: provider_id.into(),
        missing_from_discovery: rev.difference(&disc).cloned().collect(),
        unexpected_in_discovery: disc.difference(&rev).cloned().collect(),
    }
}

/// Map registry stability to product support tier **before** evidence overlay.
pub fn tier_from_registry_stability(s: ProviderStability) -> SupportTier {
    match s {
        ProviderStability::Stable => SupportTier::Supported,
        ProviderStability::Experimental => SupportTier::Experimental,
        ProviderStability::TestOnly => SupportTier::ExplicitOnly,
    }
}

/// Effective tier for a provider after applying evidence (local stays supported).
pub fn effective_provider_tier(
    provider_id: &str,
    registry_stability: ProviderStability,
    evidence: &[ProviderEvidenceRecord],
    now_unix: u64,
) -> SupportTier {
    if provider_id == "local" {
        return SupportTier::Supported;
    }
    let base = tier_from_registry_stability(registry_stability);
    if !matches!(base, SupportTier::Supported) {
        return base;
    }
    // Registry says stable/supported — require at least one fresh passing evidence
    // for any STT or TTS model route, else demote to experimental for gate purposes.
    let has_fresh = evidence
        .iter()
        .any(|r| r.provider_id == provider_id && r.qualifies_as_supported(now_unix));
    if has_fresh {
        SupportTier::Supported
    } else {
        SupportTier::Experimental
    }
}

/// Summarize builtin registry + evidence for documentation/release.
pub fn provider_tier_matrix(
    registry: &ProviderRegistry,
    evidence: &[ProviderEvidenceRecord],
    now_unix: u64,
) -> Vec<(String, SupportTier, bool, bool)> {
    list_provider_summaries(registry)
        .into_iter()
        .map(|s| {
            let tier = effective_provider_tier(&s.id, s.stability, evidence, now_unix);
            (s.id, tier, s.stt, s.tts)
        })
        .collect()
}

/// Built-in local evidence record (always valid for CI / offline).
pub fn local_stt_evidence(now_unix: u64) -> ProviderEvidenceRecord {
    ProviderEvidenceRecord {
        schema_version: PROVIDER_EVIDENCE_SCHEMA_VERSION,
        provider_id: "local".into(),
        operation: EvidenceOperation::Stt,
        model_id: "base".into(),
        voice_alias: None,
        support_tier: SupportTier::Supported,
        aurum_commit: None,
        aurum_version: env!("CARGO_PKG_VERSION").into(),
        protocol_contract: "local_whisper_v1".into(),
        executed_at_unix: now_unix,
        workflow_run_id: Some("offline-ci".into()),
        auth_ok: true,
        passed: true,
        failure_category: EvidenceFailureCategory::None,
        latency_ms: None,
        encoded_bytes: None,
        decoded_bytes: None,
        result_units: Some(1),
        sample_rate_hz: Some(16_000),
        backend_kind: Some("asr".into()),
        timestamps_reliable: true,
        capability_snapshot_digest: None,
        expires_at_unix: Some(now_unix + SUPPORTED_EVIDENCE_MAX_AGE_SECS),
        notes: Some("local STT — no network evidence required".into()),
    }
}

pub fn local_tts_evidence(now_unix: u64) -> ProviderEvidenceRecord {
    ProviderEvidenceRecord {
        schema_version: PROVIDER_EVIDENCE_SCHEMA_VERSION,
        provider_id: "local".into(),
        operation: EvidenceOperation::Tts,
        model_id: "kitten-nano-int8".into(),
        voice_alias: Some("Luna".into()),
        support_tier: SupportTier::Supported,
        aurum_commit: None,
        aurum_version: env!("CARGO_PKG_VERSION").into(),
        protocol_contract: "local_kitten_v1".into(),
        executed_at_unix: now_unix,
        workflow_run_id: Some("offline-ci".into()),
        auth_ok: true,
        passed: true,
        failure_category: EvidenceFailureCategory::None,
        latency_ms: None,
        encoded_bytes: None,
        decoded_bytes: None,
        result_units: Some(1),
        sample_rate_hz: Some(24_000),
        backend_kind: Some("local".into()),
        timestamps_reliable: false,
        capability_snapshot_digest: None,
        expires_at_unix: Some(now_unix + SUPPORTED_EVIDENCE_MAX_AGE_SECS),
        notes: Some("local TTS — no network evidence required".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(provider: &str, op: EvidenceOperation, model: &str) -> SupportedRouteClaim {
        SupportedRouteClaim {
            provider_id: provider.into(),
            operation: op,
            model_id: model.into(),
            voice_alias: None,
            required_for_release: true,
        }
    }

    #[test]
    fn local_gate_passes_without_files() {
        let idx = ProviderEvidenceIndex {
            schema_version: 1,
            aurum_version: "0.0.22".into(),
            supported_claims: vec![claim("local", EvidenceOperation::Stt, "base")],
            experimental_routes: vec![],
            notes: None,
        };
        let rep = evaluate_supported_evidence_gate(&idx, &[], None);
        assert!(rep.passed, "{:?}", rep.findings);
    }

    #[test]
    fn missing_remote_evidence_fails() {
        let idx = ProviderEvidenceIndex {
            schema_version: 1,
            aurum_version: "0.0.22".into(),
            supported_claims: vec![claim("openai", EvidenceOperation::Stt, "whisper-1")],
            experimental_routes: vec![],
            notes: None,
        };
        let rep = evaluate_supported_evidence_gate(&idx, &[], Some(1_700_000_000));
        assert!(!rep.passed);
        assert!(rep.findings.iter().any(|f| f.code == "missing_evidence"));
    }

    #[test]
    fn stale_evidence_fails() {
        let now = 2_000_000_000u64;
        let mut rec = local_stt_evidence(now);
        rec.provider_id = "openai".into();
        rec.model_id = "whisper-1".into();
        rec.protocol_contract = "openai_stt_v1".into();
        rec.executed_at_unix = now - SUPPORTED_EVIDENCE_MAX_AGE_SECS - 10;
        rec.expires_at_unix = None;
        let idx = ProviderEvidenceIndex {
            schema_version: 1,
            aurum_version: "0.0.22".into(),
            supported_claims: vec![claim("openai", EvidenceOperation::Stt, "whisper-1")],
            experimental_routes: vec![],
            notes: None,
        };
        let rep = evaluate_supported_evidence_gate(&idx, &[rec], Some(now));
        assert!(!rep.passed);
        assert!(rep.findings.iter().any(|f| f.code == "stale_evidence"));
    }

    #[test]
    fn fresh_passing_remote_ok() {
        let now = 2_000_000_000u64;
        let mut rec = local_stt_evidence(now);
        rec.provider_id = "openai".into();
        rec.model_id = "whisper-1".into();
        rec.protocol_contract = "openai_stt_v1".into();
        let idx = ProviderEvidenceIndex {
            schema_version: 1,
            aurum_version: "0.0.22".into(),
            supported_claims: vec![claim("openai", EvidenceOperation::Stt, "whisper-1")],
            experimental_routes: vec![],
            notes: None,
        };
        let rep = evaluate_supported_evidence_gate(&idx, &[rec], Some(now));
        assert!(rep.passed, "{:?}", rep.findings);
    }

    #[test]
    fn catalogue_drift_detects_removed_model() {
        let d = detect_catalogue_drift(
            "openai",
            &["whisper-1".into(), "gone-model".into()],
            &["whisper-1".into(), "new-model".into()],
        );
        assert_eq!(d.missing_from_discovery, vec!["gone-model".to_string()]);
        assert_eq!(d.unexpected_in_discovery, vec!["new-model".to_string()]);
    }

    #[test]
    fn effective_tier_demotes_without_evidence() {
        let now = 2_000_000_000u64;
        let t = effective_provider_tier("openai", ProviderStability::Stable, &[], now);
        assert_eq!(t, SupportTier::Experimental);
        let rec = {
            let mut r = local_stt_evidence(now);
            r.provider_id = "openai".into();
            r.model_id = "whisper-1".into();
            r
        };
        let t2 = effective_provider_tier("openai", ProviderStability::Stable, &[rec], now);
        assert_eq!(t2, SupportTier::Supported);
    }

    #[test]
    fn privacy_rejects_secret_notes() {
        let mut r = local_stt_evidence(1);
        r.notes = Some("sk-abc".into());
        assert!(r.validate_schema().is_err());
    }
}
