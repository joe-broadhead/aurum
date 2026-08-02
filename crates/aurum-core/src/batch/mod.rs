//! Bounded, content-addressed, transactional multi-file batch (JOE-1726 / JOE-2220).
//!
//! Manifest v2 uses full source/output SHA-256, a canonical operation fingerprint,
//! `OutputTransaction` publishes, single-writer locking, and an explicit resume
//! decision table. Partial digests are never named `sha256` and never authorize reuse.

use crate::error::{Result, UserError};
use crate::output::{CommitMode, OutputFormat, OutputTransaction};
use crate::provider_platform::{ProviderId, ProviderRegistry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Manifest schema version (v2 is authoritative).
pub const BATCH_MANIFEST_VERSION: u32 = 2;

/// Legacy schema version (never silently trusted as v2).
pub const BATCH_MANIFEST_VERSION_V1: u32 = 1;

/// Default manifest filename written into the batch output directory.
pub const BATCH_MANIFEST_NAME: &str = "aurum-batch-manifest.json";

/// Single-writer lock file name (PID/run metadata only).
pub const BATCH_LOCK_NAME: &str = "aurum-batch.lock";

/// Max error message length stored in the manifest.
pub const MAX_BATCH_ERROR_CHARS: usize = 512;

/// Extensions treated as transcription inputs (lowercase, no dot).
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "m4a", "flac", "ogg", "oga", "opus", "webm", "aac", "mp4", "mpeg", "mpga",
];

// ---------------------------------------------------------------------------
// Status & items
// ---------------------------------------------------------------------------

/// Per-item status in the batch manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    /// Source bytes changed since success.
    StaleSource,
    /// Operation fingerprint no longer matches.
    StaleConfiguration,
    /// Output missing, wrong size, or digest mismatch.
    StaleOutput,
    /// Prior process died while status was Running.
    Interrupted,
}

impl BatchItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::StaleSource => "stale_source",
            Self::StaleConfiguration => "stale_configuration",
            Self::StaleOutput => "stale_output",
            Self::Interrupted => "interrupted",
        }
    }
}

/// One file in a batch (v2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchItem {
    /// Stable id (sha256 of normalized source path, first 16 hex chars).
    pub id: String,
    /// Source path as provided / discovered (UTF-8 lossy display form).
    pub source: String,
    /// Deterministic relative output path under `output_dir`.
    pub output: String,
    pub status: BatchItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub attempts: u32,
    /// Full SHA-256 of complete source bytes (hex). Never a partial digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_unix: Option<u64>,
}

// ---------------------------------------------------------------------------
// Operation fingerprint
// ---------------------------------------------------------------------------

/// Canonical structure of every option that can affect transcript output.
///
/// Serialized with sorted keys via serde_json::Value object insertion order
/// stability: we hash a deterministic JSON string built from fixed field order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationFingerprintInput {
    pub provider_id: String,
    pub backend_route: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_evidence: Option<String>,
    pub language: String,
    pub timestamps: bool,
    pub allow_unreliable_timestamps: bool,
    pub output_format: String,
    pub cleanup_style: String,
    pub cleanup_provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_model: Option<String>,
    pub cleanup_segments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_form_policy: Option<String>,
    pub dto_schema_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_evidence_version: Option<String>,
    pub local_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_mode: Option<String>,
    pub aurum_behavior_version: String,
}

