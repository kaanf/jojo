//! D3: the SHA-256 of a provider executable, remembered per file identity.
//!
//! A provider CLI is a native binary of a few hundred megabytes. Hashing it on every
//! Start and again at the launch boundary kept the helper's control thread busy for
//! about 20 s in a debug build and serialized Starts. The digest is now computed once
//! per file identity: device, inode, size, modification time and status-change time.
//! Any write or replacement of the file changes at least one of them, and the
//! status-change time cannot be set back by an unprivileged process, so an unchanged
//! identity means unchanged bytes. Every structural check still runs on each call.
//!
//! [`begin_check`] computes a digest on a background thread so that the control thread
//! never waits for one; [`inspect_executable`] computes it in place when it is unknown.
use std::collections::{HashMap, HashSet};
use std::fs::{File, Metadata};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::codex::resolved;
use crate::digest::sha256_reader;

/// Upper bound on the executable that is hashed; a larger file is refused, not sampled.
pub const MAX_CODEX_EXECUTABLE_BYTES: u64 = 1024 * 1024 * 1024;

/// What identifies the bytes of a file without reading them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FileKey {
    dev: u64,
    ino: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl FileKey {
    fn of(metadata: &Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            size: metadata.size(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

static DIGESTS: Mutex<Option<HashMap<PathBuf, (FileKey, String)>>> = Mutex::new(None);
static CHECKING: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn known(path: &Path, key: &FileKey) -> Option<String> {
    locked(&DIGESTS)
        .as_ref()
        .and_then(|m| m.get(path))
        .filter(|(k, _)| k == key)
        .map(|(_, digest)| digest.clone())
}

/// Every check that needs no read of the file's bytes.
fn listed(path: &Path) -> Result<Metadata, String> {
    let listed = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if listed.file_type().is_symlink() {
        return Err("symbolic link; configure the native binary itself".into());
    }
    if !listed.is_file() {
        return Err("not a regular file".into());
    }
    if !resolved(path) {
        return Err("path is not absolute and fully resolved".into());
    }
    let mode = listed.permissions().mode();
    if mode & 0o111 == 0 || mode & 0o6022 != 0 {
        return Err("must be executable, not set-id and writable by its owner only".into());
    }
    if listed.len() > MAX_CODEX_EXECUTABLE_BYTES {
        return Err("larger than the inspection limit".into());
    }
    Ok(listed)
}

/// The identity of a file that passes every structural check, without reading it.
pub(crate) fn file_key(path: &Path) -> Result<FileKey, String> {
    listed(path).map(|listed| FileKey::of(&listed))
}

/// SHA-256 of a native executable that passes every structural check. The bytes are read
/// only when this file identity was not hashed before.
pub(crate) fn inspect_executable(path: &Path) -> Result<String, String> {
    let listed = listed(path)?;
    let key = FileKey::of(&listed);
    if let Some(digest) = known(path, &key) {
        return Ok(digest);
    }
    let mut file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let opened = file.metadata().map_err(|e| format!("inspect: {e}"))?;
    if FileKey::of(&opened) != key {
        return Err("replaced during inspection".into());
    }
    let (digest, head) = sha256_reader(&mut file, MAX_CODEX_EXECUTABLE_BYTES)
        .map_err(|e| format!("read: {e}"))?
        .ok_or("empty, truncated or larger than the inspection limit")?;
    if file.metadata().map(|m| FileKey::of(&m)).ok() != Some(key) {
        return Err("changed during inspection".into());
    }
    // Mach-O 64-bit, or a universal (fat) container. A `#!` launcher is not accepted.
    if !matches!(
        head,
        [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xca, 0xfe, 0xba, 0xbf]
    ) {
        return Err("not a native executable; script launchers are refused".into());
    }
    locked(&DIGESTS)
        .get_or_insert_with(HashMap::new)
        .insert(path.to_owned(), (key, digest.clone()));
    Ok(digest)
}

/// Where the digest of one executable stands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableCheck {
    /// The digest of the file as it is now is known; inspecting it reads nothing.
    Known,
    /// The digest is being computed on a background thread.
    Checking,
    /// The file fails a structural check; inspecting it reports why.
    Unusable(String),
}

/// Returns at once. When the digest of the file as it is now is unknown, starts one
/// background thread that computes it (at most one per path at a time).
pub fn begin_check(path: &Path) -> ExecutableCheck {
    let key = match listed(path) {
        Ok(listed) => FileKey::of(&listed),
        Err(why) => return ExecutableCheck::Unusable(why),
    };
    if known(path, &key).is_some() {
        return ExecutableCheck::Known;
    }
    if !locked(&CHECKING)
        .get_or_insert_with(HashSet::new)
        .insert(path.to_owned())
    {
        return ExecutableCheck::Checking;
    }
    let owned = path.to_owned();
    let spawned = std::thread::Builder::new()
        .name("jojo-executable-digest".into())
        .spawn(move || {
            let _ = inspect_executable(&owned);
            if let Some(set) = locked(&CHECKING).as_mut() {
                set.remove(&owned);
            }
        });
    if spawned.is_err() {
        if let Some(set) = locked(&CHECKING).as_mut() {
            set.remove(path);
        }
        return ExecutableCheck::Unusable("no thread for the digest".into());
    }
    ExecutableCheck::Checking
}
