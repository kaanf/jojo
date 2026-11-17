//! D3: the executable each run used, recorded in the transaction that issues the run.
//!
//! One row per distinct provider, file, release and digest; every launched attempt and
//! every accepted lead turn names its row. When a run uses another identity than the
//! provider's previous run, the change is kept as a row and an `ExecutableChanged`
//! event: Settings & Providers shows it as a warning, the run itself is not refused.
use jojo_domain::identity::OpaqueId;
use jojo_domain::provider::{ProviderFingerprint, ProviderKind};

use crate::discovery::profiled_version;
use crate::model::{id_to_text, to_db};
use crate::sqlite::Value;
use crate::{ReadView, StorageError, Tx, TxError, sha256_hex};

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// One recorded executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableRecord {
    pub identity: String,
    pub provider: ProviderKind,
    pub path: String,
    pub version: String,
    pub sha256: String,
    /// Whether the version is the one the stream profiles were written against.
    pub profiled: bool,
    /// The event that first recorded it.
    pub event: u64,
}

/// The latest change of a provider's executable between two runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableChange {
    pub event: u64,
    pub previous: ExecutableRecord,
    pub current: ExecutableRecord,
}

/// The digest bound in a production fingerprint; `None` for any other profile.
fn production_digest(fingerprint: &ProviderFingerprint) -> Option<&str> {
    match fingerprint.provider {
        ProviderKind::ClaudeCode => {
            jojo_domain::claude::configured_digest(&fingerprint.configuration)
        }
        ProviderKind::Codex => jojo_domain::codex::configured_digest(&fingerprint.configuration),
    }
}

impl Tx<'_> {
    /// Records the executable a production fingerprint names and returns its identity;
    /// `None` for a synthetic profile, which names no real executable.
    fn record_executable(
        &mut self,
        fingerprint: &ProviderFingerprint,
    ) -> Result<Option<String>, TxError> {
        let Some(sha256) = production_digest(fingerprint) else {
            return Ok(None);
        };
        let provider = format!("{:?}", fingerprint.provider);
        let path = fingerprint.executable.display().to_string();
        let version = fingerprint.version.as_str();
        let identity = sha256_hex(&[
            provider.as_bytes(),
            b"\0",
            path.as_bytes(),
            b"\0",
            version.as_bytes(),
            b"\0",
            sha256.as_bytes(),
        ]);
        let known = self.conn.query_optional(
            "SELECT 1 FROM executable_identities WHERE identity=?1",
            &[Value::Text(&identity)],
            |r| r.int(0),
        )?;
        if known.is_none() {
            let event = self.append_event(
                "ExecutableRecorded",
                None,
                &format!("{identity}|{provider}|{version}|{sha256}|{path}"),
            )?;
            let profiled = version == profiled_version(fingerprint.provider);
            self.conn.execute(
                "INSERT INTO executable_identities VALUES (?1,?2,?3,?4,?5,?6,?7)",
                &[
                    Value::Text(&identity),
                    Value::Text(&provider),
                    Value::Text(&path),
                    Value::Text(version),
                    Value::Text(sha256),
                    Value::Int(i64::from(profiled)),
                    Value::Int(to_db(event.0)?),
                ],
            )?;
        }
        let previous = self.conn.query_optional(
            "SELECT identity FROM provider_executables WHERE provider=?1",
            &[Value::Text(&provider)],
            |r| r.text(0),
        )?;
        match previous {
            None => {
                self.conn.execute(
                    "INSERT INTO provider_executables VALUES (?1,?2)",
                    &[Value::Text(&provider), Value::Text(&identity)],
                )?;
            }
            Some(previous) if previous != identity => {
                let event = self.append_event(
                    "ExecutableChanged",
                    None,
                    &format!("{provider}|{previous}|{identity}"),
                )?;
                self.conn.execute(
                    "INSERT INTO executable_changes VALUES (?1,?2,?3,?4)",
                    &[
                        Value::Int(to_db(event.0)?),
                        Value::Text(&provider),
                        Value::Text(&previous),
                        Value::Text(&identity),
                    ],
                )?;
                self.conn.execute(
                    "UPDATE provider_executables SET identity=?2 WHERE provider=?1",
                    &[Value::Text(&provider), Value::Text(&identity)],
                )?;
            }
            Some(_) => {}
        }
        Ok(Some(identity))
    }

    /// Links a launched attempt to the executable of its frozen fingerprint.
    pub(crate) fn record_attempt_executable(
        &mut self,
        attempt: OpaqueId,
        fingerprint: &ProviderFingerprint,
    ) -> Result<(), TxError> {
        if let Some(identity) = self.record_executable(fingerprint)? {
            self.conn.execute(
                "INSERT INTO attempt_executables VALUES (?1,?2)",
                &[Value::Text(&id_to_text(attempt)), Value::Text(&identity)],
            )?;
        }
        Ok(())
    }

    /// Links an accepted lead turn to the executable it will run.
    pub fn record_turn_executable(
        &mut self,
        turn: OpaqueId,
        fingerprint: &ProviderFingerprint,
    ) -> Result<(), TxError> {
        let identity = self
            .record_executable(fingerprint)?
            .ok_or_else(|| deny("TurnRefused: not a production fingerprint"))?;
        self.conn.execute(
            "INSERT INTO turn_executables VALUES (?1,?2)",
            &[Value::Text(&id_to_text(turn)), Value::Text(&identity)],
        )?;
        Ok(())
    }
}

