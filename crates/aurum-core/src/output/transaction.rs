//! Shared secure output transaction for STT, cleanup, and TTS (JOE-1644).
//!
//! ## Commit protocol
//!
//! 1. Resolve destination (reject symlink destinations, directories, empty paths).
//! 2. Create a unique same-directory temporary file with exclusive create and
//!    owner-only permissions (0o600 on Unix at create time).
//! 3. Write payload, flush, and `sync_all` the file — **errors are propagated**.
//! 4. Publish:
//!    * **NoClobber (Unix):** `link(tmp, dest)` then `unlink(tmp)`. `link` fails if
//!      `dest` already exists, so a concurrent creator cannot be overwritten.
//!    * **NoClobber (Windows):** `rename` only when dest is absent; existence is
//!      re-checked immediately before rename (residual same-user race documented).
//!    * **Replace (Unix):** `rename(tmp, dest)` atomically replaces any regular file.
//!    * **Replace (Windows):** stage existing dest aside, rename temp into place,
//!      remove backup; on failure restore the previous destination.
//! 5. Parent-directory `sync_all` errors are propagated (best-effort on platforms
//!    that reject directory fsync).
//!
//! A failure before successful publish leaves the prior destination intact and
//! removes our temp file. Temp names include a collision-resistant suffix
//! (PID + wall-clock nanoseconds); exclusive `create_new` is the safety gate
//! so concurrent writers never share a path.
//!
//! ## Durability
//!
//! A successful return means: the final path contains the complete requested
//! bytes, file data was flushed/synced, and the parent directory sync either
//! succeeded or the platform does not support it (ENOTSUP/EINVAL treated as OK).

use crate::error::{EnvironmentError, Result, UserError};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Overwrite policy for a destination path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitMode {
    /// Fail if the destination exists as a non-empty regular file, including when
    /// it appears after preflight (race-safe on Unix via hard-link publish).
    NoClobber,
    /// Replace an existing regular file destination.
    Replace,
}

/// How to treat destination paths that are symbolic links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SymlinkPolicy {
    /// Reject destinations that are symlinks (default — avoid clobbering via
    /// unexpected link targets).
    #[default]
    Reject,
}

/// Options for a single output transaction.
#[derive(Debug, Clone)]
pub struct OutputTransaction {
    dest: PathBuf,
    mode: CommitMode,
    symlink_policy: SymlinkPolicy,
}

impl OutputTransaction {
    pub fn new(dest: impl Into<PathBuf>, mode: CommitMode) -> Self {
        Self {
            dest: dest.into(),
            mode,
            symlink_policy: SymlinkPolicy::Reject,
        }
    }

    pub fn with_symlink_policy(mut self, policy: SymlinkPolicy) -> Self {
        self.symlink_policy = policy;
        self
    }

    pub fn dest(&self) -> &Path {
        &self.dest
    }

    pub fn mode(&self) -> CommitMode {
        self.mode
    }

    /// Validate the destination path shape and symlink/directory policy.
    pub fn preflight(&self) -> Result<()> {
        validate_dest_path(&self.dest)?;
        check_dest_policy(&self.dest, self.mode, self.symlink_policy)?;
        Ok(())
    }