/// SHA-256 hex of the canonical fingerprint JSON (fixed field order).
pub fn operation_fingerprint(input: &OperationFingerprintInput) -> String {
    // Fixed key order — not serde_json pretty-print order dependence.
    let payload = format!(
        concat!(
            "{{\n",
            "  \"allow_unreliable_timestamps\": {},\n",
            "  \"aurum_behavior_version\": {},\n",
            "  \"backend_route\": {},\n",
            "  \"cleanup_model\": {},\n",
            "  \"cleanup_provider\": {},\n",
            "  \"cleanup_segments\": {},\n",
            "  \"cleanup_style\": {},\n",
            "  \"dto_schema_version\": {},\n",
            "  \"language\": {},\n",
            "  \"local_only\": {},\n",
            "  \"long_form_policy\": {},\n",
            "  \"model_id\": {},\n",
            "  \"output_format\": {},\n",
            "  \"profile\": {},\n",
            "  \"profile_evidence_version\": {},\n",
            "  \"provider_id\": {},\n",
            "  \"support_evidence\": {},\n",
            "  \"timestamps\": {},\n",
            "  \"trust_mode\": {}\n",
            "}}"
        ),
        input.allow_unreliable_timestamps,
        json_str(&input.aurum_behavior_version),
        json_str(&input.backend_route),
        json_opt_str(&input.cleanup_model),
        json_str(&input.cleanup_provider),
        json_str(&input.cleanup_segments),
        json_str(&input.cleanup_style),
        json_str(&input.dto_schema_version),
        json_str(&input.language),
        input.local_only,
        json_opt_str(&input.long_form_policy),
        json_str(&input.model_id),
        json_str(&input.output_format),
        json_opt_str(&input.profile),
        json_opt_str(&input.profile_evidence_version),
        json_str(&input.provider_id),
        json_opt_str(&input.support_evidence),
        input.timestamps,
        json_opt_str(&input.trust_mode),
    );
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn json_opt_str(s: &Option<String>) -> String {
    match s {
        Some(v) => json_str(v),
        None => "null".into(),
    }
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// Versioned batch manifest (machine-readable resume state).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchManifest {
    pub schema_version: u32,
    pub aurum_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// UUID-like run id generated once per new manifest.
    pub run_id: String,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub provider: String,
    pub model: String,
    pub language: String,
    pub output_format: String,
    /// Absolute or original path string for the output directory.
    pub output_dir: String,
    /// Canonical operation fingerprint for this run.
    pub operation_fingerprint: String,
    /// Optional profile used for model selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub items: Vec<BatchItem>,
}

impl BatchManifest {
    pub fn new(
        provider: &str,
        model: &str,
        language: &str,
        format: OutputFormat,
        output_dir: &Path,
        profile: Option<&str>,
        operation_fingerprint: &str,
    ) -> Self {
        let now = unix_now();
        Self {
            schema_version: BATCH_MANIFEST_VERSION,
            aurum_version: env!("CARGO_PKG_VERSION").into(),
            commit: std::env::var("GITHUB_SHA")
                .or_else(|_| std::env::var("AURUM_COMMIT"))
                .ok(),
            run_id: new_run_id(),
            created_at_unix: now,
            updated_at_unix: now,
            provider: provider.into(),
            model: model.into(),
            language: language.into(),
            output_format: format.as_str().into(),
            output_dir: output_dir.display().to_string(),
            operation_fingerprint: operation_fingerprint.into(),
            profile: profile.map(|s| s.to_string()),
            items: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at_unix = unix_now();
    }

    pub fn summary(&self) -> BatchSummary {
        let mut s = BatchSummary::default();
        for i in &self.items {
            s.total += 1;
            match i.status {
                BatchItemStatus::Pending => s.pending += 1,
                BatchItemStatus::Running => s.running += 1,
                BatchItemStatus::Succeeded => s.succeeded += 1,
                BatchItemStatus::Failed => s.failed += 1,
                BatchItemStatus::Skipped => s.skipped += 1,
                BatchItemStatus::StaleSource => s.stale_source += 1,
                BatchItemStatus::StaleConfiguration => s.stale_configuration += 1,
                BatchItemStatus::StaleOutput => s.stale_output += 1,
                BatchItemStatus::Interrupted => s.interrupted += 1,
            }
        }
        s
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            UserError::Other {
                message: format!("batch manifest json: {e}"),
            }
            .into()
        })
    }

    /// Persist via [`OutputTransaction`] in replace mode (symlink-safe).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| UserError::Other {
                message: format!("create batch output dir {}: {e}", parent.display()),
            })?;
        }
        reject_symlink(path)?;
        let json = self.to_json_pretty()?;
        OutputTransaction::new(path, CommitMode::Replace).commit_bytes(json.as_bytes())
    }

    pub fn load(path: &Path) -> Result<Self> {
        reject_symlink(path)?;
        let meta = fs::metadata(path).map_err(|e| UserError::Other {
            message: format!("stat batch manifest {}: {e}", path.display()),
        })?;
        if !meta.is_file() {
            return Err(UserError::Other {
                message: format!("batch manifest {} is not a regular file", path.display()),
            }
            .into());
        }
        // Bound size (~32 MiB).
        if meta.len() > 32 * 1024 * 1024 {
            return Err(UserError::Other {
                message: format!(
                    "batch manifest {} exceeds 32 MiB size bound",
                    path.display()
                ),
            }
            .into());
        }
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read batch manifest {}: {e}", path.display()),
        })?;
        // Detect v1 before full parse into v2.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
            if let Some(ver) = v.get("schema_version").and_then(|x| x.as_u64()) {
                if ver == BATCH_MANIFEST_VERSION_V1 as u64 {
                    return Err(UserError::Other {
                        message: format!(
                            "batch manifest at {} is schema v1 and cannot be silently trusted as v2.\n  \
                             Hint: run with a fresh --output-dir, or use --upgrade-manifest after \
                             recomputing full source/output digests (never reuse v1 partial fingerprints).",
                            path.display()
                        ),
                    }
                    .into());
                }
            }
        }
        let m: Self = serde_json::from_str(&data).map_err(|e| UserError::Other {
            message: format!("parse batch manifest: {e}"),
        })?;
        if m.schema_version != BATCH_MANIFEST_VERSION {
            return Err(UserError::Other {
                message: format!(
                    "unsupported batch manifest schema_version {} (expected {BATCH_MANIFEST_VERSION})",
                    m.schema_version
                ),
            }
            .into());
        }
        if m.items.len() > 10_000 {
            return Err(UserError::Other {
                message: format!("batch manifest has {} items (max 10000)", m.items.len()),
            }
            .into());
        }
        Ok(m)
    }
}

