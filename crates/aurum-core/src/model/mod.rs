//! Local whisper.cpp model management: resolve, download, cache, list.

use crate::download::{self, DownloadOptions, DownloadRequest};
use crate::error::{EnvironmentError, ProviderError, Result, UserError};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// HuggingFace repo hosting official ggml whisper.cpp models.
/// Content authenticity is enforced by reviewed SHA-256 pins (JOE-1590), not by
/// mutable branch tip alone. Prefer pins over URL mutability.
const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
/// Manifest schema version for diagnostics (JOE-1590).
pub const ARTIFACT_MANIFEST_VERSION: &str = "1";
/// Provenance label for built-in pins.
pub const ARTIFACT_MANIFEST_SOURCE: &str = "aurum-builtin-review";

/// Known local model names and their ggml filenames.
#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    pub name: &'static str,
    pub filename: &'static str,
    /// Approximate download size in bytes (for progress UX).
    pub approx_bytes: u64,
    /// Human label for lists (e.g. "quantized", "english-only").
    pub notes: &'static str,
}

/// Models supported in v0.0.0 (full + common quantized variants).
pub const MODELS: &[ModelInfo] = &[
    // ---- tiny ----
    ModelInfo {
        name: "tiny",
        filename: "ggml-tiny.bin",
        approx_bytes: 75_000_000,
        notes: "fastest full-precision",
    },
    ModelInfo {
        name: "tiny-q5_1",
        filename: "ggml-tiny-q5_1.bin",
        approx_bytes: 32_000_000,
        notes: "quantized ~32MB — best first-run trial",
    },
    ModelInfo {
        name: "tiny-q8_0",
        filename: "ggml-tiny-q8_0.bin",
        approx_bytes: 44_000_000,
        notes: "quantized",
    },
    ModelInfo {
        name: "tiny.en",
        filename: "ggml-tiny.en.bin",
        approx_bytes: 75_000_000,
        notes: "english-only",
    },
    ModelInfo {
        name: "tiny.en-q5_1",
        filename: "ggml-tiny.en-q5_1.bin",
        approx_bytes: 32_000_000,
        notes: "english-only quantized",
    },
    // ---- base ----
    ModelInfo {
        name: "base",
        filename: "ggml-base.bin",
        approx_bytes: 142_000_000,
        notes: "default full-precision",
    },
    ModelInfo {
        name: "base-q5_1",
        filename: "ggml-base-q5_1.bin",
        approx_bytes: 60_000_000,
        notes: "quantized ~60MB",
    },
    ModelInfo {
        name: "base-q8_0",
        filename: "ggml-base-q8_0.bin",
        approx_bytes: 82_000_000,
        notes: "quantized",
    },
    ModelInfo {
        name: "base.en",
        filename: "ggml-base.en.bin",
        approx_bytes: 142_000_000,
        notes: "english-only",
    },
    ModelInfo {
        name: "base.en-q5_1",
        filename: "ggml-base.en-q5_1.bin",
        approx_bytes: 60_000_000,
        notes: "english-only quantized",
    },
    // ---- small ----
    ModelInfo {
        name: "small",
        filename: "ggml-small.bin",
        approx_bytes: 466_000_000,
        notes: "higher accuracy",
    },
    ModelInfo {
        name: "small-q5_1",
        filename: "ggml-small-q5_1.bin",
        approx_bytes: 190_000_000,
        notes: "quantized",
    },
    ModelInfo {
        name: "small-q8_0",
        filename: "ggml-small-q8_0.bin",
        approx_bytes: 264_000_000,
        notes: "quantized",
    },
    ModelInfo {
        name: "small.en",
        filename: "ggml-small.en.bin",
        approx_bytes: 466_000_000,
        notes: "english-only",
    },
    ModelInfo {
        name: "small.en-q5_1",
        filename: "ggml-small.en-q5_1.bin",
        approx_bytes: 190_000_000,
        notes: "english-only quantized",
    },
    // ---- medium ----
    ModelInfo {
        name: "medium",
        filename: "ggml-medium.bin",
        approx_bytes: 1_500_000_000,
        notes: "large download",
    },
    ModelInfo {
        name: "medium.en",
        filename: "ggml-medium.en.bin",
        approx_bytes: 1_500_000_000,
        notes: "english-only",
    },
    // ---- large ----
    ModelInfo {
        name: "large-v3",
        filename: "ggml-large-v3.bin",
        approx_bytes: 3_100_000_000,
        notes: "highest quality",
    },
    ModelInfo {
        name: "large-v3-q5_0",
        filename: "ggml-large-v3-q5_0.bin",
        approx_bytes: 1_080_000_000,
        notes: "experimental — not recommended (degeneration risk; prefer large-v3-turbo)",
    },
    ModelInfo {
        name: "large",
        filename: "ggml-large-v3.bin",
        approx_bytes: 3_100_000_000,
        notes: "alias of large-v3",
    },
    ModelInfo {
        name: "large-v3-turbo",
        filename: "ggml-large-v3-turbo.bin",
        approx_bytes: 1_600_000_000,
        notes: "fast large",
    },
    ModelInfo {
        name: "large-v3-turbo-q5_0",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        approx_bytes: 574_000_000,
        notes: "quantized turbo",
    },
    ModelInfo {
        name: "turbo",
        filename: "ggml-large-v3-turbo.bin",
        approx_bytes: 1_600_000_000,
        notes: "alias of large-v3-turbo",
    },
    ModelInfo {
        name: "turbo-q5_0",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        approx_bytes: 574_000_000,
        notes: "alias of large-v3-turbo-q5_0",
    },
];

