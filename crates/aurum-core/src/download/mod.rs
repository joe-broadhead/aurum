//! Shared verify-before-publish artifact downloader (JOE-1591).
//!
//! STT (`model/`) and TTS (`tts/catalogue.rs`) both route through
//! [`download_verified_request`]. This module owns: exclusive partials, size
//! caps from reviewed metadata only, HF/GitHub redirect policy, disk-budget
//! preflight, streaming SHA-256, and durable publish.

use crate::error::{EnvironmentError, ProviderError, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Reviewed artifact identity for a single downloadable file (static catalogue).
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

/// Runtime download identity (STT/TTS map pins into this).
#[derive(Debug, Clone, Copy)]
pub struct DownloadRequest<'a> {
    pub id: &'a str,
    pub filename: &'a str,
    pub sha256: &'a str,
    pub exact_bytes: Option<u64>,
    pub approx_bytes: u64,
    pub url: &'a str,
}

impl ArtifactSpec {
    pub fn request(&self) -> DownloadRequest<'_> {
        DownloadRequest {
            id: self.id,
            filename: self.filename,
            sha256: self.sha256,
            exact_bytes: self.exact_bytes,
            approx_bytes: self.approx_bytes,
            url: self.url,
        }
    }
}

/// Byte progress callback: `(downloaded_bytes, advisory_total_bytes)`.
pub type DownloadByteProgress = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Options for a single download.
#[derive(Clone)]
pub struct DownloadOptions {
    pub show_progress: bool,
    pub connect_timeout: Duration,
    pub total_timeout: Duration,
    /// Hard size cap multiplier over `approx_bytes` / exact (default 3×, floor 1 MiB).
    pub size_cap_factor: u64,
    /// Optional progress hook (CLI/library progress bars).
    pub on_progress: Option<DownloadByteProgress>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            show_progress: false,
            connect_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(30 * 60),
            size_cap_factor: 3,
            on_progress: None,
        }
    }
}

impl std::fmt::Debug for DownloadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadOptions")
            .field("show_progress", &self.show_progress)
            .field("connect_timeout", &self.connect_timeout)
            .field("total_timeout", &self.total_timeout)
            .field("size_cap_factor", &self.size_cap_factor)
            .field("on_progress", &self.on_progress.is_some())
            .finish()
    }
}

/// Hard disk cap derived **only** from reviewed metadata (never raised by Content-Length).
pub fn download_byte_cap(approx_bytes: u64, exact: Option<u64>, factor: u64) -> u64 {
    const FLOOR: u64 = 1_000_000;
    let base = exact.unwrap_or(approx_bytes).max(approx_bytes);
    base.saturating_mul(factor.max(1)).max(FLOOR)
}

/// Extra free-space headroom beyond the hard download cap (disk pressure).
const DISK_HEADROOM_BYTES: u64 = 64 * 1024 * 1024;

/// Best-effort free bytes for the filesystem containing `path`.
///
/// Returns `None` when the platform probe is unavailable — callers must not
/// treat that as infinite space; they still rely on write failures + size caps.
pub fn available_disk_bytes(path: &Path) -> Option<u64> {
    available_disk_bytes_inner(path)
}

#[cfg(unix)]
fn available_disk_bytes_inner(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut buf: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut buf) };
    if rc != 0 {
        return None;
    }
    // f_frsize / f_bsize / f_bavail are `u64` on some platforms and `c_ulong` on
    // others — cast through u64 for a single portable product.
    #[allow(clippy::unnecessary_cast)]
    let fr = {
        let frsize = buf.f_frsize as u64;
        let bsize = buf.f_bsize as u64;
        if frsize > 0 {
            frsize
        } else {
            bsize
        }
    };
    if fr == 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)]
    let avail = buf.f_bavail as u64;
    Some(avail.saturating_mul(fr))
}

#[cfg(windows)]
fn available_disk_bytes_inner(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lp_directory_name: *const u16,
            lp_free_bytes_available_to_caller: *mut u64,
            lp_total_number_of_bytes: *mut u64,
            lp_total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_to_caller: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_to_caller,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        None
    } else {
        Some(free_to_caller)
    }
}

#[cfg(not(any(unix, windows)))]
fn available_disk_bytes_inner(_path: &Path) -> Option<u64> {
    None
}

/// Fail closed when free space is known and below `need_bytes` + headroom.
pub fn ensure_disk_budget(path: &Path, need_bytes: u64) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(path);
    // Ensure the parent exists so the probe hits the right volume.
    if !parent.exists() {
        let _ = fs::create_dir_all(parent);
    }
    let probe = if parent.exists() { parent } else { path };
    let Some(free) = available_disk_bytes(probe) else {
        return Ok(());
    };
    let required = need_bytes.saturating_add(DISK_HEADROOM_BYTES);
    if free < required {
        return Err(EnvironmentError::DiskSpace {
            path: probe.display().to_string(),
            reason: format!(
                "insufficient free space: have {free} bytes, need ~{required} \
                 (download budget {need_bytes} + {DISK_HEADROOM_BYTES} headroom)"
            ),
        }
        .into());
    }
    Ok(())
}