/// Aggregate counters for partial-success reporting.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchSummary {
    pub total: u32,
    pub pending: u32,
    pub running: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub skipped: u32,
    #[serde(default)]
    pub stale_source: u32,
    #[serde(default)]
    pub stale_configuration: u32,
    #[serde(default)]
    pub stale_output: u32,
    #[serde(default)]
    pub interrupted: u32,
}

// ---------------------------------------------------------------------------
// Discovery / naming
// ---------------------------------------------------------------------------

/// Discover audio files under `input` (file or directory).
pub fn discover_inputs(input: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if !input.exists() {
        return Err(UserError::FileNotFound {
            path: input.display().to_string(),
        }
        .into());
    }
    if input.is_file() {
        if is_audio_path(input) {
            return Ok(vec![input.to_path_buf()]);
        }
        return Err(UserError::InvalidAudio {
            reason: format!("{} is not a recognised audio extension", input.display()),
        }
        .into());
    }
    if !input.is_dir() {
        return Err(UserError::InvalidAudio {
            reason: format!("{} is not a file or directory", input.display()),
        }
        .into());
    }

    let mut out = Vec::new();
    walk_dir(input, recursive, &mut out)?;
    out.sort();
    if out.is_empty() {
        return Err(UserError::Other {
            message: format!(
                "no audio files found under {}\n  Hint: supported extensions: {}",
                input.display(),
                AUDIO_EXTENSIONS.join(", ")
            ),
        }
        .into());
    }
    const MAX_BATCH_ITEMS: usize = 10_000;
    if out.len() > MAX_BATCH_ITEMS {
        return Err(UserError::Other {
            message: format!(
                "batch has {} items (max {MAX_BATCH_ITEMS}); split the collection",
                out.len()
            ),
        }
        .into());
    }
    Ok(out)
}

fn walk_dir(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|e| UserError::Other {
        message: format!("read dir {}: {e}", dir.display()),
    })?;
    for ent in entries {
        let ent = ent.map_err(|e| UserError::Other {
            message: format!("read dir entry: {e}"),
        })?;
        let path = ent.path();
        if path.is_dir() {
            if recursive {
                walk_dir(&path, true, out)?;
            }
        } else if is_audio_path(&path) {
            out.push(path);
        }
    }
    Ok(())
}

pub fn is_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Deterministic output file name: `<stem>.<format>` with collision disambiguation.
pub fn output_name_for(source: &Path, format: OutputFormat, used: &mut BTreeSet<String>) -> String {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    let ext = format.default_extension();
    let mut name = format!("{stem}.{ext}");
    if used.insert(name.clone()) {
        return name;
    }
    let h = short_id(&source.display().to_string());
    name = format!("{stem}-{h}.{ext}");
    let mut n = 2u32;
    while !used.insert(name.clone()) {
        name = format!("{stem}-{h}-{n}.{ext}");
        n += 1;
    }
    name
}

/// Build items for a fresh batch from discovered sources.
pub fn build_items(sources: &[PathBuf], format: OutputFormat) -> Vec<BatchItem> {
    let mut used = BTreeSet::new();
    sources
        .iter()
        .map(|src| {
            let source = src.display().to_string();
            let id = short_id(&source);
            let output = output_name_for(src, format, &mut used);
            BatchItem {
                id,
                source,
                output,
                status: BatchItemStatus::Pending,
                error: None,
                attempts: 0,
                source_sha256: None,
                source_size: None,
                output_sha256: None,
                output_size: None,
                operation_fingerprint: None,
                model_digest: None,
                started_at_unix: None,
                finished_at_unix: None,
            }
        })
        .collect()
}

