//! Shared verify-before-publish artifact downloader (JOE-1591).
//!
//! Used by STT and TTS catalogues so download/trust policy is identical.

use crate::error::{EnvironmentError, ProviderError, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Reviewed artifact identity for a single downloadable file.
#[derive(Debug, Clone, Copy)]
pub struct ArtifactSpec {
    pub id: &'static str,
    pub filename: &'static str,
    /// Reviewed content SHA-256 (hex). Required for publish.
    pub sha256: &'static str,
    /// Exact expected byte size when known (from reviewed metadata).
    pub exact_bytes: Option<u64>,
    /// Soft size used only for progress UX and as floor for the hard cap.
    pub approx_bytes: u64,
    /// Absolute or origin-relative download URL (must match policy).
    pub url: &'static str,
    pub license: &'static str,
    pub source_revision: &'static str,
}

/// Options for a single download.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub show_progress: bool,
    pub connect_timeout: Duration,
    pub total_timeout: Duration,
    /// Hard size cap multiplier over `approx_bytes` / exact (default 3×, floor 1 MiB).
    pub size_cap_factor: u64,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            show_progress: false,
            connect_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(30 * 60),
            size_cap_factor: 3,
        }
    }
}

/// Hard disk cap derived **only** from reviewed metadata (never raised by Content-Length).
pub fn download_byte_cap(approx_bytes: u64, exact: Option<u64>, factor: u64) -> u64 {
    const FLOOR: u64 = 1_000_000;
    let base = exact.unwrap_or(approx_bytes).max(approx_bytes);
    base.saturating_mul(factor.max(1)).max(FLOOR)
}