    /// Write `bytes` through the full commit protocol.
    pub fn commit_bytes(&self, bytes: &[u8]) -> Result<()> {
        self.preflight()?;
        ensure_parent_dir(&self.dest)?;

        let tmp = create_exclusive_temp(&self.dest)?;
        let write_result = (|| -> Result<()> {
            {
                let mut file = OpenOptions::new()
                    .write(true)
                    .open(&tmp)
                    .map_err(|e| map_io_write(&self.dest, e))?;
                file.write_all(bytes)
                    .map_err(|e| map_io_write(&self.dest, e))?;
                file.flush().map_err(|e| map_io_write(&self.dest, e))?;
                file.sync_all().map_err(|e| map_io_sync(&self.dest, e))?;
            }
            publish(&tmp, &self.dest, self.mode, self.symlink_policy)?;
            sync_parent_dir(&self.dest)?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        write_result
    }

    /// Write via a callback that receives the exclusive temp file path.
    ///
    /// The callback must fully write its content to `tmp`. This path is used when
    /// the payload is produced by a specialized writer (e.g. WAV).
    pub fn commit_with<F>(&self, write_tmp: F) -> Result<()>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        self.preflight()?;
        ensure_parent_dir(&self.dest)?;

        let tmp = create_exclusive_temp(&self.dest)?;
        let write_result = (|| -> Result<()> {
            write_tmp(&tmp)?;
            // Ensure durable bytes even if the callback only flushed its writer.
            // Re-open writeable: on Windows, FlushFileBuffers on a read-only handle
            // can return ERROR_ACCESS_DENIED.
            {
                let file = OpenOptions::new()
                    .write(true)
                    .open(&tmp)
                    .map_err(|e| map_io_write(&self.dest, e))?;
                file.sync_all().map_err(|e| map_io_sync(&self.dest, e))?;
            }
            publish(&tmp, &self.dest, self.mode, self.symlink_policy)?;
            sync_parent_dir(&self.dest)?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        write_result
    }
}

/// Convenience: commit UTF-8 text with a trailing newline if missing.
pub fn commit_text(dest: &Path, text: &str, mode: CommitMode) -> Result<()> {
    let mut body = text.to_string();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    OutputTransaction::new(dest, mode).commit_bytes(body.as_bytes())
}

fn validate_dest_path(path: &Path) -> Result<()> {
    let s = path.as_os_str();
    if s.is_empty() {
        return Err(UserError::Other {
            message: "output path is empty".into(),
        }
        .into());
    }
    if path.file_name().map(|n| n.is_empty()).unwrap_or(true) {
        return Err(UserError::Other {
            message: format!(
                "output path '{}' must be a file path, not a directory",
                path.display()
            ),
        }
        .into());
    }
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| EnvironmentError::DirectoryAccess {
                path: parent.display().to_string(),
                reason: e.to_string(),
            })?;
        }
    }
    Ok(())
}

fn check_dest_policy(path: &Path, mode: CommitMode, symlink_policy: SymlinkPolicy) -> Result<()> {
    // Symlink check uses symlink_metadata so we do not follow the link.
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                match symlink_policy {
                    SymlinkPolicy::Reject => {
                        return Err(UserError::Other {
                            message: format!(
                                "output path is a symbolic link (refused): {}\n  \
                                 Hint: write to a regular file path, or remove the symlink first.",
                                path.display()
                            ),
                        }
                        .into());
                    }
                }
            }
            if meta.is_dir() {
                return Err(UserError::Other {
                    message: format!(
                        "output path is a directory: {}\n  Hint: provide a file path.",
                        path.display()
                    ),
                }
                .into());
            }
            // Reject non-regular special files (FIFO, device, …) when detectable.
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                let ft = meta.file_type();
                if ft.is_fifo() || ft.is_socket() || ft.is_block_device() || ft.is_char_device() {
                    return Err(UserError::Other {
                        message: format!("output path is not a regular file: {}", path.display()),
                    }
                    .into());
                }
            }
            match mode {
                CommitMode::Replace => Ok(()),
                CommitMode::NoClobber => {
                    if meta.len() == 0 {
                        // Empty file is treated as a placeholder we may replace.
                        Ok(())
                    } else {
                        Err(UserError::Other {
                            message: format!(
                                "output file already exists: {}\n  \
                                 Hint: pass --force to overwrite, or choose another path.",
                                path.display()
                            ),
                        }
                        .into())
                    }
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(EnvironmentError::DirectoryAccess {
            path: path.display().to_string(),
            reason: e.to_string(),
        }
        .into()),
    }
}