fn provider_of(text: &str) -> Result<ProviderKind, StorageError> {
    match text {
        "Codex" => Ok(ProviderKind::Codex),
        "ClaudeCode" => Ok(ProviderKind::ClaudeCode),
        other => Err(StorageError::Corrupt(format!(
            "executable provider {other}"
        ))),
    }
}

impl ReadView<'_> {
    fn executable(&self, identity: &str) -> Result<ExecutableRecord, StorageError> {
        let (provider, path, version, sha256, profiled, event) = self.conn.query_one(
            "SELECT provider,path,version,sha256,profiled,event FROM executable_identities
             WHERE identity=?1",
            &[Value::Text(identity)],
            |r| {
                Ok((
                    r.text(0)?,
                    r.text(1)?,
                    r.text(2)?,
                    r.text(3)?,
                    r.int(4)?,
                    r.int(5)?,
                ))
            },
        )?;
        Ok(ExecutableRecord {
            identity: identity.to_owned(),
            provider: provider_of(&provider)?,
            path,
            version,
            sha256,
            profiled: profiled == 1,
            event: u64::try_from(event).map_err(|_| StorageError::Corrupt("event".into()))?,
        })
    }

    /// The executable of a provider's last run, and the latest change between two runs.
    pub fn executable_record(
        &self,
        provider: ProviderKind,
    ) -> Result<(Option<ExecutableRecord>, Option<ExecutableChange>), StorageError> {
        let name = format!("{provider:?}");
        let last = self.conn.query_optional(
            "SELECT identity FROM provider_executables WHERE provider=?1",
            &[Value::Text(&name)],
            |r| r.text(0),
        )?;
        let change = self.conn.query_optional(
            "SELECT event,previous,identity FROM executable_changes WHERE provider=?1
             ORDER BY event DESC LIMIT 1",
            &[Value::Text(&name)],
            |r| Ok((r.int(0)?, r.text(1)?, r.text(2)?)),
        )?;
        let last = last
            .map(|identity| self.executable(&identity))
            .transpose()?;
        let change = match change {
            None => None,
            Some((event, previous, current)) => Some(ExecutableChange {
                event: u64::try_from(event).map_err(|_| StorageError::Corrupt("event".into()))?,
                previous: self.executable(&previous)?,
                current: self.executable(&current)?,
            }),
        };
        Ok((last, change))
    }

    /// The executable a launched attempt or an accepted lead turn used, if recorded.
    pub fn run_executable(&self, run: OpaqueId) -> Result<Option<ExecutableRecord>, StorageError> {
        let id = id_to_text(run);
        let identity = self.conn.query_optional(
            "SELECT identity FROM attempt_executables WHERE attempt=?1
             UNION ALL SELECT identity FROM turn_executables WHERE turn=?1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        identity
            .map(|identity| self.executable(&identity))
            .transpose()
    }
}