/// Names shown in user-facing help (canonical, not aliases).
pub fn available_model_names() -> String {
    list_canonical_models()
        .iter()
        .map(|m| m.name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn list_canonical_models() -> Vec<&'static ModelInfo> {
    MODELS
        .iter()
        .filter(|m| !matches!(m.name, "large" | "turbo" | "turbo-q5_0"))
        .collect()
}

pub fn lookup_model(name: &str) -> Result<&'static ModelInfo> {
    let key = name.trim().to_ascii_lowercase();
    MODELS.iter().find(|m| m.name == key).ok_or_else(|| {
        UserError::InvalidModel {
            model: name.to_string(),
            available: available_model_names(),
        }
        .into()
    })
}

/// Directory where ggml models are stored: `<cache>/models/`.
pub fn models_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("models")
}

/// Path to a cached model file (may not exist yet).
pub fn model_path(cache_dir: &Path, info: &ModelInfo) -> PathBuf {
    models_dir(cache_dir).join(info.filename)
}

/// Status of a model relative to the local cache.
#[derive(Debug, Clone)]
pub struct ModelStatus {
    pub info: &'static ModelInfo,
    pub cached: bool,
    pub path: PathBuf,
    pub size_bytes: Option<u64>,
}

/// List canonical models and whether each is cached.
pub fn list_models(cache_dir: &Path) -> Vec<ModelStatus> {
    list_canonical_models()
        .into_iter()
        .map(|info| {
            let path = model_path(cache_dir, info);
            let (cached, size_bytes) = match fs::metadata(&path) {
                Ok(m) if m.len() > 1_000_000 => (true, Some(m.len())),
                _ => (false, None),
            };
            ModelStatus {
                info,
                cached,
                path,
                size_bytes,
            }
        })
        .collect()
}

/// Format a human-readable model table for CLI output.
pub fn format_model_list(cache_dir: &Path) -> String {
    let rows = list_models(cache_dir);
    let mut out = String::from("Local whisper.cpp models (cache: ");
    out.push_str(&models_dir(cache_dir).display().to_string());
    out.push_str(")\n\n");
    out.push_str(&format!(
        "{:<22} {:>10}  {:<8}  {:<12}  {}\n",
        "NAME", "SIZE", "STATUS", "TIER", "NOTES"
    ));
    out.push_str(&format!(
        "{:<22} {:>10}  {:<8}  {:<12}  {}\n",
        "----", "----", "------", "----", "-----"
    ));
    for row in rows {
        let size = format_bytes(row.info.approx_bytes);
        let status = if row.cached { "cached" } else { "—" };
        let tier = match model_support_tier(row.info.name) {
            ModelSupportTier::Supported => "supported",
            ModelSupportTier::Experimental => "experimental",
        };
        out.push_str(&format!(
            "{:<22} {:>10}  {:<8}  {:<12}  {}\n",
            row.info.name, size, status, tier, row.info.notes
        ));
    }
    out.push_str(
        "\nTip: first run downloads the selected model. Try `tiny-q5_1` (~32 MB) for a quick trial.\n",
    );
    out.push_str("Default model: `base` (~142 MB). Use --model <name> to choose.\n");
    out.push_str(
        "Guidance (single-clip dogfood, not formal WER): English lecture quality often \
         favors `small.en` or `large-v3-turbo`; prefer `.en` variants for English-only audio. \
         Avoid `large-v3-q5_0` for production (experimental).\n",
    );
    out
}

fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = n as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else if n >= MB {
        format!("{:.0} MB", n / MB)
    } else {
        format!("{:.0} KB", n / KB)
    }
}

/// Progress event while downloading a model (library hosts / UI).
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub model: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

impl DownloadProgress {
    pub fn fraction(&self) -> Option<f64> {
        if self.total_bytes == 0 {
            None
        } else {
            Some((self.downloaded_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0))
        }
    }
}

/// Callback for download progress. Invoked from the async download task.
pub type DownloadProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

/// Options for [`ensure_model`].
#[derive(Clone, Default)]
pub struct EnsureModelOptions {
    /// When true, never hit the network; fail if the model is not already cached.
    pub local_only: bool,
    /// Show CLI-style progress on stderr (indicatif).
    pub show_progress: bool,
    /// Optional structured progress hook for embedders.
    pub on_progress: Option<DownloadProgressCallback>,
}

impl EnsureModelOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn local_only(mut self, v: bool) -> Self {
        self.local_only = v;
        self
    }

    pub fn show_progress(mut self, v: bool) -> Self {
        self.show_progress = v;
        self
    }

    pub fn on_progress(mut self, cb: DownloadProgressCallback) -> Self {
        self.on_progress = Some(cb);
        self
    }
}

/// True if a usable model file is already on disk.
pub fn is_model_cached(cache_dir: &Path, model_name: &str) -> bool {
    let Ok(info) = lookup_model(model_name) else {
        return false;
    };
    let path = model_path(cache_dir, info);
    if !(path.exists()
        && path
            .metadata()
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false))
    {
        return false;
    }
    // Trusted catalogue requires a reviewed pin (JOE-1645).
    pinned_sha256(info.filename).is_some() && verify_model_basic(&path, info).is_ok()
}

/// Ensure a model is present locally, downloading if needed. Returns the path.
pub async fn ensure_model(
    cache_dir: &Path,
    model_name: &str,
    show_progress: bool,
) -> Result<PathBuf> {
    ensure_model_with_options(
        cache_dir,
        model_name,
        EnsureModelOptions {
            show_progress,
            ..EnsureModelOptions::default()
        },
    )
    .await
}

/// Ensure model with offline / progress options.
pub async fn ensure_model_with_options(
    cache_dir: &Path,
    model_name: &str,
    opts: EnsureModelOptions,
) -> Result<PathBuf> {
    let info = lookup_model(model_name)?;
    let path = model_path(cache_dir, info);

    if path.exists()
        && path
            .metadata()
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false)
    {
        // Trusted catalogue requires reviewed pin + integrity (JOE-1645).
        if pinned_sha256(info.filename).is_none() {
            return Err(ProviderError::ModelDownload {
                model: model_name.to_string(),
                reason: format!(
                    "model `{}` has no reviewed SHA-256 pin — not available on the trusted path",
                    info.name
                ),
            }
            .into());
        }
        if verify_model_basic(&path, info).is_ok() {
            tracing::info!(model = info.name, path = %path.display(), "using cached model");
            return Ok(path);
        }
        tracing::warn!(
            model = info.name,
            path = %path.display(),
            "cached model failed integrity check; re-downloading"
        );
        let _ = fs::remove_file(&path);
    }

    if opts.local_only {
        return Err(UserError::ModelNotCached {
            model: model_name.to_string(),
        }
        .into());
    }

    fs::create_dir_all(models_dir(cache_dir)).map_err(|e| EnvironmentError::DirectoryAccess {
        path: models_dir(cache_dir).display().to_string(),
        reason: e.to_string(),
    })?;

    // Cross-process advisory lock so concurrent aurum runs don't double-download.
    let lock_path = path.with_extension("bin.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| EnvironmentError::DirectoryAccess {
            path: lock_path.display().to_string(),
            reason: e.to_string(),
        })?;

    if opts.show_progress {
        eprintln!("aurum: waiting for model download lock ({}) …", info.name);
    }
    lock_file.lock().map_err(|e| EnvironmentError::Other {
        message: format!("failed to acquire model lock: {e}"),
    })?;

    // Re-check after lock — another process may have finished the download.
    if path.exists()
        && path
            .metadata()
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false)
        && pinned_sha256(info.filename).is_some()
        && verify_model_basic(&path, info).is_ok()
    {
        let _ = lock_file.unlock();
        tracing::info!(model = info.name, "model appeared while waiting on lock");
        return Ok(path);
    }

    if opts.local_only {
        let _ = lock_file.unlock();
        return Err(UserError::ModelNotCached {
            model: model_name.to_string(),
        }
        .into());
    }

    if opts.show_progress {
        eprintln!(
            "aurum: downloading model `{}` ({}) — first run only …",
            info.name,
            format_bytes(info.approx_bytes)
        );
    }

    let result = download_model(info, &path, opts.show_progress, opts.on_progress.as_ref()).await;
    let _ = lock_file.unlock();
    result?;
    verify_model_basic(&path, info)?;
    Ok(path)
}