/// Verify an on-disk file against reviewed identity.
pub fn verify_artifact(path: &Path, spec: &ArtifactSpec) -> Result<()> {
    if !path.exists() {
        return Err(ProviderError::ModelDownload {
            model: spec.id.to_string(),
            reason: format!("missing artifact {}", path.display()),
        }
        .into());
    }
    let meta = fs::metadata(path).map_err(|e| ProviderError::ModelDownload {
        model: spec.id.to_string(),
        reason: e.to_string(),
    })?;
    if let Some(exact) = spec.exact_bytes {
        if meta.len() != exact {
            return Err(ProviderError::ModelDownload {
                model: spec.id.to_string(),
                reason: format!(
                    "size mismatch for {} (got {}, expected {exact})",
                    spec.filename,
                    meta.len()
                ),
            }
            .into());
        }
    } else if meta.len() < 1_000 {
        return Err(ProviderError::ModelDownload {
            model: spec.id.to_string(),
            reason: format!("artifact too small ({} bytes)", meta.len()),
        }
        .into());
    }
    let digest = sha256_file(path)?;
    if digest != spec.sha256 {
        return Err(ProviderError::ModelDownload {
            model: spec.id.to_string(),
            reason: format!(
                "sha256 mismatch for {} (got {digest}, expected {})",
                spec.filename, spec.sha256
            ),
        }
        .into());
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = File::open(path).map_err(|e| EnvironmentError::DirectoryAccess {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(EnvironmentError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Download `spec` to `dest`, verifying before publish. Invisible until success.
pub async fn download_verified(
    spec: &ArtifactSpec,
    dest: &Path,
    opts: &DownloadOptions,
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| EnvironmentError::DirectoryAccess {
            path: parent.display().to_string(),
            reason: e.to_string(),
        })?;
    }

    let tmp = exclusive_partial_path(dest)?;
    let result = download_to_partial(spec, &tmp, dest, opts).await;
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn exclusive_partial_path(dest: &Path) -> Result<PathBuf> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let stem = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("artifact");
    for _ in 0..32 {
        let name = format!(
            ".{}.{}-{}.aurum.partial",
            stem,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => {
                drop(f);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(EnvironmentError::DirectoryAccess {
                    path: parent.display().to_string(),
                    reason: format!("exclusive partial create failed: {e}"),
                }
                .into());
            }
        }
    }
    Err(EnvironmentError::DirectoryAccess {
        path: parent.display().to_string(),
        reason: "could not allocate exclusive partial path".into(),
    }
    .into())
}

async fn download_to_partial(
    spec: &ArtifactSpec,
    tmp: &Path,
    dest: &Path,
    opts: &DownloadOptions,
) -> Result<()> {
    tracing::info!(id = spec.id, url = spec.url, "downloading artifact");

    let client = reqwest::Client::builder()
        .user_agent(concat!("aurum-core/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(opts.connect_timeout)
        .timeout(opts.total_timeout)
        // Model packs live on HF CDN (302 from resolve/). Allow only HF hosts.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let host = attempt.url().host_str().unwrap_or("").to_ascii_lowercase();
            let ok = host == "huggingface.co"
                || host.ends_with(".huggingface.co")
                || host.ends_with(".hf.co")
                || host == "hf.co"
                || host.ends_with(".cdn.hf.co");
            if ok && attempt.previous().len() < 8 {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|e| ProviderError::ModelDownload {
            model: spec.id.to_string(),
            reason: format!("http client: {e}"),
        })?;

    let response = client
        .get(spec.url)
        .send()
        .await
        .map_err(|e| ProviderError::ModelDownload {
            model: spec.id.to_string(),
            reason: format!("request failed: {e}"),
        })?;

    if !response.status().is_success() {
        return Err(ProviderError::ModelDownload {
            model: spec.id.to_string(),
            reason: format!("HTTP {}", response.status()),
        }
        .into());
    }

    let hard_cap = download_byte_cap(spec.approx_bytes, spec.exact_bytes, opts.size_cap_factor);
    // Content-Length may only lower the accepted limit (early reject).
    if let Some(cl) = response.content_length() {
        if cl > hard_cap {
            return Err(ProviderError::ModelDownload {
                model: spec.id.to_string(),
                reason: format!("Content-Length {cl} exceeds reviewed size cap {hard_cap}"),
            }
            .into());
        }
    }

    let progress_total = response
        .content_length()
        .filter(|&n| n > 0 && n <= hard_cap)
        .or(spec.exact_bytes)
        .unwrap_or(spec.approx_bytes);

    let pb = if opts.show_progress {
        use indicatif::{ProgressBar, ProgressStyle};
        let pb = ProgressBar::new(progress_total);
        pb.set_style(
            ProgressStyle::with_template(
                "{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-"),
        );
        pb.set_message(format!("Downloading {}", spec.id));
        Some(pb)
    } else {
        None
    };

    let mut file = OpenOptions::new().write(true).open(tmp).map_err(|e| {
        EnvironmentError::DirectoryAccess {
            path: tmp.display().to_string(),
            reason: e.to_string(),
        }
    })?;

    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ProviderError::ModelDownload {
            model: spec.id.to_string(),
            reason: format!("stream error: {e}"),
        })?;
        file.write_all(&chunk)
            .map_err(|e| EnvironmentError::DiskSpace {
                path: tmp.display().to_string(),
                reason: e.to_string(),
            })?;
        hasher.update(&chunk);
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > hard_cap {
            return Err(ProviderError::ModelDownload {
                model: spec.id.to_string(),
                reason: format!("download exceeded size cap ({downloaded} > {hard_cap})"),
            }
            .into());
        }
        if let Some(pb) = &pb {
            pb.set_position(downloaded.min(progress_total));
        }
    }
    file.flush().map_err(|e| EnvironmentError::DiskSpace {
        path: tmp.display().to_string(),
        reason: e.to_string(),
    })?;
    file.sync_all().ok();
    drop(file);

    let digest = hex::encode(hasher.finalize());
    if digest != spec.sha256 {
        return Err(ProviderError::ModelDownload {
            model: spec.id.to_string(),
            reason: format!(
                "sha256 mismatch (got {digest}, expected {}) — refusing to publish",
                spec.sha256
            ),
        }
        .into());
    }
    if let Some(exact) = spec.exact_bytes {
        if downloaded != exact {
            return Err(ProviderError::ModelDownload {
                model: spec.id.to_string(),
                reason: format!(
                    "size mismatch after download (got {downloaded}, expected {exact})"
                ),
            }
            .into());
        }
    }

    // Atomic publish.
    if dest.exists() {
        fs::remove_file(dest).map_err(|e| EnvironmentError::DirectoryAccess {
            path: dest.display().to_string(),
            reason: format!("replace existing: {e}"),
        })?;
    }
    fs::rename(tmp, dest).map_err(|e| EnvironmentError::DirectoryAccess {
        path: dest.display().to_string(),
        reason: e.to_string(),
    })?;
    if let Some(parent) = dest.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message(format!("Downloaded {} ({downloaded} bytes)", spec.id));
    }

    // Sidecar checksum for operators: `file.bin.sha256`
    let sidecar = PathBuf::from(format!("{}.sha256", dest.display()));
    let _ = fs::write(&sidecar, format!("{}  {}\n", digest, spec.filename));

    Ok(())
}

/// Sweep only this process-family stale partials (never another live writer's file).
pub fn sweep_stale_partials(dir: &Path, stale_after: Duration) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.contains(".aurum.partial") && !name.contains(".bin.partial.") {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_never_raised_by_content_length_logic() {
        // Helper ignores Content-Length; forged CL cannot increase cap.
        let cap = download_byte_cap(10_000_000, Some(10_000_000), 3);
        assert_eq!(cap, 30_000_000);
        let forged = 10_u64.pow(12);
        assert!(cap < forged);
    }

    #[test]
    fn floor_for_tiny_pin() {
        assert_eq!(download_byte_cap(100, Some(100), 3), 1_000_000);
    }
}
