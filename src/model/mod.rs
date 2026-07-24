//! Local whisper.cpp model management: resolve, download, cache.

use crate::error::{EnvironmentError, ProviderError, Result, UserError};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// HuggingFace repo hosting official ggml whisper.cpp models.
const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Known local model names and their ggml filenames.
#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    pub name: &'static str,
    pub filename: &'static str,
    /// Approximate download size in bytes (for progress UX; not a hard requirement).
    pub approx_bytes: u64,
}

/// Models supported in v0.0.0.
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "tiny",
        filename: "ggml-tiny.bin",
        approx_bytes: 75_000_000,
    },
    ModelInfo {
        name: "tiny.en",
        filename: "ggml-tiny.en.bin",
        approx_bytes: 75_000_000,
    },
    ModelInfo {
        name: "base",
        filename: "ggml-base.bin",
        approx_bytes: 142_000_000,
    },
    ModelInfo {
        name: "base.en",
        filename: "ggml-base.en.bin",
        approx_bytes: 142_000_000,
    },
    ModelInfo {
        name: "small",
        filename: "ggml-small.bin",
        approx_bytes: 466_000_000,
    },
    ModelInfo {
        name: "small.en",
        filename: "ggml-small.en.bin",
        approx_bytes: 466_000_000,
    },
    ModelInfo {
        name: "medium",
        filename: "ggml-medium.bin",
        approx_bytes: 1_500_000_000,
    },
    ModelInfo {
        name: "medium.en",
        filename: "ggml-medium.en.bin",
        approx_bytes: 1_500_000_000,
    },
    ModelInfo {
        name: "large-v3",
        filename: "ggml-large-v3.bin",
        approx_bytes: 3_100_000_000,
    },
    ModelInfo {
        name: "large-v3-turbo",
        filename: "ggml-large-v3-turbo.bin",
        approx_bytes: 1_600_000_000,
    },
    // Alias commonly used by users
    ModelInfo {
        name: "large",
        filename: "ggml-large-v3.bin",
        approx_bytes: 3_100_000_000,
    },
    ModelInfo {
        name: "turbo",
        filename: "ggml-large-v3-turbo.bin",
        approx_bytes: 1_600_000_000,
    },
];

pub fn available_model_names() -> String {
    // Deduplicate display names preferring canonical ones
    let names = [
        "tiny",
        "tiny.en",
        "base",
        "base.en",
        "small",
        "small.en",
        "medium",
        "medium.en",
        "large-v3",
        "large-v3-turbo",
    ];
    names.join(", ")
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
        // Re-verify basic integrity (and checksum when we previously wrote one).
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

    download_model(info, &path, show_progress).await?;
    verify_model_basic(&path, info)?;
    Ok(path)
}

/// If a `.sha256` sidecar exists, ensure it matches the file contents.
/// Missing sidecar → treat as ok (older caches / first run before checksum feature).
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
    use std::io::Read;
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

    // Unique partial name avoids two concurrent downloads clobbering each other.
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
        .user_agent(concat!("aurum/", env!("CARGO_PKG_VERSION")))
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

    // Atomic-ish replace
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

    // Persist checksum alongside the model for future integrity checks.
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

    // Tiny models are ~75MB; reject obviously truncated files.
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

    // ggml files store the fourcc as a little-endian u32, so on disk the bytes of
    // "ggml" appear as b"lmgg". Accept both orderings plus gguf.
    let mut hdr = [0u8; 4];
    let mut f = File::open(path).map_err(|e| ProviderError::ModelDownload {
        model: info.name.to_string(),
        reason: e.to_string(),
    })?;
    use std::io::Read;
    f.read_exact(&mut hdr)
        .map_err(|e| ProviderError::ModelDownload {
            model: info.name.to_string(),
            reason: format!("cannot read header: {e}"),
        })?;

    let magic_ok = matches!(
        &hdr,
        b"ggml"
            | b"lmgg" // LE on-disk form of "ggml"
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
}