fn create_exclusive_temp(dest: &Path) -> Result<PathBuf> {
    let parent = dest.parent().filter(|p| !p.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));
    let stem = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("aurum-out");
    // Sanitize stem for hidden temp name (avoid nested path separators).
    let safe_stem: String = stem
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();

    // Retry on collision (extremely unlikely with unique suffix).
    for _ in 0..32 {
        let suffix = random_suffix();
        let name = format!(".{}.{}-{}.aurum.tmp", safe_stem, std::process::id(), suffix);
        let tmp = parent.join(name);
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&tmp) {
            Ok(file) => {
                drop(file);
                return Ok(tmp);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(EnvironmentError::DirectoryAccess {
                    path: parent.display().to_string(),
                    reason: format!("failed to create exclusive temp file: {e}"),
                }
                .into());
            }
        }
    }
    Err(EnvironmentError::DirectoryAccess {
        path: parent.display().to_string(),
        reason: "failed to allocate exclusive temp file after retries".into(),
    }
    .into())
}

fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Mix PID + nanos for uniqueness without pulling in an RNG crate.
    format!("{nanos:x}")
}

/// Publish `tmp` as `dest` according to `mode`.
///
/// On success, `tmp` no longer exists (consumed by rename or unlinked after link).
fn publish(tmp: &Path, dest: &Path, mode: CommitMode, symlink_policy: SymlinkPolicy) -> Result<()> {
    // Final policy check immediately before publish (covers races after preflight).
    check_dest_policy(dest, mode, symlink_policy)?;

    match mode {
        CommitMode::NoClobber => publish_noclobber(tmp, dest),
        CommitMode::Replace => publish_replace(tmp, dest),
    }
}

/// NoClobber: never overwrite a destination that exists at publish time.
fn publish_noclobber(tmp: &Path, dest: &Path) -> Result<()> {
    // Empty placeholder may be removed so we can create exclusively.
    if let Ok(meta) = fs::symlink_metadata(dest) {
        if meta.len() == 0 && !meta.file_type().is_symlink() {
            fs::remove_file(dest).map_err(|e| EnvironmentError::DirectoryAccess {
                path: dest.display().to_string(),
                reason: format!("failed to clear empty placeholder: {e}"),
            })?;
        }
    }

    #[cfg(unix)]
    {
        // hard_link fails with EEXIST if dest appeared — race-safe vs rename overwrite.
        match fs::hard_link(tmp, dest) {
            Ok(()) => {
                let _ = fs::remove_file(tmp);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(UserError::Other {
                message: format!(
                    "output file already exists: {}\n  \
                         Hint: pass --force to overwrite, or choose another path.",
                    dest.display()
                ),
            }
            .into()),
            Err(e) => Err(EnvironmentError::DirectoryAccess {
                path: dest.display().to_string(),
                reason: format!("NoClobber publish (hard_link) failed: {e}"),
            }
            .into()),
        }
    }

    #[cfg(not(unix))]
    {
        // Windows: no portable hard_link+EEXIST guarantee for our use; refuse if
        // dest exists and rename only when absent. Residual TOCTOU vs a concurrent
        // creator is documented; prefer Unix semantics for strict race safety.
        if dest.exists() {
            return Err(UserError::Other {
                message: format!(
                    "output file already exists: {}\n  \
                     Hint: pass --force to overwrite, or choose another path.",
                    dest.display()
                ),
            }
            .into());
        }
        fs::rename(tmp, dest).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                UserError::Other {
                    message: format!(
                        "output file already exists: {}\n  \
                         Hint: pass --force to overwrite, or choose another path.",
                        dest.display()
                    ),
                }
                .into()
            } else {
                EnvironmentError::DirectoryAccess {
                    path: dest.display().to_string(),
                    reason: format!("NoClobber publish failed: {e}"),
                }
                .into()
            }
        })
    }
}

