//! Operating-system-backed singleton ownership of one database.
//!
//! The owner holds an exclusive advisory `flock` on a sidecar lock file for its whole
//! lifetime. The kernel releases the lock when the owning process exits for any reason
//! (including SIGKILL), so a crashed owner never needs a timeout to be replaced, and a
//! live owner is never displaced because time passed.
//!
//! Scope: the lock coordinates Jojo helpers. It does not stop an unrelated program from
//! writing the database, and deleting the lock file while it is held would let a second
//! helper lock a fresh file. That residual case is covered by the database epoch fence
//! (`Writer::fence`), which rejects every write of a superseded engine.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::StorageError;

/// File name of the authoritative database inside the private data root.
pub const DATABASE_FILE_NAME: &str = "jojo.sqlite3";

/// Held for the lifetime of the owning engine. Dropping it releases ownership.
#[derive(Debug)]
pub struct OwnershipLock {
    file: File,
    lock_path: PathBuf,
    db_path: PathBuf,
}

impl OwnershipLock {
    /// Acquires ownership of `db_path` without waiting. Fails with
    /// [`StorageError::OwnershipHeld`] while any other holder exists.
    pub fn acquire(db_path: &Path) -> Result<Self, StorageError> {
        let db_path = canonical_database_path(db_path)?;
        let lock_path = lock_path_for(&db_path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock_path)
            .map_err(|e| StorageError::io("opening ownership lock file", e))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                let mut owner_hint = String::new();
                // Informational only; a torn or empty read is fine.
                let _ = (&file).take(256).read_to_string(&mut owner_hint);
                return Err(StorageError::OwnershipHeld {
                    lock_path,
                    owner_hint: owner_hint.trim().to_string(),
                });
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(StorageError::io("locking ownership lock file", e));
            }
        }
        // The lock is on an inode. If the path was unlinked/replaced between open and lock,
        // we would own a file nobody else can find; refuse instead of proceeding unowned.
        let locked = file
            .metadata()
            .map_err(|e| StorageError::io("inspecting ownership lock file", e))?;
        let visible = std::fs::metadata(&lock_path)
            .map_err(|e| StorageError::io("re-checking ownership lock path", e))?;
        if (locked.dev(), locked.ino()) != (visible.dev(), visible.ino()) {
            return Err(StorageError::OwnershipHeld {
                lock_path,
                owner_hint: "lock file was replaced during acquisition".into(),
            });
        }
        Ok(Self {
            file,
            lock_path,
            db_path,
        })
    }

    /// Canonical database path this lock protects; the engine opens exactly this path.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    /// Records who owns the lock, for display by a later helper/UI. Not used for decisions:
    /// a PID is not a process identity and the text can be stale.
    pub(crate) fn describe_owner(&mut self, description: &str) -> Result<(), StorageError> {
        (|| {
            self.file.set_len(0)?;
            self.file.seek(SeekFrom::Start(0))?;
            self.file.write_all(description.as_bytes())?;
            self.file.sync_all()
        })()
        .map_err(|e| StorageError::io("writing ownership metadata", e))
    }
}

impl Drop for OwnershipLock {
    /// Releases the lock explicitly instead of relying on the close. While another thread
    /// starts a process, the new child briefly holds a copy of this descriptor (until its
    /// exec closes it), and a lock released only by closing stays held for that moment: a
    /// successor acquiring right after the drop was refused about 2% of the time under
    /// concurrent spawning (M0). An explicit unlock applies to the open file itself.
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Resolves symlinks so that path aliases of one database share one lock.
fn canonical_database_path(db_path: &Path) -> Result<PathBuf, StorageError> {
    if db_path.exists() {
        return std::fs::canonicalize(db_path)
            .map_err(|e| StorageError::io("resolving database path", e));
    }
    let (Some(parent), Some(name)) = (db_path.parent(), db_path.file_name()) else {
        return Err(StorageError::corrupt(
            "database path has no parent directory",
        ));
    };
    let parent = std::fs::canonicalize(parent)
        .map_err(|e| StorageError::io("resolving database directory", e))?;
    Ok(parent.join(name))
}

fn lock_path_for(canonical_db_path: &Path) -> Result<PathBuf, StorageError> {
    let mut name = canonical_db_path
        .file_name()
        .ok_or_else(|| StorageError::corrupt("database path has no file name"))?
        .to_os_string();
    name.push(".owner-lock");
    Ok(canonical_db_path.with_file_name(name))
}

/// Creates (mode 0700) or validates the private data root and returns the database path
/// inside it. An existing root that is not a real directory, or that grants any access to
/// group/others, is refused rather than silently used or re-permissioned.
pub fn prepare_data_root(root: &Path) -> Result<PathBuf, StorageError> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)
        .map_err(|e| StorageError::io("creating data root", e))?;
    let metadata =
        std::fs::symlink_metadata(root).map_err(|e| StorageError::io("inspecting data root", e))?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(StorageError::DataRootNotPrivate(root.to_path_buf()));
    }
    Ok(root.join(DATABASE_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_file_sits_next_to_the_database() {
        assert_eq!(
            lock_path_for(Path::new("/data/jojo.sqlite3")).unwrap(),
            Path::new("/data/jojo.sqlite3.owner-lock")
        );
    }
}