async fn download_model(
    info: &ModelInfo,
    dest: &Path,
    show_progress: bool,
    on_progress: Option<&DownloadProgressCallback>,
) -> Result<()> {
    // Verify-before-publish requires a reviewed pin (JOE-1591 / JOE-1645).
    let Some(expected) = pinned_sha256(info.filename) else {
        return Err(ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!(
                "no reviewed SHA-256 pin for {} — refusing to publish unauthenticated artifact",
                info.filename
            ),
        }
        .into());
    };

    let url = format!("{HF_BASE}/{}?download=true", info.filename);
    let req = DownloadRequest {
        id: info.name,
        filename: info.filename,
        sha256: expected,
        exact_bytes: pinned_exact_bytes(info.filename),
        approx_bytes: info.approx_bytes,
        url: &url,
    };

    let mut opts = DownloadOptions {
        show_progress,
        ..DownloadOptions::default()
    };
    if let Some(cb) = on_progress {
        let model = info.name.to_string();
        let cb = Arc::clone(cb);
        opts.on_progress = Some(Arc::new(move |downloaded, total| {
            cb(DownloadProgress {
                model: model.clone(),
                downloaded_bytes: downloaded,
                total_bytes: total,
            });
        }));
    }

    download::download_verified_request(&req, dest, &opts).await?;

    // Best-effort cleanup of orphaned partials from prior crashed runs.
    sweep_stale_partials(dest.parent().unwrap_or_else(|| Path::new(".")));
    Ok(())
}

/// Independently reviewed SHA-256 digests (JOE-1590 / JOE-1645).
///
/// Every trusted catalogue filename must have a pin. Identity is the digest +
/// exact size, not a mutable upstream branch tip. A missing pin is a release
/// defect: download refuses to publish and cache verify never labels the file healthy.
pub fn pinned_sha256(filename: &str) -> Option<&'static str> {
    match filename {
        "ggml-tiny.bin" => Some("be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
        "ggml-tiny-q5_1.bin" => {
            Some("818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7")
        }
        "ggml-tiny-q8_0.bin" => {
            Some("c2085835d3f50733e2ff6e4b41ae8a2b8d8110461e18821b09a15c40c42d1cca")
        }
        "ggml-tiny.en.bin" => {
            Some("921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f")
        }
        "ggml-tiny.en-q5_1.bin" => {
            Some("c77c5766f1cef09b6b7d47f21b546cbddd4157886b3b5d6d4f709e91e66c7c2b")
        }
        "ggml-base.bin" => Some("60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
        "ggml-base-q5_1.bin" => {
            Some("422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898")
        }
        "ggml-base-q8_0.bin" => {
            Some("c577b9a86e7e048a0b7eada054f4dd79a56bbfa911fbdacf900ac5b567cbb7d9")
        }
        "ggml-base.en.bin" => {
            Some("a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002")
        }
        "ggml-base.en-q5_1.bin" => {
            Some("4baf70dd0d7c4247ba2b81fafd9c01005ac77c2f9ef064e00dcf195d0e2fdd2f")
        }
        "ggml-small.bin" => {
            Some("1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b")
        }
        "ggml-small-q5_1.bin" => {
            Some("ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb")
        }
        "ggml-small-q8_0.bin" => {
            Some("49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f")
        }
        "ggml-small.en.bin" => {
            Some("c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d")
        }
        "ggml-small.en-q5_1.bin" => {
            Some("bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30")
        }
        "ggml-medium.bin" => {
            Some("6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208")
        }
        "ggml-medium.en.bin" => {
            Some("cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356")
        }
        "ggml-large-v3.bin" => {
            Some("64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2")
        }
        "ggml-large-v3-q5_0.bin" => {
            Some("d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1")
        }
        "ggml-large-v3-turbo.bin" => {
            Some("1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69")
        }
        "ggml-large-v3-turbo-q5_0.bin" => {
            Some("394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2")
        }
        _ => None,
    }
}

/// Support tier for catalogue display (JOE-1650).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSupportTier {
    /// Default recommended catalogue entries.
    Supported,
    /// Available but not recommended for production quality (known risk).
    Experimental,
}