/// Merge new sources into an existing manifest for resume (keeps terminal results).
pub fn merge_for_resume(manifest: &mut BatchManifest, sources: &[PathBuf], format: OutputFormat) {
    let existing: BTreeSet<String> = manifest.items.iter().map(|i| i.source.clone()).collect();
    let mut used: BTreeSet<String> = manifest.items.iter().map(|i| i.output.clone()).collect();
    for src in sources {
        let source = src.display().to_string();
        if existing.contains(&source) {
            continue;
        }
        let id = short_id(&source);
        let output = output_name_for(src, format, &mut used);
        manifest.items.push(BatchItem {
            id,
            source,
            output,
            status: BatchItemStatus::Pending,
            error: None,
            attempts: 0,
            source_sha256: None,
            source_size: None,
            output_sha256: None,
            output_size: None,
            operation_fingerprint: None,
            model_digest: None,
            started_at_unix: None,
            finished_at_unix: None,
        });
    }
    manifest.touch();
}

// ---------------------------------------------------------------------------
// Resume decision table
// ---------------------------------------------------------------------------

/// Outcome of verifying a previously recorded item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeDecision {
    /// Exact match — reuse as succeeded.
    Reuse,
    /// Needs (re)processing.
    Work,
    /// Configuration mismatch — fail closed unless reprocess-changed.
    FailConfiguration,
}

/// Verify a succeeded (or terminal) item against live source/output/fingerprint.
pub fn verify_item_for_resume(
    item: &BatchItem,
    output_dir: &Path,
    current_fingerprint: &str,
    reprocess_changed: bool,
) -> (ResumeDecision, Option<BatchItemStatus>) {
    match item.status {
        BatchItemStatus::Pending => (ResumeDecision::Work, None),
        BatchItemStatus::Failed | BatchItemStatus::Interrupted => (ResumeDecision::Work, None),
        BatchItemStatus::Skipped => (ResumeDecision::Reuse, None),
        BatchItemStatus::Running => {
            // Prior process died mid-item.
            (ResumeDecision::Work, Some(BatchItemStatus::Interrupted))
        }
        BatchItemStatus::StaleSource
        | BatchItemStatus::StaleConfiguration
        | BatchItemStatus::StaleOutput => {
            if reprocess_changed {
                (ResumeDecision::Work, None)
            } else {
                (ResumeDecision::FailConfiguration, None)
            }
        }
        BatchItemStatus::Succeeded => {
            verify_succeeded(item, output_dir, current_fingerprint, reprocess_changed)
        }
    }
}

fn verify_succeeded(
    item: &BatchItem,
    output_dir: &Path,
    current_fingerprint: &str,
    reprocess_changed: bool,
) -> (ResumeDecision, Option<BatchItemStatus>) {
    // 1. Source identity
    let src = PathBuf::from(&item.source);
    match sha256_file_full(&src) {
        Ok((digest, size)) => {
            if item.source_sha256.as_deref() != Some(digest.as_str())
                || item.source_size != Some(size)
            {
                return if reprocess_changed {
                    (ResumeDecision::Work, Some(BatchItemStatus::StaleSource))
                } else {
                    (
                        ResumeDecision::FailConfiguration,
                        Some(BatchItemStatus::StaleSource),
                    )
                };
            }
        }
        Err(_) => {
            return if reprocess_changed {
                (ResumeDecision::Work, Some(BatchItemStatus::StaleSource))
            } else {
                (
                    ResumeDecision::FailConfiguration,
                    Some(BatchItemStatus::StaleSource),
                )
            };
        }
    }

    // 2. Operation fingerprint
    if item.operation_fingerprint.as_deref() != Some(current_fingerprint) {
        return if reprocess_changed {
            (
                ResumeDecision::Work,
                Some(BatchItemStatus::StaleConfiguration),
            )
        } else {
            (
                ResumeDecision::FailConfiguration,
                Some(BatchItemStatus::StaleConfiguration),
            )
        };
    }

    // 3–4. Output exists, regular file, size + digest
    let out_path = output_dir.join(&item.output);
    if out_path
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return if reprocess_changed {
            (ResumeDecision::Work, Some(BatchItemStatus::StaleOutput))
        } else {
            (
                ResumeDecision::FailConfiguration,
                Some(BatchItemStatus::StaleOutput),
            )
        };
    }
    match sha256_file_full(&out_path) {
        Ok((digest, size)) => {
            if item.output_sha256.as_deref() != Some(digest.as_str())
                || item.output_size != Some(size)
            {
                return if reprocess_changed {
                    (ResumeDecision::Work, Some(BatchItemStatus::StaleOutput))
                } else {
                    (
                        ResumeDecision::FailConfiguration,
                        Some(BatchItemStatus::StaleOutput),
                    )
                };
            }
        }
        Err(_) => {
            return if reprocess_changed {
                (ResumeDecision::Work, Some(BatchItemStatus::StaleOutput))
            } else {
                (
                    ResumeDecision::FailConfiguration,
                    Some(BatchItemStatus::StaleOutput),
                )
            };
        }
    }

    (ResumeDecision::Reuse, None)
}

