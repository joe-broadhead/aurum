//! Local whisper.cpp model management: resolve, download, cache, list.

use crate::error::{EnvironmentError, ProviderError, Result, UserError};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// HuggingFace repo hosting official ggml whisper.cpp models.
const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

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

/// Ensure a model is present locally, downloading if needed. Returns the path.
pub async fn ensure_model(
    cache_dir: &Path,
    model_name: &str,
    show_progress: bool,
) -> Result<PathBuf> {
    let info = lookup_model(model_name)?;
    let path = model_path(cache_dir, info);

    if path.exists()
        && path
            .metadata()
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false)
    {
        if verify_model_basic(&path, info).is_ok() && verify_cached_checksum(&path) {
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

    if show_progress {
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

    if show_progress {
        eprintln!(
            "aurum: downloading model `{}` ({}) — first run only …",
            info.name,
            format_bytes(info.approx_bytes)
        );
    }

    let result = download_model(info, &path, show_progress).await;
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

async fn download_model(info: &ModelInfo, dest: &Path, show_progress: bool) -> Result<()> {
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

    let client = reqwest::Client::builder()
        .user_agent(concat!("aurum-core/", env!("CARGO_PKG_VERSION")))
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
            .unwrap()
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
        if let Some(pb) = &pb {
            pb.set_position(downloaded.min(total));
        }
    }

    file.flush().ok();
    drop(file);

    fs::rename(&partial, dest).map_err(|e| EnvironmentError::DirectoryAccess {
        path: dest.display().to_string(),
        reason: e.to_string(),
    })?;

    if let Some(pb) = pb {
        pb.finish_with_message(format!("Downloaded {} ({downloaded} bytes)", info.name));
    }

    let digest = hex::encode(hasher.finalize());
    tracing::info!(
        model = info.name,
        sha256 = %digest,
        bytes = downloaded,
        "model download complete"
    );

    let checksum_path = dest.with_extension("bin.sha256");
    let _ = fs::write(&checksum_path, format!("{digest}  {}\n", info.filename));

    Ok(())
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
        tracing::warn!(
            model = info.name,
            header = ?hdr,
            "model header did not match expected ggml/gguf magic; continuing anyway"
        );
    }

    Ok(())
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
}