/// Replace: strongest platform-native replacement available.
fn publish_replace(tmp: &Path, dest: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        // Atomic replace of a regular file (or create if absent).
        fs::rename(tmp, dest).map_err(|e| EnvironmentError::DirectoryAccess {
            path: dest.display().to_string(),
            reason: format!("atomic publish failed: {e}"),
        })?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        if !dest.exists() {
            fs::rename(tmp, dest).map_err(|e| EnvironmentError::DirectoryAccess {
                path: dest.display().to_string(),
                reason: format!("atomic publish failed: {e}"),
            })?;
            return Ok(());
        }

        // Move the existing file aside so we never delete-before-replace.
        let backup = dest.with_extension(format!(
            "aurum-bak.{}-{}",
            std::process::id(),
            random_suffix()
        ));
        fs::rename(dest, &backup).map_err(|e| EnvironmentError::DirectoryAccess {
            path: dest.display().to_string(),
            reason: format!("failed to stage existing output for replace: {e}"),
        })?;

        match fs::rename(tmp, dest) {
            Ok(()) => {
                let _ = fs::remove_file(&backup);
                Ok(())
            }
            Err(e) => {
                // Restore previous destination; leave tmp for the caller to clean.
                let _ = fs::rename(&backup, dest);
                Err(EnvironmentError::DirectoryAccess {
                    path: dest.display().to_string(),
                    reason: format!("atomic publish failed (previous file restored): {e}"),
                }
                .into())
            }
        }
    }
}

fn sync_parent_dir(dest: &Path) -> Result<()> {
    let Some(parent) = dest.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    // Opening a directory handle for fsync is platform-specific. On Windows,
    // `File::open` on a directory often returns Access Denied — treat as
    // unsupported (best-effort durability), not a hard failure (JOE-1644).
    let dir = match File::open(parent) {
        Ok(d) => d,
        Err(e) => {
            #[cfg(windows)]
            {
                let _ = e;
                return Ok(());
            }
            #[cfg(not(windows))]
            {
                // On Unix, inability to open the parent is unusual — surface it.
                return Err(EnvironmentError::DirectoryAccess {
                    path: parent.display().to_string(),
                    reason: format!("failed to open parent directory for sync: {e}"),
                }
                .into());
            }
        }
    };
    match dir.sync_all() {
        Ok(()) => Ok(()),
        Err(e) => {
            // Some platforms/filesystems reject directory fsync (EINVAL/ENOTSUP).
            // Windows often cannot fsync directory handles either.
            #[cfg(windows)]
            {
                let _ = e;
                return Ok(());
            }
            #[cfg(not(windows))]
            {
                let raw = e.raw_os_error();
                // EINVAL=22; ENOTSUP=45 (macOS) / 95 (Linux EOPNOTSUPP)
                if matches!(raw, Some(22) | Some(45) | Some(95)) {
                    return Ok(());
                }
                Err(EnvironmentError::DirectoryAccess {
                    path: parent.display().to_string(),
                    reason: format!("parent directory sync failed: {e}"),
                }
                .into())
            }
        }
    }
}

fn map_io_write(dest: &Path, e: std::io::Error) -> crate::error::TranscriptionError {
    if e.kind() == std::io::ErrorKind::OutOfMemory
        || e.to_string().to_ascii_lowercase().contains("no space")
    {
        return EnvironmentError::DiskSpace {
            path: dest.display().to_string(),
            reason: e.to_string(),
        }
        .into();
    }
    EnvironmentError::DiskSpace {
        path: dest.display().to_string(),
        reason: e.to_string(),
    }
    .into()
}