/// Support tier for a catalogue name (aliases resolve via [`lookup_model`]).
pub fn model_support_tier(name: &str) -> ModelSupportTier {
    match name {
        "large-v3-q5_0" => ModelSupportTier::Experimental,
        _ => ModelSupportTier::Supported,
    }
}

/// Reviewed exact sizes when known (from HF package metadata / maintainer review).
pub fn pinned_exact_bytes(filename: &str) -> Option<u64> {
    match filename {
        "ggml-tiny.bin" => Some(77_691_713),
        "ggml-tiny-q5_1.bin" => Some(32_152_673),
        "ggml-tiny-q8_0.bin" => Some(43_537_433),
        "ggml-tiny.en.bin" => Some(77_704_715),
        "ggml-tiny.en-q5_1.bin" => Some(32_166_155),
        "ggml-base.bin" => Some(147_951_465),
        "ggml-base-q5_1.bin" => Some(59_707_625),
        "ggml-base-q8_0.bin" => Some(81_768_585),
        "ggml-base.en.bin" => Some(147_964_211),
        "ggml-base.en-q5_1.bin" => Some(59_721_011),
        "ggml-small.bin" => Some(487_601_967),
        "ggml-small-q5_1.bin" => Some(190_085_487),
        "ggml-small-q8_0.bin" => Some(264_464_607),
        "ggml-small.en.bin" => Some(487_614_201),
        "ggml-small.en-q5_1.bin" => Some(190_098_681),
        "ggml-medium.bin" => Some(1_533_763_059),
        "ggml-medium.en.bin" => Some(1_533_774_781),
        "ggml-large-v3.bin" => Some(3_095_033_483),
        "ggml-large-v3-q5_0.bin" => Some(1_081_140_203),
        "ggml-large-v3-turbo.bin" => Some(1_624_555_275),
        "ggml-large-v3-turbo-q5_0.bin" => Some(574_041_195),
        _ => None,
    }
}

/// Diagnostic JSON for a catalogue entry (manifest provenance).
pub fn artifact_manifest_json(info: &ModelInfo) -> serde_json::Value {
    serde_json::json!({
        "manifest_version": ARTIFACT_MANIFEST_VERSION,
        "source": ARTIFACT_MANIFEST_SOURCE,
        "id": info.name,
        "filename": info.filename,
        "approx_bytes": info.approx_bytes,
        "exact_bytes": pinned_exact_bytes(info.filename),
        "sha256": pinned_sha256(info.filename),
        "license": "MIT (whisper.cpp weights via OpenAI Whisper terms)",
        "family": "whisper",
        "download_url_template": format!("{HF_BASE}/{}", info.filename),
    })
}

fn sweep_stale_partials(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let stale_after = std::time::Duration::from_secs(6 * 3600);
    let now = std::time::SystemTime::now();
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        // Only stale leftovers — never touch another live download's unique partial.
        // Matches both legacy `.bin.partial.PID-ms` and exclusive `.name.PID-ns.aurum.partial`.
        if !name.contains(".bin.partial.") && !name.ends_with(".aurum.partial") {
            continue;
        }
        let Ok(meta) = ent.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if now.duration_since(modified).unwrap_or_default() > stale_after {
            let _ = fs::remove_file(ent.path());
        }
    }
}

/// Public local-only verify used by cache inventory (no network).
pub fn ensure_model_verified_local(path: &Path, info: &ModelInfo) -> Result<()> {
    verify_model_basic(path, info)
}

