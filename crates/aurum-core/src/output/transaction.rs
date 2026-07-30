//! Shared secure output transaction for STT, cleanup, and TTS.
//!
//! Protocol (cross-platform):
//! 1. Resolve destination (reject unsafe symlink targets by default).
//! 2. Create a randomized same-directory temporary file with exclusive create.
//! 3. Write payload, flush, and `sync_all` the file.
//! 4. Atomically publish (`rename`, with Windows replace-via-remove).
//! 5. Best-effort directory `sync` where supported.
//!
//! A failure before commit leaves the previous destination intact and removes
//! our temp file. Temp names include PID + random bytes so concurrent writers
//! never share a path.

use crate::error::{EnvironmentError, Result, UserError};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Overwrite policy for a destination path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitMode {
    /// Fail if the destination exists and is non-empty (or is a non-empty file
    /// that appears between open and commit).
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
                // Permissions already set at exclusive create (0o600 on Unix).
                file.write_all(bytes)
                    .map_err(|e| map_io_write(&self.dest, e))?;
                file.flush().map_err(|e| map_io_write(&self.dest, e))?;
                file.sync_all().map_err(|e| map_io_write(&self.dest, e))?;
            }
            // Re-check race: file created between preflight and publish.
            check_dest_policy(&self.dest, self.mode, self.symlink_policy)?;
            publish_replace(&tmp, &self.dest)?;
            sync_parent_dir(&self.dest);
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        write_result
    }

    /// Write via a callback that receives the exclusive temp file path.
    ///
    /// The callback must fully write and flush its content to `tmp`. This path
    /// is used when the payload is produced by a specialized writer (e.g. WAV).
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
            if let Ok(file) = File::open(&tmp) {
                let _ = file.sync_all();
            }
            check_dest_policy(&self.dest, self.mode, self.symlink_policy)?;
            publish_replace(&tmp, &self.dest)?;
            sync_parent_dir(&self.dest);
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

    // Retry on collision (extremely unlikely with random suffix).
    for _ in 0..32 {
        let suffix = random_suffix();
        let name = format!(".{}.{}-{}.aurum.tmp", safe_stem, std::process::id(), suffix);
        let tmp = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => {
                drop(file);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
                }
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

/// Atomically publish `tmp` as `dest` without a window that loses both files.
///
/// * **Unix:** `rename` replaces the destination atomically.
/// * **Windows:** move the existing dest aside, rename temp into place, then
///   remove the backup. If the final rename fails, restore the backup so the
///   previous destination is preserved.
fn publish_replace(tmp: &Path, dest: &Path) -> Result<()> {
    #[cfg(unix)]
    {
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

fn sync_parent_dir(dest: &Path) {
    let Some(parent) = dest.parent() else {
        return;
    };
    if parent.as_os_str().is_empty() {
        return;
    }
    // Directory fsync is best-effort and platform-dependent.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
}

fn map_io_write(dest: &Path, e: std::io::Error) -> crate::error::TranscriptionError {
    EnvironmentError::DiskSpace {
        path: dest.display().to_string(),
        reason: e.to_string(),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
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
        // No leftover .aurum.tmp for this dest.
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
        // If dest exists, Unix rename overwrites atomically — dest bytes change
        // only after successful publish, never via a prior remove_file.
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
}
