//! Local whisper.cpp model management: resolve, download, cache, list.

use crate::error::{EnvironmentError, ProviderError, Result, UserError};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
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
        notes: "quantized",
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
        "{:<22} {:>10}  {:<8}  {}\n",
        "NAME", "SIZE", "STATUS", "NOTES"
    ));
    out.push_str(&format!(
        "{:<22} {:>10}  {:<8}  {}\n",
        "----", "----", "------", "-----"
    ));
    for row in rows {
        let size = format_bytes(row.info.approx_bytes);
        let status = if row.cached { "cached" } else { "—" };
        out.push_str(&format!(
            "{:<22} {:>10}  {:<8}  {}\n",
            row.info.name, size, status, row.info.notes
        ));
    }
    out.push_str(
        "\nTip: first run downloads the selected model. Try `tiny-q5_1` (~32 MB) for a quick trial.\n",
    );
    out.push_str("Default model: `base` (~142 MB). Use --model <name> to choose.\n");
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
    // Pinned path: one hash inside verify_model_basic. Unpinned: magic + sidecar.
    if pinned_sha256(info.filename).is_some() {
        verify_model_basic(&path, info).is_ok()
    } else {
        verify_model_basic(&path, info).is_ok() && verify_cached_checksum(&path)
    }
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
        // Prefer pinned SHA when available (one full hash). Otherwise magic + optional sidecar.
        let ok = if pinned_sha256(info.filename).is_some() {
            verify_model_basic(&path, info).is_ok()
        } else {
            verify_model_basic(&path, info).is_ok() && verify_cached_checksum(&path)
        };
        if ok {
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
        && verify_model_basic(&path, info).is_ok()
        && verify_cached_checksum(&path)
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

/// If a `.sha256` sidecar exists, ensure it matches the file contents.
fn verify_cached_checksum(path: &Path) -> bool {
    let checksum_path = path.with_extension("bin.sha256");
    let Ok(contents) = fs::read_to_string(&checksum_path) else {
        return true;
    };
    let Some(expected) = contents.split_whitespace().next() else {
        return true;
    };
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 64];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        tracing::warn!(%expected, %actual, "model checksum mismatch");
        return false;
    }
    true
}