/// Verify an on-disk file against reviewed identity.
pub fn verify_artifact(path: &Path, spec: &ArtifactSpec) -> Result<()> {
    verify_artifact_request(path, &spec.request())
}

/// Verify an on-disk file against a runtime download request.
pub fn verify_artifact_request(path: &Path, req: &DownloadRequest<'_>) -> Result<()> {
    if !path.exists() {
        return Err(ProviderError::ModelDownload {
            model: req.id.to_string(),
            reason: format!("missing artifact {}", path.display()),
        }
        .into());
    }
    let meta = fs::metadata(path).map_err(|e| ProviderError::ModelDownload {
        model: req.id.to_string(),
        reason: e.to_string(),
    })?;
    if let Some(exact) = req.exact_bytes {
        if meta.len() != exact {
            return Err(ProviderError::ModelDownload {
                model: req.id.to_string(),
                reason: format!(
                    "size mismatch for {} (got {}, expected {exact})",
                    req.filename,
                    meta.len()
                ),
            }
            .into());
        }
    } else if meta.len() < 1_000 {
        return Err(ProviderError::ModelDownload {
            model: req.id.to_string(),
            reason: format!("artifact too small ({} bytes)", meta.len()),
        }
        .into());
    }
    let digest = sha256_file(path)?;
    if digest != req.sha256 {
        return Err(ProviderError::ModelDownload {
            model: req.id.to_string(),
            reason: format!(
                "sha256 mismatch for {} (got {digest}, expected {})",
                req.filename, req.sha256
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

/// Download static [`ArtifactSpec`] to `dest`, verifying before publish.
pub async fn download_verified(
    spec: &ArtifactSpec,
    dest: &Path,
    opts: &DownloadOptions,
) -> Result<()> {
    download_verified_request(&spec.request(), dest, opts).await
}

/// Download `req` to `dest`, verifying before publish. Invisible until success.
pub async fn download_verified_request(
    req: &DownloadRequest<'_>,
    dest: &Path,
    opts: &DownloadOptions,
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| EnvironmentError::DirectoryAccess {
            path: parent.display().to_string(),
            reason: e.to_string(),
        })?;
    }

    let hard_cap = download_byte_cap(req.approx_bytes, req.exact_bytes, opts.size_cap_factor);
    ensure_disk_budget(dest, hard_cap)?;

    let tmp = exclusive_partial_path(dest)?;
    let result = download_to_partial(req, &tmp, dest, opts, hard_cap).await;
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

/// Allow HuggingFace CDN and GitHub release asset redirects; stop otherwise.
fn artifact_redirect_policy(attempt: reqwest::redirect::Attempt<'_>) -> reqwest::redirect::Action {
    let host = attempt.url().host_str().unwrap_or("").to_ascii_lowercase();
    let ok = host == "huggingface.co"
        || host.ends_with(".huggingface.co")
        || host.ends_with(".hf.co")
        || host == "hf.co"
        || host.ends_with(".cdn.hf.co")
        || host == "github.com"
        || host == "www.github.com"
        || host.ends_with(".github.com")
        || host == "objects.githubusercontent.com"
        || host.ends_with(".githubusercontent.com")
        || host == "release-assets.githubusercontent.com";
    if ok && attempt.previous().len() < 8 {
        attempt.follow()
    } else {
        attempt.stop()
    }
}

async fn download_to_partial(
    req: &DownloadRequest<'_>,
    tmp: &Path,
    dest: &Path,
    opts: &DownloadOptions,
    hard_cap: u64,
) -> Result<()> {
    tracing::info!(id = req.id, url = req.url, "downloading artifact");

    let client = reqwest::Client::builder()
        .user_agent(concat!("aurum-core/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(opts.connect_timeout)
        .timeout(opts.total_timeout)
        .redirect(reqwest::redirect::Policy::custom(artifact_redirect_policy))
        .build()
        .map_err(|e| ProviderError::ModelDownload {
            model: req.id.to_string(),
            reason: format!("http client: {e}"),
        })?;

    let response = client
        .get(req.url)
        .send()
        .await
        .map_err(|e| ProviderError::ModelDownload {
            model: req.id.to_string(),
            reason: format!("request failed: {e}"),
        })?;

    if !response.status().is_success() {
        return Err(ProviderError::ModelDownload {
            model: req.id.to_string(),
            reason: format!("HTTP {}", response.status()),
        }
        .into());
    }

    // Content-Length may only lower the accepted limit (early reject).
    if let Some(cl) = response.content_length() {
        if cl > hard_cap {
            return Err(ProviderError::ModelDownload {
                model: req.id.to_string(),
                reason: format!("Content-Length {cl} exceeds reviewed size cap {hard_cap}"),
            }
            .into());
        }
    }

    let progress_total = response
        .content_length()
        .filter(|&n| n > 0 && n <= hard_cap)
        .or(req.exact_bytes)
        .unwrap_or(req.approx_bytes);

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
        pb.set_message(format!("Downloading {}", req.id));
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
            model: req.id.to_string(),
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
                model: req.id.to_string(),
                reason: format!("download exceeded size cap ({downloaded} > {hard_cap})"),
            }
            .into());
        }
        if let Some(pb) = &pb {
            pb.set_position(downloaded.min(progress_total));
        }
        if let Some(cb) = &opts.on_progress {
            cb(downloaded, progress_total);
        }
    }
    file.flush().map_err(|e| EnvironmentError::DiskSpace {
        path: tmp.display().to_string(),
        reason: e.to_string(),
    })?;
    // JOE-1918: durability failures must fail closed, not be ignored.
    file.sync_all().map_err(|e| EnvironmentError::DiskSpace {
        path: tmp.display().to_string(),
        reason: format!("sync partial download: {e}"),
    })?;
    drop(file);

    let digest = hex::encode(hasher.finalize());
    if digest != req.sha256 {
        return Err(ProviderError::ModelDownload {
            model: req.id.to_string(),
            reason: format!(
                "sha256 mismatch (got {digest}, expected {}) — refusing to publish",
                req.sha256
            ),
        }
        .into());
    }
    if let Some(exact) = req.exact_bytes {
        if downloaded != exact {
            return Err(ProviderError::ModelDownload {
                model: req.id.to_string(),
                reason: format!(
                    "size mismatch after download (got {downloaded}, expected {exact})"
                ),
            }
            .into());
        }
    }

    // Durable publish: rename verified partial into place.
    publish_verified_download(tmp, dest)?;
    if let Some(parent) = dest.parent() {
        if let Ok(dir) = File::open(parent) {
            dir.sync_all()
                .map_err(|e| EnvironmentError::DirectoryAccess {
                    path: parent.display().to_string(),
                    reason: format!("sync parent dir after publish: {e}"),
                })?;
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message(format!("Downloaded {} ({downloaded} bytes)", req.id));
    }

    // Sidecar checksum for operators: `file.bin.sha256`
    let sidecar = PathBuf::from(format!("{}.sha256", dest.display()));
    let _ = fs::write(&sidecar, format!("{}  {}\n", digest, req.filename));

    Ok(())
}

/// Publish a verified partial into `dest` without a durable-window gap (JOE-1918).
///
/// Prefer atomic rename over pre-delete. On platforms where rename cannot
/// replace, stage the previous file aside and restore it if the new rename fails.
fn publish_verified_download(tmp: &Path, dest: &Path) -> Result<()> {
    match fs::rename(tmp, dest) {
        Ok(()) => Ok(()),
        Err(e) if dest.exists() => {
            // Replacement path (common on Windows when dest exists).
            let backup = dest.with_extension(format!(
                "aurum.bak.{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            fs::rename(dest, &backup).map_err(|re| EnvironmentError::DirectoryAccess {
                path: dest.display().to_string(),
                reason: format!("stage previous artifact: {re} (after rename: {e})"),
            })?;
            match fs::rename(tmp, dest) {
                Ok(()) => {
                    let _ = fs::remove_file(&backup);
                    Ok(())
                }
                Err(re) => {
                    let _ = fs::rename(&backup, dest);
                    Err(EnvironmentError::DirectoryAccess {
                        path: dest.display().to_string(),
                        reason: format!("publish verified artifact: {re}"),
                    }
                    .into())
                }
            }
        }
        Err(e) => Err(EnvironmentError::DirectoryAccess {
            path: dest.display().to_string(),
            reason: e.to_string(),
        }
        .into()),
    }
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
        if !name.contains(".aurum.partial") {
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

    #[test]
    fn disk_budget_ok_when_space_unknown_or_ample() {
        let dir = tempfile::tempdir().unwrap();
        // Tiny need should pass on any real volume; unknown probe also Ok.
        ensure_disk_budget(dir.path(), 1).unwrap();
    }

    #[test]
    fn disk_budget_fails_when_need_exceeds_free() {
        let dir = tempfile::tempdir().unwrap();
        // Only assert when probe works — otherwise skip.
        if available_disk_bytes(dir.path()).is_some() {
            let err = ensure_disk_budget(dir.path(), u64::MAX / 4).unwrap_err();
            let s = err.to_string();
            assert!(
                s.contains("insufficient free space")
                    || s.contains("DiskSpace")
                    || s.contains("disk"),
                "unexpected err: {s}"
            );
        }
    }
}
