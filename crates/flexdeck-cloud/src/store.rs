//! The cloud's data directory.
//!
//! The cloud keeps almost nothing: no accounts, no linked desktops, no
//! credentials. Tunnels and pairing offers live in memory and are rebuilt by
//! the desktops themselves as they reconnect. This module guards the directory
//! the cloud runs from: one process per directory, proven writable at start.

use std::fs;
use std::path::{Path, PathBuf};

/// The file an advisory lock is taken on.
const LOCK: &str = "cloud.lock";

/// Held for as long as the process runs, so a second process pointed at the
/// same data directory fails loudly rather than racing this one's writes.
///
/// An OS advisory lock rather than a marker file, because the kernel releases it
/// when the process dies: a crash cannot leave a lock behind that has to be
/// deleted by hand before the service will start again.
#[derive(Debug)]
pub struct DataDirLock {
    /// Dropping this releases the lock. Named for what it is rather than `_`,
    /// which would drop it immediately and lock nothing at all.
    _file: fs::File,
}

/// The directory the cloud keeps its files in.
#[derive(Debug, Clone)]
pub struct DataDir {
    path: PathBuf,
}

impl DataDir {
    pub fn at(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Take the directory for this process, or say who has it.
    pub fn lock(&self) -> Result<DataDirLock, StoreError> {
        fs::create_dir_all(&self.path).map_err(|error| StoreError::Write {
            path: self.path.clone(),
            error,
        })?;

        let path = self.path.join(LOCK);
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|error| StoreError::Write {
                path: path.clone(),
                error,
            })?;

        file.try_lock().map_err(|_| StoreError::Locked {
            path: self.path.clone(),
        })?;

        Ok(DataDirLock { _file: file })
    }

    /// Prove the directory is writable before serving.
    ///
    /// A read-only mount, or a volume that has not attached yet, otherwise looks
    /// exactly like a healthy fresh deployment — right up until the first write.
    pub fn probe(&self) -> Result<(), StoreError> {
        fs::create_dir_all(&self.path).map_err(|error| StoreError::Write {
            path: self.path.clone(),
            error,
        })?;

        let probe = self.path.join(format!(".probe.{}.tmp", std::process::id()));
        fs::write(&probe, b"probe").map_err(|error| StoreError::Write {
            path: probe.clone(),
            error,
        })?;
        fs::remove_file(&probe).map_err(|error| StoreError::Write { path: probe, error })
    }
}

/// Why the data directory could not be used.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not write {path}: {error}")]
    Write {
        path: PathBuf,
        error: std::io::Error,
    },

    #[error("another flexdeck cloud is already using {path}; run one process per data directory")]
    Locked { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_process_cannot_take_the_same_data_directory() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let data = DataDir::at(directory.path());

        let _held = data.lock().expect("the first lock is taken");
        assert!(matches!(data.lock(), Err(StoreError::Locked { .. })));
    }

    #[test]
    fn the_probe_creates_the_directory_and_leaves_nothing_in_it() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let inner = directory.path().join("fresh");

        DataDir::at(&inner).probe().expect("writable");

        let left: Vec<_> = fs::read_dir(&inner).expect("exists").collect();
        assert!(left.is_empty(), "the probe cleans up after itself");
    }
}