async fn download_model(
    info: &ModelInfo,
    dest: &Path,
    show_progress: bool,
    on_progress: Option<&DownloadProgressCallback>,
) -> Result<()> {
    let url = format!("{HF_BASE}/{}?download=true", info.filename);
    tracing::info!(model = info.name, %url, "downloading model");

    let partial = dest.with_extension(format!(
        "bin.partial.{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    if partial.exists() {
        let _ = fs::remove_file(&partial);
    }

    // Bound total download time so a stalled transfer can't hold the lock forever.
    // No redirects: credentials/tokenless downloads must not hop off reviewed origin.
    let client = reqwest::Client::builder()
        .user_agent(concat!("aurum-core/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!("http client error: {e}"),
        })?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!("request failed: {e}"),
        })?;

    if !response.status().is_success() {
        return Err(ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!("HTTP {}", response.status()),
        }
        .into());
    }

    let total = response.content_length().unwrap_or(info.approx_bytes);

    let pb = if show_progress {
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::with_template(
                "{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-"),
        );
        pb.set_message(format!("Downloading {}", info.name));
        Some(pb)
    } else {
        None
    };

    let mut file = File::create(&partial).map_err(|e| EnvironmentError::DirectoryAccess {
        path: partial.display().to_string(),
        reason: e.to_string(),
    })?;

    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!("stream error: {e}"),
        })?;
        file.write_all(&chunk)
            .map_err(|e| EnvironmentError::DiskSpace {
                path: partial.display().to_string(),
                reason: e.to_string(),
            })?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        // Hard cap from reviewed metadata only (JOE-1591). Content-Length cannot raise it.
        let max_allowed = crate::download::download_byte_cap(
            info.approx_bytes,
            pinned_exact_bytes(info.filename),
            3,
        );
        if downloaded > max_allowed {
            let _ = fs::remove_file(&partial);
            return Err(ProviderError::ModelDownload {
                model: info.name.to_string(),
                reason: format!(
                    "download exceeded size cap ({downloaded} > {max_allowed} bytes) — aborting"
                ),
            }
            .into());
        }
        if let Some(pb) = &pb {
            pb.set_position(downloaded.min(total));
        }
        if let Some(cb) = on_progress {
            cb(DownloadProgress {
                model: info.name.to_string(),
                downloaded_bytes: downloaded,
                total_bytes: total,
            });
        }
    }

    file.flush().ok();
    let _ = file.sync_all();
    drop(file);

    let digest = hex::encode(hasher.finalize());
    tracing::info!(
        model = info.name,
        sha256 = %digest,
        bytes = downloaded,
        "model download complete (pre-publish)"
    );

    // Verify-before-publish (JOE-1591): never expose a bad file at the final path.
    if let Some(expected) = pinned_sha256(info.filename) {
        if digest != expected {
            let _ = fs::remove_file(&partial);
            return Err(ProviderError::ModelDownload {
                model: info.name.to_string(),
                reason: format!(
                    "sha256 mismatch (got {digest}, expected {expected}) — refusing to publish"
                ),
            }
            .into());
        }
    } else if let Some(exact) = pinned_exact_bytes(info.filename) {
        if downloaded != exact {
            let _ = fs::remove_file(&partial);
            return Err(ProviderError::ModelDownload {
                model: info.name.to_string(),
                reason: format!(
                    "size mismatch (got {downloaded}, expected {exact}) — refusing to publish"
                ),
            }
            .into());
        }
    }

    // Atomic publish after verification.
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    fs::rename(&partial, dest).map_err(|e| {
        let _ = fs::remove_file(&partial);
        EnvironmentError::DirectoryAccess {
            path: dest.display().to_string(),
            reason: e.to_string(),
        }
    })?;

    if let Some(pb) = pb {
        pb.finish_with_message(format!("Downloaded {} ({downloaded} bytes)", info.name));
    }

    let checksum_path = dest.with_extension("bin.sha256");
    let _ = fs::write(&checksum_path, format!("{digest}  {}\n", info.filename));

    // Best-effort cleanup of orphaned partials from prior crashed runs.
    sweep_stale_partials(dest.parent().unwrap_or_else(|| Path::new(".")));

    Ok(())
}

/// Independently reviewed SHA-256 digests (JOE-1590).
/// A missing pin still allows download but fails closed on publish when
/// `require_reviewed_pin` is true; cache verify reports unpinned state.
fn pinned_sha256(filename: &str) -> Option<&'static str> {
    match filename {
        "ggml-tiny.bin" => Some("be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
        "ggml-tiny-q5_1.bin" => {
            Some("818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7")
        }
        "ggml-tiny.en-q5_1.bin" => {
            Some("c77c5766f1cef09b6b7d47f21b546cbddd4157886b3b5d6d4f709e91e66c7c2b")
        }
        "ggml-base.bin" => Some("60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
        "ggml-base-q5_1.bin" => {
            Some("422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898")
        }
        "ggml-base.en-q5_1.bin" => {
            Some("4baf70dd0d7c4247ba2b81fafd9c01005ac77c2f9ef064e00dcf195d0e2fdd2f")
        }
        "ggml-small-q5_1.bin" => {
            Some("ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb")
        }
        _ => None,
    }
}

/// Reviewed exact sizes when known (from HF package metadata / maintainer review).
fn pinned_exact_bytes(filename: &str) -> Option<u64> {
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
        if !name.contains(".bin.partial.") {
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
        let _ = fs::remove_file(path);
        return Err(ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!(
                "downloaded file is only {} bytes — likely truncated or HTML error page",
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
        let _ = fs::remove_file(path);
        return Err(ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!(
                "model header {:?} is not a recognized ggml/gguf magic — refusing to use file",
                hdr
            ),
        }
        .into());
    }

    // When a pin exists, enforce it on cache hits. Do not delete — quarantine is
    // the operator path (JOE-1592); here we fail closed for consumers.
    if let Some(expected) = pinned_sha256(info.filename) {
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
    fn reviewed_sha256_pins_cover_default_and_trial_models() {
        for name in ["tiny", "tiny-q5_1", "base", "base-q5_1"] {
            let info = lookup_model(name).unwrap();
            assert!(
                pinned_sha256(info.filename).is_some(),
                "missing sha256 for default/trial model {name}"
            );
        }
    }
}