/// Apply resume verification across the manifest; returns indices to process.
///
/// On `FailConfiguration` without reprocess, returns an error describing the first mismatch.
pub fn prepare_resume(
    manifest: &mut BatchManifest,
    current_fingerprint: &str,
    retry_failed: bool,
    reprocess_changed: bool,
) -> Result<Vec<usize>> {
    let output_dir = PathBuf::from(&manifest.output_dir);
    let mut work = Vec::new();

    // Convert abandoned Running → Interrupted first.
    for item in &mut manifest.items {
        if item.status == BatchItemStatus::Running {
            item.status = BatchItemStatus::Interrupted;
            item.error = Some(truncate_error(
                "interrupted: prior process did not finish this item",
            ));
        }
    }

    for (idx, item) in manifest.items.iter_mut().enumerate() {
        let (decision, new_status) =
            verify_item_for_resume(item, &output_dir, current_fingerprint, reprocess_changed);
        if let Some(st) = new_status {
            item.status = st;
        }
        match decision {
            ResumeDecision::Reuse => {}
            ResumeDecision::Work => {
                let should = match item.status {
                    BatchItemStatus::Pending
                    | BatchItemStatus::Interrupted
                    | BatchItemStatus::StaleSource
                    | BatchItemStatus::StaleConfiguration
                    | BatchItemStatus::StaleOutput => true,
                    BatchItemStatus::Failed if retry_failed || reprocess_changed => true,
                    BatchItemStatus::Running => true,
                    _ => false,
                };
                if should {
                    // Reset to pending for reprocess paths.
                    let resettable = matches!(
                        item.status,
                        BatchItemStatus::StaleSource
                            | BatchItemStatus::StaleConfiguration
                            | BatchItemStatus::StaleOutput
                            | BatchItemStatus::Interrupted
                            | BatchItemStatus::Failed
                    );
                    let may_reset = reprocess_changed
                        || matches!(
                            item.status,
                            BatchItemStatus::Interrupted | BatchItemStatus::Failed
                        );
                    if resettable && may_reset {
                        item.status = BatchItemStatus::Pending;
                        item.error = None;
                        item.output_sha256 = None;
                        item.output_size = None;
                    }
                    work.push(idx);
                }
            }
            ResumeDecision::FailConfiguration => {
                return Err(UserError::Other {
                    message: format!(
                        "batch resume refused for item '{}' (status={}): source/config/output mismatch.\n  \
                         Hint: pass --reprocess-changed to opt in to reprocessing, or use a new --output-dir",
                        item.source,
                        item.status.as_str()
                    ),
                }
                .into());
            }
        }
    }
    // Also pick pure pending that verify returned Work for.
    // work_indices fallback for pending not yet covered.
    for (idx, item) in manifest.items.iter().enumerate() {
        if item.status == BatchItemStatus::Pending && !work.contains(&idx) {
            work.push(idx);
        }
        if retry_failed && item.status == BatchItemStatus::Failed && !work.contains(&idx) {
            work.push(idx);
        }
    }
    work.sort_unstable();
    work.dedup();
    Ok(work)
}

/// Indices that still need work (simple path without full verify).
pub fn work_indices(manifest: &BatchManifest, retry_failed: bool) -> Vec<usize> {
    manifest
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| match i.status {
            BatchItemStatus::Pending | BatchItemStatus::Running | BatchItemStatus::Interrupted => {
                true
            }
            BatchItemStatus::Failed if retry_failed => true,
            BatchItemStatus::StaleSource
            | BatchItemStatus::StaleConfiguration
            | BatchItemStatus::StaleOutput => true,
            _ => false,
        })
        .map(|(idx, _)| idx)
        .collect()
}

// ---------------------------------------------------------------------------
// Digests
// ---------------------------------------------------------------------------

/// Full SHA-256 of complete file bytes plus size. Authoritative for resume.
pub fn sha256_file_full(path: &Path) -> Result<(String, u64)> {
    let mut f = File::open(path).map_err(|e| UserError::Other {
        message: format!("open {}: {e}", path.display()),
    })?;
    let meta = f.metadata().map_err(|e| UserError::Other {
        message: format!("stat {}: {e}", path.display()),
    })?;
    if meta.file_type().is_symlink() {
        return Err(UserError::Other {
            message: format!("{} is a symlink (rejected)", path.display()),
        }
        .into());
    }
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 64];
    let mut total = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| UserError::Other {
            message: format!("read {}: {e}", path.display()),
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    if total != meta.len() {
        // Still ok if concurrent truncate; report what we hashed.
    }
    Ok((hex::encode(hasher.finalize()), meta.len()))
}