/// Basic integrity check: file exists, is large enough, and starts with ggml magic-ish bytes.
fn verify_model_basic(path: &Path, info: &ModelInfo) -> Result<()> {
    let meta = fs::metadata(path).map_err(|e| ProviderError::ModelDownload {
        model: info.name.to_string(),
        reason: format!("missing after download: {e}"),
    })?;

    if meta.len() < 1_000_000 {
        // Do not delete — cache verify quarantines; leave bytes for forensics.
        return Err(ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!(
                "model file is only {} bytes — likely truncated or HTML error page",
                meta.len()
            ),
        }
        .into());
    }

    let mut hdr = [0u8; 4];
    let mut f = File::open(path).map_err(|e| ProviderError::ModelDownload {
        model: info.name.to_string(),
        reason: e.to_string(),
    })?;
    f.read_exact(&mut hdr)
        .map_err(|e| ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!("cannot read header: {e}"),
        })?;

    let magic_ok = matches!(
        &hdr,
        b"ggml"
            | b"lmgg"
            | b"ggmf"
            | b"fmgg"
            | b"ggjt"
            | b"tjgg"
            | b"ggjf"
            | b"fjgg"
            | b"gguf"
            | b"fugg"
            | b"GGUF"
    );

    if !magic_ok {
        // Do not delete — cache verify quarantines; leave bytes for forensics.
        return Err(ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!(
                "model header {:?} is not a recognized ggml/gguf magic — refusing to use file",
                hdr
            ),
        }
        .into());
    }

    // JOE-1645: trusted path requires a reviewed pin; never bless unpinned files.
    let Some(expected) = pinned_sha256(info.filename) else {
        return Err(ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!(
                "no reviewed SHA-256 pin for {} — not trusted",
                info.filename
            ),
        }
        .into());
    };
    if !verify_against_expected(path, expected) {
        return Err(ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!(
                "cached model failed pinned sha256 check ({expected}); \
                 run `aurum cache verify` / quarantine repair"
            ),
        }
        .into());
    }
    if let Some(exact) = pinned_exact_bytes(info.filename) {
        if meta.len() != exact {
            return Err(ProviderError::ModelDownload {
                model: info.name.to_string(),
                reason: format!(
                    "cached model size mismatch (got {}, expected {exact})",
                    meta.len()
                ),
            }
            .into());
        }
    }

    Ok(())
}

fn verify_against_expected(path: &Path, expected: &str) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    hex::encode(hasher.finalize()) == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_known_models() {
        assert_eq!(lookup_model("base").unwrap().filename, "ggml-base.bin");
        assert_eq!(
            lookup_model("tiny-q5_1").unwrap().filename,
            "ggml-tiny-q5_1.bin"
        );
        assert_eq!(
            lookup_model("large-v3-turbo").unwrap().filename,
            "ggml-large-v3-turbo.bin"
        );
        assert_eq!(lookup_model("turbo").unwrap().name, "turbo");
        assert!(lookup_model("nope").is_err());
    }

    #[test]
    fn model_path_joins() {
        let p = model_path(Path::new("/tmp/cache"), lookup_model("tiny").unwrap());
        assert_eq!(p, PathBuf::from("/tmp/cache/models/ggml-tiny.bin"));
    }

    #[test]
    fn list_includes_quantized() {
        let list = format_model_list(Path::new("/tmp/aurum-cache-test"));
        assert!(list.contains("tiny-q5_1"));
        assert!(list.contains("base-q5_1"));
        assert!(list.contains("first run"));
    }

    #[test]
    fn every_catalogue_file_has_exact_size_metadata() {
        // JOE-1590: exact size is required reviewed metadata for every unique file.
        let mut missing = Vec::new();
        for m in MODELS {
            if pinned_exact_bytes(m.filename).is_none() {
                missing.push(m.filename);
            }
        }
        assert!(
            missing.is_empty(),
            "missing exact_bytes pins for: {missing:?}"
        );
    }

    #[test]
    fn reviewed_sha256_pins_cover_every_catalogue_filename() {
        // JOE-1645: every trusted entry must have an immutable digest.
        let mut missing = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for m in MODELS {
            if !seen.insert(m.filename) {
                continue; // aliases share identity
            }
            if pinned_sha256(m.filename).is_none() {
                missing.push(m.filename);
            } else {
                let pin = pinned_sha256(m.filename).unwrap();
                assert_eq!(pin.len(), 64, "pin length for {}", m.filename);
            }
        }
        assert!(missing.is_empty(), "missing sha256 pins for: {missing:?}");
    }

    #[test]
    fn aliases_share_canonical_artifact_pins() {
        let large = lookup_model("large").unwrap();
        let large_v3 = lookup_model("large-v3").unwrap();
        assert_eq!(large.filename, large_v3.filename);
        assert_eq!(
            pinned_sha256(large.filename),
            pinned_sha256(large_v3.filename)
        );
    }

    #[test]
    fn large_v3_q5_0_is_experimental_tier() {
        assert_eq!(
            model_support_tier("large-v3-q5_0"),
            ModelSupportTier::Experimental
        );
        assert_eq!(model_support_tier("base"), ModelSupportTier::Supported);
    }
}