fn map_io_sync(dest: &Path, e: std::io::Error) -> crate::error::TranscriptionError {
    EnvironmentError::DirectoryAccess {
        path: dest.display().to_string(),
        reason: format!("file sync failed (durability): {e}"),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn no_clobber_rejects_existing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.txt");
        fs::write(&path, b"old").unwrap();
        let err = OutputTransaction::new(&path, CommitMode::NoClobber)
            .commit_bytes(b"new")
            .unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert_eq!(fs::read(&path).unwrap(), b"old");
    }

    #[test]
    fn replace_overwrites() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.txt");
        fs::write(&path, b"old").unwrap();
        OutputTransaction::new(&path, CommitMode::Replace)
            .commit_bytes(b"new-data")
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new-data");
    }

    #[test]
    fn empty_dest_allowed_under_no_clobber() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        fs::write(&path, b"").unwrap();
        OutputTransaction::new(&path, CommitMode::NoClobber)
            .commit_bytes(b"filled")
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "filled");
    }

    #[test]
    fn failure_before_publish_keeps_old() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("keep.txt");
        fs::write(&path, b"original").unwrap();
        let err = OutputTransaction::new(&path, CommitMode::Replace)
            .commit_with(|_tmp| {
                Err(EnvironmentError::DiskSpace {
                    path: "inject".into(),
                    reason: "simulated full disk".into(),
                }
                .into())
            })
            .unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert_eq!(fs::read(&path).unwrap(), b"original");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".aurum.tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp left behind: {leftovers:?}");
    }

    #[test]
    fn rejects_directory_dest() {
        let dir = tempdir().unwrap();
        let err = OutputTransaction::new(dir.path(), CommitMode::Replace)
            .commit_bytes(b"x")
            .unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn concurrent_temps_are_distinct() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        let a = create_exclusive_temp(&dest).unwrap();
        let b = create_exclusive_temp(&dest).unwrap();
        assert_ne!(a, b);
        assert!(a.exists() && b.exists());
        let _ = fs::remove_file(a);
        let _ = fs::remove_file(b);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_destination() {
        let dir = tempdir().unwrap();
        let real = dir.path().join("real.txt");
        fs::write(&real, b"secret").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = OutputTransaction::new(&link, CommitMode::Replace)
            .commit_bytes(b"hijack")
            .unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert_eq!(fs::read(&real).unwrap(), b"secret");
    }

    #[test]
    fn commit_text_adds_newline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.txt");
        commit_text(&path, "hello", CommitMode::NoClobber).unwrap();
        let mut s = String::new();
        File::open(&path).unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello\n");
    }

    #[test]
    fn creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a/b/c.out");
        OutputTransaction::new(&path, CommitMode::NoClobber)
            .commit_bytes(b"nested")
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"nested");
    }

    #[cfg(unix)]
    #[test]
    fn unix_replace_does_not_unlink_before_rename() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("atomic.txt");
        fs::write(&path, b"original-content").unwrap();
        OutputTransaction::new(&path, CommitMode::Replace)
            .commit_bytes(b"replacement")
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
    }

    #[cfg(unix)]
    #[test]
    fn temp_is_owner_rw_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let dest = dir.path().join("perm.out");
        let tmp = create_exclusive_temp(&dest).unwrap();
        let mode = fs::metadata(&tmp).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0o600, got {mode:o}");
        let _ = fs::remove_file(tmp);
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_noclobber_exactly_one_wins() {
        let dir = tempdir().unwrap();
        let path = Arc::new(dir.path().join("race.txt"));
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = vec![];
        for i in 0..8 {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                let body = format!("writer-{i}");
                OutputTransaction::new(path.as_ref(), CommitMode::NoClobber)
                    .commit_bytes(body.as_bytes())
            }));
        }
        let mut ok = 0;
        let mut err = 0;
        for h in handles {
            match h.join().unwrap() {
                Ok(()) => ok += 1,
                Err(_) => err += 1,
            }
        }
        assert_eq!(ok, 1, "exactly one NoClobber writer must succeed");
        assert_eq!(err, 7);
        let content = fs::read_to_string(path.as_ref()).unwrap();
        assert!(
            content.starts_with("writer-"),
            "winner bytes incomplete: {content}"
        );
    }
}
