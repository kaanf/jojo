//! Content-addressed evidence blobs (P1-10). Filesystem effects stay outside writer
//! transactions: a blob is written to an owned temporary file, synced, renamed to its
//! digest and its directory synced *before* any database reference commits. A crash in
//! between leaves an unreferenced orphan, never a reference to bytes that do not exist.
//! Referenced blobs are verified by digest before use and are never reconstructed.
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::model::to_db;
use crate::sqlite::Value;
use crate::{Engine, ReadView, StorageError, Tx, TxError, sha256_hex};
use jojo_domain::payload::Record;

static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

type IoResult<T> = Result<T, String>;
fn io<T>(r: std::io::Result<T>) -> IoResult<T> {
    r.map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobRef {
    pub digest: String,
    pub size: u64,
}

/// Test fault boundaries around the rename that publishes a blob.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlobFaults {
    /// Fail after the temporary file is synced, before it is renamed.
    pub before_rename: bool,
    /// Fail after the rename and directory sync, before any reference is committed.
    pub after_rename: bool,
}

/// `<data root>/evidence/{blobs,tmp}`, private to the owning user.
#[derive(Clone, Debug)]
pub struct BlobStore {
    root: PathBuf,
}

fn private_dir(p: &Path) -> IoResult<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        Ok(_) => return Err(format!("{} is not a private directory", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            io(fs::create_dir(p))?;
            io(fs::set_permissions(p, fs::Permissions::from_mode(0o700)))?;
        }
        Err(e) => return Err(e.to_string()),
    }
    if io(fs::symlink_metadata(p))?.permissions().mode() & 0o077 != 0 {
        return Err(format!("{} must be 0700", p.display()));
    }
    Ok(())
}

fn sync_dir(p: &Path) -> IoResult<()> {
    io(fs::File::open(p).and_then(|f| f.sync_all()))
}

fn is_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl BlobStore {
    /// The store beside the engine's database, inside its private data root.
    pub fn for_engine(engine: &Engine) -> IoResult<Self> {
        let data = engine
            .db_path()
            .parent()
            .ok_or("database has no data root")?;
        Self::open(&data.join("evidence"))
    }

    pub fn open(root: &Path) -> IoResult<Self> {
        private_dir(root)?;
        private_dir(&root.join("blobs"))?;
        private_dir(&root.join("tmp"))?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub fn path(&self, digest: &str) -> PathBuf {
        self.root.join("blobs").join(digest)
    }

    /// Publishes `bytes` under their digest. An existing blob is verified, never replaced.
    pub fn put(&self, bytes: &[u8], faults: BlobFaults) -> IoResult<BlobRef> {
        let blob = BlobRef {
            digest: sha256_hex(&[bytes]),
            size: bytes.len() as u64,
        };
        let target = self.path(&blob.digest);
        if fs::symlink_metadata(&target).is_ok() {
            self.verify(&blob)?;
            return Ok(blob);
        }
        // Unique per write: an abandoned temporary file never blocks a later publication.
        let tmp = self.root.join("tmp").join(format!(
            "{}.{}.{}",
            blob.digest,
            std::process::id(),
            NEXT_TMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut f = io(OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp))?;
        io(f.write_all(bytes))?;
        io(f.sync_all())?;
        drop(f);
        if faults.before_rename {
            return Err("injected fault before blob rename".into());
        }
        io(fs::set_permissions(&tmp, fs::Permissions::from_mode(0o400)))?;
        io(fs::rename(&tmp, &target))?;
        sync_dir(&self.root.join("blobs"))?;
        if faults.after_rename {
            return Err("injected fault after blob rename, before reference".into());
        }
        Ok(blob)
    }

    /// The bytes published under `digest`, verified against it.
    pub fn read(&self, digest: &str) -> IoResult<Vec<u8>> {
        let bytes = io(fs::read(self.path(digest)))?;
        if sha256_hex(&[&bytes]) != digest {
            return Err("evidence digest mismatch".into());
        }
        Ok(bytes)
    }

    /// The referenced bytes exist as a regular file with exactly this size and digest.
    pub fn verify(&self, blob: &BlobRef) -> IoResult<()> {
        if !is_digest(&blob.digest) {
            return Err("malformed blob digest".into());
        }
        let p = self.path(&blob.digest);
        let m = fs::symlink_metadata(&p).map_err(|e| format!("blob {}: {e}", blob.digest))?;
        if !m.is_file() || m.nlink() != 1 || m.size() != blob.size {
            return Err(format!("blob {} is not the recorded file", blob.digest));
        }
        if sha256_hex(&[&io(fs::read(&p))?]) != blob.digest {
            return Err(format!("blob {} content changed", blob.digest));
        }
        Ok(())
    }

    /// Published blobs no reference names, plus abandoned temporary files. Safe to sweep
    /// later; nothing here deletes them.
    pub fn orphans(&self, referenced: &BTreeSet<String>) -> IoResult<Vec<String>> {
        let mut out = Vec::new();
        for dir in ["blobs", "tmp"] {
            for entry in io(fs::read_dir(self.root.join(dir)))? {
                let name = io(entry)?.file_name().to_string_lossy().into_owned();
                if dir == "tmp" || !referenced.contains(&name) {
                    out.push(format!("{dir}/{name}"));
                }
            }
        }
        out.sort();
        Ok(out)
    }
}

impl Tx<'_> {
    /// Records that a verified blob exists. Idempotent for identical bytes.
    pub fn reference_blob(&mut self, blob: &BlobRef) -> Result<(), TxError> {
        if !is_digest(&blob.digest) {
            return Err(TxError::Rejected("malformed blob digest".into()));
        }
        let size = self.conn.query_optional(
            "SELECT size FROM evidence_blobs WHERE digest=?1",
            &[Value::Text(&blob.digest)],
            |r| r.int(0),
        )?;
        match size {
            Some(s) if s == to_db(blob.size)? => Ok(()),
            Some(_) => Err(TxError::Rejected("blob size conflict".into())),
            None => {
                let event = self.append_event(
                    "EvidenceBlobReferenced",
                    None,
                    &Record::new()
                        .text("digest", blob.digest.clone())
                        .count("size", blob.size),
                )?;
                self.conn.execute(
                    "INSERT INTO evidence_blobs VALUES (?1,?2,?3)",
                    &[
                        Value::Text(&blob.digest),
                        Value::Int(to_db(blob.size)?),
                        Value::Int(to_db(event.0)?),
                    ],
                )?;
                Ok(())
            }
        }
    }
}

impl ReadView<'_> {
    pub fn referenced_blobs(&self) -> Result<BTreeSet<String>, StorageError> {
        Ok(self
            .conn
            .query("SELECT digest FROM evidence_blobs", &[], |r| r.text(0))?
            .into_iter()
            .collect())
    }
}
