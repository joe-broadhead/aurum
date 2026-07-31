//! Bounded, resumable multi-file transcription manifests (JOE-1726).
//!
//! The CLI drives transcription; this module owns discovery, deterministic
//! naming, versioned manifests, resume selection, and partial-success reports.

use crate::error::{Result, UserError};
use crate::output::OutputFormat;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Manifest schema version.
pub const BATCH_MANIFEST_VERSION: u32 = 1;

/// Default manifest filename written into the batch output directory.
pub const BATCH_MANIFEST_NAME: &str = "aurum-batch-manifest.json";

/// Extensions treated as transcription inputs (lowercase, no dot).
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "m4a", "flac", "ogg", "oga", "opus", "webm", "aac", "mp4", "mpeg", "mpga",
];

/// Per-item status in the batch manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

/// One file in a batch.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
}

/// Versioned batch manifest (machine-readable resume state).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchManifest {
    pub schema_version: u32,
    pub aurum_version: String,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub provider: String,
    pub model: String,
    pub language: String,
    pub output_format: String,
    /// Absolute or original path string for the output directory.
    pub output_dir: String,
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
    ) -> Self {
        let now = unix_now();
        Self {
            schema_version: BATCH_MANIFEST_VERSION,
            aurum_version: env!("CARGO_PKG_VERSION").into(),
            created_at_unix: now,
            updated_at_unix: now,
            provider: provider.into(),
            model: model.into(),
            language: language.into(),
            output_format: format.as_str().into(),
            output_dir: output_dir.display().to_string(),
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

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| UserError::Other {
                message: format!("create batch output dir {}: {e}", parent.display()),
            })?;
        }
        let json = self.to_json_pretty()?;
        // Write via temp + rename for crash safety.
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json.as_bytes()).map_err(|e| UserError::Other {
            message: format!("write batch manifest temp: {e}"),
        })?;
        fs::rename(&tmp, path).map_err(|e| UserError::Other {
            message: format!("publish batch manifest: {e}"),
        })?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).map_err(|e| UserError::Other {
            message: format!("read batch manifest {}: {e}", path.display()),
        })?;
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
}

/// Discover audio files under `input` (file or directory).
///
/// Directories are walked non-recursively by default; set `recursive` for depth-first.
/// Results are sorted for deterministic ordering.
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
    // Cap batch size for resource governance (hard ceiling).
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
    // Disambiguate with a short hash of the full path.
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
        });
    }
    manifest.touch();
}

/// Indices that still need work.
pub fn work_indices(manifest: &BatchManifest, retry_failed: bool) -> Vec<usize> {
    manifest
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| match i.status {
            BatchItemStatus::Pending | BatchItemStatus::Running => true,
            BatchItemStatus::Failed if retry_failed => true,
            _ => false,
        })
        .map(|(idx, _)| idx)
        .collect()
}

/// Optional cheap content fingerprint (first 1 MiB + size) for resume honesty.
pub fn fingerprint_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).map_err(|e| UserError::Other {
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
    hasher.update(meta.len().to_le_bytes());
    hasher.update(&buf[..n]);
    Ok(hex::encode(hasher.finalize()))
}

pub fn short_id(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let full = hex::encode(hasher.finalize());
    full[..16].to_string()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Manifest path inside an output directory.
pub fn manifest_path(output_dir: &Path) -> PathBuf {
    output_dir.join(BATCH_MANIFEST_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
        let mut m =
            BatchManifest::new("local", "base", "auto", OutputFormat::Txt, dir.path(), None);
        m.items.push(BatchItem {
            id: "1".into(),
            source: "/x/a.wav".into(),
            output: "a.txt".into(),
            status: BatchItemStatus::Succeeded,
            error: None,
            attempts: 1,
            source_sha256: None,
        });
        m.items.push(BatchItem {
            id: "2".into(),
            source: "/x/b.wav".into(),
            output: "b.txt".into(),
            status: BatchItemStatus::Failed,
            error: Some("boom".into()),
            attempts: 1,
            source_sha256: None,
        });
        let work = work_indices(&m, false);
        assert!(work.is_empty());
        let retry = work_indices(&m, true);
        assert_eq!(retry, vec![1]);
    }

    #[test]
    fn manifest_roundtrip() {
        let dir = tempdir().unwrap();
        let mut m = BatchManifest::new(
            "local",
            "tiny-q5_1",
            "en",
            OutputFormat::Json,
            dir.path(),
            Some("speed"),
        );
        m.items = build_items(&[PathBuf::from("/tmp/x.wav")], OutputFormat::Json);
        let path = manifest_path(dir.path());
        m.save(&path).unwrap();
        let loaded = BatchManifest::load(&path).unwrap();
        assert_eq!(loaded.model, "tiny-q5_1");
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.profile.as_deref(), Some("speed"));
    }
}
