//! Shared T02 fixtures: temporary data directories, an injected clock and tiny commands.
#![allow(dead_code)]

pub mod attempts;
pub mod scenario;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use jojo_domain::identity::{ClockEpochId, OpaqueId, Revision};
use jojo_domain::time::ClockReading;
use jojo_storage::{
    AggregateKey, CommandError, CommandId, CommandRequest, Engine, EngineConfig, EngineEpoch,
    Receipt,
};
use jojo_test_support::FakeClock;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// A private scratch directory removed when the test ends.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "jojo-t02-{}-{}-{name}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn db(&self) -> PathBuf {
        self.0.join("jojo.sqlite3")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn reading(unix_millis: i64) -> ClockReading {
    ClockReading {
        unix_millis,
        monotonic_nanos: 0,
        epoch: ClockEpochId(OpaqueId(1)),
    }
}

pub fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(reading(1_700_000_000_000)))
}

pub fn start(db: &Path) -> Engine {
    Engine::start(EngineConfig::new(db, clock())).expect("engine starts")
}

pub fn key(kind: &str, id: u128) -> AggregateKey {
    AggregateKey::new(kind, OpaqueId(id))
}

pub fn request(engine: &Engine, id: u128, kind: &str, payload: &str) -> CommandRequest {
    request_at(engine.epoch(), id, kind, payload)
}

pub fn request_at(epoch: EngineEpoch, id: u128, kind: &str, payload: &str) -> CommandRequest {
    CommandRequest::new(CommandId(OpaqueId(id)), epoch, kind, payload)
}

/// Creates `key` at revision 1 with state "1" and a `Created` event.
pub fn create(engine: &Engine, id: u128, key: &AggregateKey) -> Receipt {
    let owned = key.clone();
    engine
        .execute(
            request(engine, id, "create", &format!("{key:?}")),
            move |tx| {
                let revision = tx.create_aggregate(&owned, "1")?;
                tx.append_event("Created", Some((&owned, revision)), "1")?;
                Ok("created".into())
            },
        )
        .expect("create commits")
}

/// Compare-and-swap `key` from `expected` to the next revision; the state is the new
/// revision number and a `Bumped` event carries the same number.
pub fn bump(
    engine: &Engine,
    id: u128,
    key: &AggregateKey,
    expected: Revision,
) -> Result<Receipt, CommandError> {
    let owned = key.clone();
    engine.execute(
        request(engine, id, "bump", &format!("{key:?}@{}", expected.0)),
        move |tx| {
            let state = (expected.0 + 1).to_string();
            let revision = tx.update_aggregate(&owned, expected, &state)?;
            tx.append_event("Bumped", Some((&owned, revision)), &state)?;
            Ok(state)
        },
    )
}