/// Cheap discovery preflight identity (size + first 1 MiB). **Not** named sha256;
/// must never authorize reuse of a succeeded result.
pub fn discovery_preflight_id(path: &Path) -> Result<String> {
    let mut f = File::open(path).map_err(|e| UserError::Other {
        message: format!("open {}: {e}", path.display()),
    })?;
    let meta = f.metadata().map_err(|e| UserError::Other {
        message: format!("stat {}: {e}", path.display()),
    })?;
    let mut buf = vec![0u8; 1024 * 1024];
    let n = f.read(&mut buf).map_err(|e| UserError::Other {
        message: format!("read {}: {e}", path.display()),
    })?;
    let mut hasher = Sha256::new();
    hasher.update(b"preflight-v1:");
    hasher.update(meta.len().to_le_bytes());
    hasher.update(&buf[..n]);
    Ok(hex::encode(hasher.finalize()))
}

/// Deprecated name retained as a thin alias that documents the partial nature.
/// Prefer [`sha256_file_full`] for resume and [`discovery_preflight_id`] for discovery.
#[deprecated(note = "use sha256_file_full for resume; discovery_preflight_id for cheap discovery")]
pub fn fingerprint_file(path: &Path) -> Result<String> {
    discovery_preflight_id(path)
}

pub fn short_id(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let full = hex::encode(hasher.finalize());
    full[..16].to_string()
}

pub fn truncate_error(msg: &str) -> String {
    let mut out: String = msg.chars().take(MAX_BATCH_ERROR_CHARS).collect();
    if msg.chars().count() > MAX_BATCH_ERROR_CHARS {
        out.push('…');
    }
    out
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_run_id() -> String {
    let mut hasher = Sha256::new();
    hasher.update(unix_now().to_le_bytes());
    hasher.update(format!("{:?}", std::thread::current().id()).as_bytes());
    #[cfg(unix)]
    {
        hasher.update(std::process::id().to_le_bytes());
    }
    let full = hex::encode(hasher.finalize());
    full[..32].to_string()
}

/// Manifest path inside an output directory.
pub fn manifest_path(output_dir: &Path) -> PathBuf {
    output_dir.join(BATCH_MANIFEST_NAME)
}

pub fn lock_path(output_dir: &Path) -> PathBuf {
    output_dir.join(BATCH_LOCK_NAME)
}

fn reject_symlink(path: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(UserError::Other {
                message: format!("refusing symlink path {}", path.display()),
            }
            .into());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Single-writer lock
// ---------------------------------------------------------------------------

/// Batch directory lock (PID + run_id + start time; no private paths).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchLock {
    pub pid: u32,
    pub run_id: String,
    pub started_at_unix: u64,
    pub aurum_version: String,
}

/// Acquire exclusive lock for `output_dir`. Fails if a live lock exists.
pub fn acquire_batch_lock(output_dir: &Path, run_id: &str) -> Result<BatchLockGuard> {
    fs::create_dir_all(output_dir).map_err(|e| UserError::Other {
        message: format!("create batch output dir {}: {e}", output_dir.display()),
    })?;
    let path = lock_path(output_dir);
    if path.exists() {
        // Do not break a live lock automatically.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        return Err(UserError::Other {
            message: format!(
                "batch lock exists at {} — another process may be writing this output directory.\n  \
                 Lock metadata: {}\n  \
                 Hint: if the holder is dead, remove the lock file deliberately after verifying the PID is gone",
                path.display(),
                existing.chars().take(200).collect::<String>()
            ),
        }
        .into());
    }
    let lock = BatchLock {
        pid: std::process::id(),
        run_id: run_id.into(),
        started_at_unix: unix_now(),
        aurum_version: env!("CARGO_PKG_VERSION").into(),
    };
    let json = serde_json::to_string_pretty(&lock).map_err(|e| UserError::Other {
        message: format!("batch lock json: {e}"),
    })?;
    // Exclusive create.
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    let mut f = opts.open(&path).map_err(|e| UserError::Other {
        message: format!("create batch lock {}: {e}", path.display()),
    })?;
    f.write_all(json.as_bytes()).map_err(|e| UserError::Other {
        message: format!("write batch lock: {e}"),
    })?;
    f.sync_all().ok();
    Ok(BatchLockGuard { path, lock })
}

/// RAII guard that removes the lock file on drop.
pub struct BatchLockGuard {
    path: PathBuf,
    lock: BatchLock,
}

impl BatchLockGuard {
    pub fn lock(&self) -> &BatchLock {
        &self.lock
    }
}

impl Drop for BatchLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Provider registry parity
// ---------------------------------------------------------------------------

/// Validate that `provider` is a registered STT provider (not a hard-coded subset).
pub fn validate_batch_stt_provider(
    registry: &ProviderRegistry,
    provider: &str,
) -> Result<ProviderId> {
    let id = ProviderId::parse(provider)?;
    // Resolve via registry STT factory — same path as one-file CLI/library.
    registry.stt_factory(&id)?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fp_input(model: &str) -> OperationFingerprintInput {
        OperationFingerprintInput {
            provider_id: "local".into(),
            backend_route: "whisper_cpp".into(),
            model_id: model.into(),
            support_evidence: None,
            language: "en".into(),
            timestamps: false,
            allow_unreliable_timestamps: false,
            output_format: "txt".into(),
            cleanup_style: "raw".into(),
            cleanup_provider: "rules".into(),
            cleanup_model: None,
            cleanup_segments: "auto".into(),
            long_form_policy: None,
            dto_schema_version: "1".into(),
            profile: None,
            profile_evidence_version: None,
            local_only: false,
            trust_mode: None,
            aurum_behavior_version: "0.0.22".into(),
        }
    }

    #[test]
    fn fingerprint_stable_and_sensitive() {
        let a = operation_fingerprint(&fp_input("base"));
        let b = operation_fingerprint(&fp_input("base"));
        assert_eq!(a, b);
        let c = operation_fingerprint(&fp_input("tiny-q5_1"));
        assert_ne!(a, c);
        let mut x = fp_input("base");
        x.timestamps = true;
        assert_ne!(a, operation_fingerprint(&x));
    }

    #[test]
    fn full_digest_detects_change_after_first_mib() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let mut data = vec![0u8; 1024 * 1024 + 64];
        data[0] = 1;
        fs::write(&path, &data).unwrap();
        let (d1, s1) = sha256_file_full(&path).unwrap();
        // Change only after first MiB; size unchanged.
        data[1024 * 1024 + 10] = 0xAB;
        fs::write(&path, &data).unwrap();
        let (d2, s2) = sha256_file_full(&path).unwrap();
        assert_eq!(s1, s2);
        assert_ne!(d1, d2);
        // Preflight may miss the change (only first MiB) — that's why it must not authorize reuse.
        let p1 = discovery_preflight_id(&path).unwrap();
        data[1024 * 1024 + 10] = 0x00;
        fs::write(&path, &data).unwrap();
        let p2 = discovery_preflight_id(&path).unwrap();
        // After restore of tail, preflight of original vs changed-tail:
        // re-write changed again for preflight equality check on first MiB only
        let mut data2 = vec![0u8; 1024 * 1024 + 64];
        data2[0] = 1;
        fs::write(&path, &data2).unwrap();
        let p_base = discovery_preflight_id(&path).unwrap();
        data2[1024 * 1024 + 10] = 0xAB;
        fs::write(&path, &data2).unwrap();
        let p_changed_tail = discovery_preflight_id(&path).unwrap();
        assert_eq!(
            p_base, p_changed_tail,
            "preflight must only see first MiB+size"
        );
        let _ = (p1, p2);
    }

    #[test]
    fn discover_and_names_stable() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.wav"), b"x").unwrap();
        fs::write(dir.path().join("b.mp3"), b"y").unwrap();
        fs::write(dir.path().join("skip.txt"), b"z").unwrap();
        let found = discover_inputs(dir.path(), false).unwrap();
        assert_eq!(found.len(), 2);
        let items = build_items(&found, OutputFormat::Txt);
        assert_eq!(items[0].output, "a.txt");
        assert_eq!(items[1].output, "b.txt");
        assert_eq!(items[0].status, BatchItemStatus::Pending);
    }

    #[test]
    fn resume_keeps_succeeded() {
        let dir = tempdir().unwrap();
        let fp = operation_fingerprint(&fp_input("base"));
        let mut m = BatchManifest::new(
            "local",
            "base",
            "auto",
            OutputFormat::Txt,
            dir.path(),
            None,
            &fp,
        );
        m.items.push(BatchItem {
            id: "1".into(),
            source: "/x/a.wav".into(),
            output: "a.txt".into(),
            status: BatchItemStatus::Succeeded,
            error: None,
            attempts: 1,
            source_sha256: None,
            source_size: None,
            output_sha256: None,
            output_size: None,
            operation_fingerprint: Some(fp.clone()),
            model_digest: None,
            started_at_unix: None,
            finished_at_unix: None,
        });
        m.items.push(BatchItem {
            id: "2".into(),
            source: "/x/b.wav".into(),
            output: "b.txt".into(),
            status: BatchItemStatus::Failed,
            error: Some("boom".into()),
            attempts: 1,
            source_sha256: None,
            source_size: None,
            output_sha256: None,
            output_size: None,
            operation_fingerprint: Some(fp),
            model_digest: None,
            started_at_unix: None,
            finished_at_unix: None,
        });
        let work = work_indices(&m, false);
        assert!(work.is_empty());
        let retry = work_indices(&m, true);
        assert_eq!(retry, vec![1]);
    }

    #[test]
    fn manifest_roundtrip_via_transaction() {
        let dir = tempdir().unwrap();
        let fp = operation_fingerprint(&fp_input("tiny-q5_1"));
        let mut m = BatchManifest::new(
            "local",
            "tiny-q5_1",
            "en",
            OutputFormat::Json,
            dir.path(),
            Some("speed"),
            &fp,
        );
        m.items = build_items(&[PathBuf::from("/tmp/x.wav")], OutputFormat::Json);
        let path = manifest_path(dir.path());
        m.save(&path).unwrap();
        let loaded = BatchManifest::load(&path).unwrap();
        assert_eq!(loaded.model, "tiny-q5_1");
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.profile.as_deref(), Some("speed"));
        assert_eq!(loaded.schema_version, 2);
        assert!(!loaded.run_id.is_empty());
    }

    #[test]
    fn v1_manifest_rejected() {
        let dir = tempdir().unwrap();
        let path = manifest_path(dir.path());
        let v1 = r#"{
            "schema_version": 1,
            "aurum_version": "0.0.21",
            "created_at_unix": 1,
            "updated_at_unix": 1,
            "provider": "local",
            "model": "base",
            "language": "en",
            "output_format": "txt",
            "output_dir": "/tmp",
            "items": []
        }"#;
        fs::write(&path, v1).unwrap();
        let err = BatchManifest::load(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("schema v1"), "{msg}");
    }

    #[test]
    fn resume_decision_stale_source() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("a.wav");
        fs::write(&src, b"hello-audio").unwrap();
        let (digest, size) = sha256_file_full(&src).unwrap();
        let out = dir.path().join("a.txt");
        fs::write(&out, b"transcript").unwrap();
        let (od, os) = sha256_file_full(&out).unwrap();
        let fp = operation_fingerprint(&fp_input("base"));
        let item = BatchItem {
            id: "1".into(),
            source: src.display().to_string(),
            output: "a.txt".into(),
            status: BatchItemStatus::Succeeded,
            error: None,
            attempts: 1,
            source_sha256: Some(digest),
            source_size: Some(size),
            output_sha256: Some(od),
            output_size: Some(os),
            operation_fingerprint: Some(fp.clone()),
            model_digest: None,
            started_at_unix: None,
            finished_at_unix: None,
        };
        // Exact match
        let (d, _) = verify_item_for_resume(&item, dir.path(), &fp, false);
        assert_eq!(d, ResumeDecision::Reuse);
        // Change source after first byte
        fs::write(&src, b"HELLO-AUDIO").unwrap();
        let (d2, st) = verify_item_for_resume(&item, dir.path(), &fp, false);
        assert_eq!(d2, ResumeDecision::FailConfiguration);
        assert_eq!(st, Some(BatchItemStatus::StaleSource));
        let (d3, st3) = verify_item_for_resume(&item, dir.path(), &fp, true);
        assert_eq!(d3, ResumeDecision::Work);
        assert_eq!(st3, Some(BatchItemStatus::StaleSource));
    }

    #[test]
    fn lock_exclusive() {
        let dir = tempdir().unwrap();
        let g1 = acquire_batch_lock(dir.path(), "run1").unwrap();
        assert!(acquire_batch_lock(dir.path(), "run2").is_err());
        drop(g1);
        let g2 = acquire_batch_lock(dir.path(), "run2").unwrap();
        drop(g2);
    }

    #[test]
    fn running_becomes_interrupted_on_prepare() {
        let dir = tempdir().unwrap();
        let fp = operation_fingerprint(&fp_input("base"));
        let mut m = BatchManifest::new(
            "local",
            "base",
            "en",
            OutputFormat::Txt,
            dir.path(),
            None,
            &fp,
        );
        m.items.push(BatchItem {
            id: "1".into(),
            source: dir.path().join("missing.wav").display().to_string(),
            output: "a.txt".into(),
            status: BatchItemStatus::Running,
            error: None,
            attempts: 1,
            source_sha256: None,
            source_size: None,
            output_sha256: None,
            output_size: None,
            operation_fingerprint: Some(fp.clone()),
            model_digest: None,
            started_at_unix: None,
            finished_at_unix: None,
        });
        let work = prepare_resume(&mut m, &fp, true, true).unwrap();
        assert_eq!(work, vec![0]);
        assert_eq!(m.items[0].status, BatchItemStatus::Pending);
    }
}
