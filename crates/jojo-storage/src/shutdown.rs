//! Close-ticket and shutdown coordinator (P1-12).
//!
//! `PrepareClose` atomically layers a close gate over the session admission and snapshots
//! active work: current-epoch scheduler attempts that are not sealed (reserved, starting,
//! running, stopping or settling). Queued-only work is not active and stays queued. With
//! no active work the ticket resolves `NO_ACTIVE` without any confirmation; otherwise it
//! waits for exactly one human resolution:
//!
//! - `CancelClose` records the cancellation and removes only that gate. It sends nothing
//!   and leaves the stored RUN/HOLD intent and user pauses/cancels unchanged.
//! - `ConfirmClose` freezes the shutdown target set (re-checked at commit, so a reserved or
//!   starting attempt cannot vanish) and commits a shutdown stop intent for every target
//!   whose tree is not already proven absent. That intent is an interruption, never a
//!   permanent task cancellation. Each P1-09 runner observes it and escalates in parallel
//!   (capability INT, TERM, verified KILL) on its own retained lifetime handle.
//!
//! A GUI disconnect (stdio EOF) confirms an emergency stop without a human. Its session is
//! never sealed clean.
//!
//! [`finish_close`] waits within an injected budget for every target runner to conclude,
//! settles proven-absent targets through P1-10, then commits one receipt computed inside
//! the writer transaction. READY (plus the CLEAN session marker, atomically) needs every
//! target proven stopped and sealed, intact evidence, no live claim and no earlier-epoch
//! process without an absence proof; anything else commits FAILED and keeps claims and the
//! admission hold. Nothing here signals a process: only a runner holding the lifetime
//! handle may, and a failed or unknown stop is never turned into a clean acknowledgement.
use std::time::Duration;

use jojo_domain::identity::{IdGenerator, OpaqueId};
use jojo_domain::payload::Record;
use jojo_domain::shutdown::{
    CloseOrigin, CloseResolution, CloseState, ShutdownOutcome, TargetFacts, TargetStop,
    close_state, verdict,
};
use jojo_domain::time::Clock;

use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::outcomes::{SettlementContext, settle_attempt};
use crate::sqlite::{Connection, Value};
use crate::{
    BlobFaults, EngineEpoch, EventSeq, HumanOrigin, ReadView, StopBudgets, StorageError, Tx,
    TxError,
};

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// Whether an open or confirmed close ticket gates admission in `epoch`.
pub(crate) fn close_gate(conn: &Connection, epoch: EngineEpoch) -> Result<bool, StorageError> {
    Ok(conn.query_one(
        "SELECT EXISTS(SELECT 1 FROM close_tickets t WHERE t.epoch=?1 AND NOT EXISTS(
           SELECT 1 FROM close_resolutions r WHERE r.ticket=t.ticket AND r.resolution='CANCELLED'))",
        &[Value::Int(to_db(epoch.0)?)],
        |r| r.int(0),
    )? != 0)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveWork {
    pub attempt: OpaqueId,
    pub phase: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloseTicket {
    pub ticket: OpaqueId,
    pub epoch: EngineEpoch,
    pub origin: CloseOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShutdownReceipt {
    pub ticket: OpaqueId,
    pub shutdown: OpaqueId,
    pub event: EventSeq,
    pub outcome: ShutdownOutcome,
    pub process_stop_confirmed: bool,
    pub persistence_sealed: bool,
    /// One line per target: `attempt: stop, sealed`.
    pub targets: Vec<String>,
    pub recovery_items: Vec<String>,
    pub failures: Vec<String>,
}

/// The newest close of the current epoch, as durable records prove it.
#[derive(Clone, Debug)]
pub struct CloseStatus {
    pub state: CloseState,
    pub ticket: Option<CloseTicket>,
    pub shutdown: Option<OpaqueId>,
    /// Live: work that is active right now (a dialog count may change; it never confirms).
    pub active: Vec<ActiveWork>,
    /// The frozen shutdown target set with its current stop facts.
    pub targets: Vec<(OpaqueId, TargetFacts)>,
    pub receipt: Option<ShutdownReceipt>,
}

fn lines(s: &str) -> Vec<String> {
    s.lines().map(str::to_owned).collect()
}

fn actor(origin: &HumanOrigin) -> Result<String, TxError> {
    let (HumanOrigin::TrustedUi { actor } | HumanOrigin::FakeHumanFixture { actor }) = origin;
    if actor.trim().is_empty() {
        return Err(deny("human identity required"));
    }
    Ok(format!("{origin:?}"))
}

fn current_epoch(conn: &Connection) -> Result<EngineEpoch, StorageError> {
    crate::queries::latest_session(conn)?
        .map(|s| s.epoch)
        .ok_or_else(|| StorageError::corrupt("no engine session"))
}

fn active_work(conn: &Connection, epoch: EngineEpoch) -> Result<Vec<ActiveWork>, StorageError> {
    let rows = conn.query(
        "SELECT a.attempt,p.phase FROM scheduler_attempts a JOIN approval_attempts p ON p.id=a.attempt
         WHERE a.epoch=?1 AND a.sealed=0 ORDER BY a.attempt",
        &[Value::Int(to_db(epoch.0)?)],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?;
    rows.into_iter()
        .map(|(a, phase)| {
            Ok(ActiveWork {
                attempt: id_from_text(&a)?,
                phase,
            })
        })
        .collect()
}

type TicketRow = (CloseTicket, Option<(CloseResolution, Option<OpaqueId>)>);

/// The newest ticket of `epoch` (or the named one) and its resolution.
fn ticket(
    conn: &Connection,
    epoch: Option<EngineEpoch>,
    id: Option<OpaqueId>,
) -> Result<Option<TicketRow>, StorageError> {
    let row = conn.query_optional(
        "SELECT t.ticket,t.epoch,t.origin,r.resolution,r.shutdown FROM close_tickets t
         LEFT JOIN close_resolutions r ON r.ticket=t.ticket
         WHERE (?1 IS NULL OR t.epoch=?1) AND (?2 IS NULL OR t.ticket=?2)
         ORDER BY t.event DESC LIMIT 1",
        &[
            epoch.map_or(Ok(Value::Null), |e| to_db(e.0).map(Value::Int))?,
            id.map(id_to_text)
                .as_deref()
                .map_or(Value::Null, Value::Text),
        ],
        |r| {
            Ok((
                r.text(0)?,
                r.int(1)?,
                r.text(2)?,
                r.optional_text(3)?,
                r.optional_text(4)?,
            ))
        },
    )?;
    let Some((t, e, origin, resolution, shutdown)) = row else {
        return Ok(None);
    };
    let ticket = CloseTicket {
        ticket: id_from_text(&t)?,
        epoch: EngineEpoch(from_db(e)?),
        origin: if origin == "USER" {
            CloseOrigin::User
        } else {
            CloseOrigin::Disconnect
        },
    };
    let resolution = match resolution.as_deref() {
        None => None,
        Some(r) => Some((
            match r {
                "CANCELLED" => CloseResolution::Cancelled,
                "CONFIRMED" => CloseResolution::Confirmed,
                _ => CloseResolution::NoActive,
            },
            shutdown.as_deref().map(id_from_text).transpose()?,
        )),
    };
    Ok(Some((ticket, resolution)))
}

/// Durable stop/persistence facts of one target. Only a supervision conclusion, a retired
/// never-issued reservation or a sealed recovered result proves its processes stopped.
fn target_facts(conn: &Connection, id: &str) -> Result<TargetFacts, StorageError> {
    let obs = conn.query(
        "SELECT kind,detail FROM process_observations WHERE attempt=?1 ORDER BY event",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?;
    let has = |k: &str| obs.iter().any(|(kind, _)| kind == k);
    let (sealed, launched, result) = conn.query_one(
        "SELECT (SELECT sealed FROM scheduler_attempts WHERE attempt=?1),
                EXISTS(SELECT 1 FROM process_launches WHERE attempt=?1),
                EXISTS(SELECT 1 FROM attempt_results WHERE attempt=?1)",
        &[Value::Text(id)],
        |r| {
            Ok((
                r.optional_int(0)? == Some(1),
                r.int(1)? != 0,
                r.int(2)? != 0,
            ))
        },
    )?;
    let stop = if let Some((_, why)) = obs.iter().find(|(k, _)| k == "STOP_UNCONFIRMED") {
        TargetStop::Unconfirmed(why.clone())
    } else if has("TREE_ABSENT") || has("NOT_SPAWNED") {
        TargetStop::Stopped {
            forced: obs
                .iter()
                .any(|(k, d)| k == "SIGNAL_SENT" && d.starts_with("KILL ")),
        }
    } else if has("SPAWN_FAILED") {
        TargetStop::Unconfirmed("spawn outcome uncertain; no process identity".into())
    } else if !launched && sealed {
        TargetStop::NeverStarted
    } else if result {
        TargetStop::Stopped { forced: false }
    } else {
        TargetStop::Stopping
    };
    Ok(TargetFacts {
        stop,
        sealed,
        evidence_lost: has("EVIDENCE_LOSS"),
    })
}

fn targets(conn: &Connection, shutdown: &str) -> Result<Vec<(String, TargetFacts)>, StorageError> {
    conn.query(
        "SELECT attempt FROM shutdown_targets WHERE shutdown=?1 ORDER BY attempt",
        &[Value::Text(shutdown)],
        |r| r.text(0),
    )?
    .into_iter()
    .map(|a| Ok((a.clone(), target_facts(conn, &a)?)))
    .collect()
}

fn receipt(
    conn: &Connection,
    ticket: OpaqueId,
    shutdown: &str,
) -> Result<Option<ShutdownReceipt>, StorageError> {
    let row = conn.query_optional(
        "SELECT event,outcome,process_stop_confirmed,persistence_sealed,targets,recovery_items,failures
         FROM shutdown_receipts WHERE shutdown=?1 ORDER BY event DESC LIMIT 1",
        &[Value::Text(shutdown)],
        |r| {
            Ok((
                r.int(0)?,
                r.text(1)?,
                r.int(2)?,
                r.int(3)?,
                r.text(4)?,
                r.text(5)?,
                r.text(6)?,
            ))
        },
    )?;
    let Some((event, outcome, stopped, sealed, targets, items, failures)) = row else {
        return Ok(None);
    };
    Ok(Some(ShutdownReceipt {
        ticket,
        shutdown: id_from_text(shutdown)?,
        event: EventSeq(from_db(event)?),
        outcome: match outcome.as_str() {
            "READY" => ShutdownOutcome::Ready,
            "EMERGENCY" => ShutdownOutcome::Emergency,
            _ => ShutdownOutcome::Failed,
        },
        process_stop_confirmed: stopped == 1,
        persistence_sealed: sealed == 1,
        targets: lines(&targets),
        recovery_items: lines(&items),
        failures: lines(&failures),
    }))
}

impl ReadView<'_> {
    pub fn close_status(&self) -> Result<CloseStatus, StorageError> {
        let epoch = current_epoch(self.conn)?;
        let active = active_work(self.conn, epoch)?;
        let Some((t, resolution)) = ticket(self.conn, Some(epoch), None)? else {
            return Ok(CloseStatus {
                state: CloseState::Open,
                ticket: None,
                shutdown: None,
                active,
                targets: Vec::new(),
                receipt: None,
            });
        };
        let shutdown = resolution.and_then(|(_, s)| s);
        let (targets, receipt) = match shutdown {
            Some(s) => {
                let s = id_to_text(s);
                let targets = targets(self.conn, &s)?
                    .into_iter()
                    .map(|(a, f)| Ok((id_from_text(&a)?, f)))
                    .collect::<Result<_, StorageError>>()?;
                (targets, receipt(self.conn, t.ticket, &s)?)
            }
            None => (Vec::new(), None),
        };
        Ok(CloseStatus {
            state: close_state(
                Some(resolution.map(|(r, _)| r)),
                receipt.as_ref().map(|r| r.outcome),
            ),
            ticket: Some(t),
            shutdown,
            active,
            targets,
            receipt,
        })
    }
}

impl Tx<'_> {
    fn insert_ticket(
        &mut self,
        ticket: OpaqueId,
        origin: &str,
        actor: &str,
    ) -> Result<usize, TxError> {
        let active = active_work(self.conn, self.epoch)?;
        let summary = active
            .iter()
            .map(|a| format!("{}:{}", id_to_text(a.attempt), a.phase))
            .collect::<Vec<_>>()
            .join("\n");
        let id = id_to_text(ticket);
        let event = self.append_event(
            "CloseTicketPrepared",
            None,
            &Record::new()
                .text("ticket", id.clone())
                .text("origin", origin),
        )?;
        self.conn.execute(
            "INSERT INTO close_tickets VALUES (?1,?2,?3,?4,?5,?6)",
            &[
                Value::Text(&id),
                Value::Int(to_db(self.epoch.0)?),
                Value::Text(origin),
                Value::Text(actor),
                Value::Text(&summary),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(active.len())
    }

    fn resolve_ticket(
        &mut self,
        ticket: OpaqueId,
        resolution: &str,
        shutdown: Option<OpaqueId>,
        actor: &str,
    ) -> Result<(), TxError> {
        let id = id_to_text(ticket);
        let event = self.append_event(
            "CloseTicketResolved",
            None,
            &Record::new()
                .text("ticket", id.clone())
                .text("resolution", resolution),
        )?;
        self.conn.execute(
            "INSERT INTO close_resolutions VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Text(&id),
                Value::Text(resolution),
                shutdown
                    .map(id_to_text)
                    .as_deref()
                    .map_or(Value::Null, Value::Text),
                Value::Text(actor),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }

    /// The ticket named by a cancel/confirm: it must belong to this running epoch.
    fn owned_ticket(&self, id: OpaqueId) -> Result<TicketRow, TxError> {
        let row = ticket(self.conn, None, Some(id))?.ok_or_else(|| deny("unknown close ticket"))?;
        if row.0.epoch != self.epoch {
            return Err(deny("close ticket belongs to an earlier engine epoch"));
        }
        Ok(row)
    }

    /// Freezes every current-epoch unsealed attempt as a target. Attempts whose tree is
    /// already proven absent only settle; all others get a shutdown stop intent, which
    /// also retires a reservation whose launch was never issued.
    fn freeze_targets(&mut self, shutdown: OpaqueId) -> Result<usize, TxError> {
        let s = id_to_text(shutdown);
        let active = active_work(self.conn, self.epoch)?;
        for a in &active {
            let id = id_to_text(a.attempt);
            let event = self.append_event(
                "ShutdownTarget",
                None,
                &Record::new()
                    .text("shutdown", s.clone())
                    .text("attempt", id.clone()),
            )?;
            self.conn.execute(
                "INSERT INTO shutdown_targets VALUES (?1,?2,?3)",
                &[
                    Value::Text(&s),
                    Value::Text(&id),
                    Value::Int(to_db(event.0)?),
                ],
            )?;
            let concluded = self.conn.query_one(
                "SELECT EXISTS(SELECT 1 FROM process_observations WHERE attempt=?1
                   AND kind IN ('TREE_ABSENT','NOT_SPAWNED'))",
                &[Value::Text(&id)],
                |r| r.int(0),
            )?;
            if concluded == 0 {
                self.request_shutdown_stop(a.attempt)?;
            }
        }
        Ok(active.len())
    }

    /// PrepareClose. Returns `NO_ACTIVE`, `CONFIRM_REQUIRED n` or, when a close of this
    /// epoch is already open or confirmed, `EXISTING` (repeated quits share one operation).
    pub fn prepare_close(
        &mut self,
        ticket: OpaqueId,
        shutdown: OpaqueId,
        origin: &HumanOrigin,
    ) -> Result<String, TxError> {
        let actor = actor(origin)?;
        if close_gate(self.conn, self.epoch)? {
            return Ok("EXISTING".into());
        }
        let active = self.insert_ticket(ticket, "USER", &actor)?;
        if active == 0 {
            self.resolve_ticket(ticket, "NO_ACTIVE", Some(shutdown), &actor)?;
            return Ok("NO_ACTIVE".into());
        }
        Ok(format!("CONFIRM_REQUIRED {active}"))
    }

    /// CancelClose: removes only this close gate. No stop intent or signal exists yet.
    pub fn cancel_close(
        &mut self,
        ticket: OpaqueId,
        origin: &HumanOrigin,
    ) -> Result<String, TxError> {
        let actor = actor(origin)?;
        match self.owned_ticket(ticket)?.1 {
            None => {}
            Some((CloseResolution::Cancelled, _)) => return Err(deny("close ticket expired")),
            Some(_) => {
                return Err(deny(
                    "close already confirmed; issued stops cannot be undone",
                ));
            }
        }
        self.resolve_ticket(ticket, "CANCELLED", None, &actor)?;
        Ok("CANCELLED".into())
    }

    /// ConfirmClose: freezes the target set and commits shutdown stop intents. Idempotent
    /// for an already confirmed ticket; refused for a cancelled (expired) one.
    pub fn confirm_close(
        &mut self,
        ticket: OpaqueId,
        shutdown: OpaqueId,
        origin: &HumanOrigin,
    ) -> Result<String, TxError> {
        let actor = actor(origin)?;
        match self.owned_ticket(ticket)?.1 {
            None => {}
            Some((CloseResolution::Cancelled, _)) => return Err(deny("close ticket expired")),
            Some(_) => return Ok("CONFIRMED".into()),
        }
        self.resolve_ticket(ticket, "CONFIRMED", Some(shutdown), &actor)?;
        let n = self.freeze_targets(shutdown)?;
        Ok(format!("CONFIRMED {n}"))
    }

    /// GUI disconnect: no confirmation is possible. An open user ticket is superseded by
    /// a confirmed DISCONNECT ticket; an already resolved close keeps going. The stored
    /// admission intent is closed as well.
    pub fn emergency_close(
        &mut self,
        ticket: OpaqueId,
        shutdown: OpaqueId,
    ) -> Result<String, TxError> {
        self.set_admission(crate::Admission::Closed)?;
        let actor = "GUI disconnect (stdio EOF)";
        match ticket_of(self.conn, self.epoch)? {
            Some((_, Some((r, _)))) if r != CloseResolution::Cancelled => {
                return Ok("ALREADY_CLOSING".into());
            }
            Some((open, None)) => self.resolve_ticket(open.ticket, "CANCELLED", None, actor)?,
            _ => {}
        }
        self.insert_ticket(ticket, "DISCONNECT", actor)?;
        self.resolve_ticket(ticket, "CONFIRMED", Some(shutdown), actor)?;
        let n = self.freeze_targets(shutdown)?;
        Ok(format!("EMERGENCY {n}"))
    }

    /// Commits one receipt for the resolved close of this epoch, computed from the facts
    /// visible in this transaction. READY also seals the session CLEAN in the same commit;
    /// a disconnect never does. Returns the receipt outcome.
    pub fn finalize_shutdown(&mut self, disconnected: bool) -> Result<String, TxError> {
        let (t, resolution) =
            ticket_of(self.conn, self.epoch)?.ok_or_else(|| deny("no close to finalize"))?;
        let Some((CloseResolution::Confirmed | CloseResolution::NoActive, Some(shutdown))) =
            resolution
        else {
            return Err(deny("close is not confirmed"));
        };
        let s = id_to_text(shutdown);
        if receipt(self.conn, t.ticket, &s)?.is_some_and(|r| r.outcome != ShutdownOutcome::Failed) {
            return Err(deny("shutdown already acknowledged"));
        }
        let facts = targets(self.conn, &s)?;
        let epoch = Value::Int(to_db(self.epoch.0)?);
        let untracked = self.conn.query(
            "SELECT attempt FROM scheduler_attempts WHERE epoch=?1 AND sealed=0
             AND attempt NOT IN (SELECT attempt FROM shutdown_targets WHERE shutdown=?2)",
            &[epoch, Value::Text(&s)],
            |r| r.text(0),
        )?;
        let unconfirmed: Vec<String> = self
            .conn
            .query(
                "SELECT l.attempt FROM process_launches l WHERE l.epoch<?1
                 AND NOT EXISTS(SELECT 1 FROM attempt_results WHERE attempt=l.attempt)
                 AND NOT EXISTS(SELECT 1 FROM recovery_absence_proofs WHERE attempt=l.attempt)
                 ORDER BY l.attempt",
                &[epoch],
                |r| r.text(0),
            )?
            .into_iter()
            .map(|a| {
                format!("{a}: earlier-epoch attempt neither settled nor proven absent by recovery")
            })
            .collect();
        let mut unsealed: Vec<String> = untracked
            .into_iter()
            .map(|a| format!("{a}: active attempt outside the shutdown target set"))
            .collect();
        let claims = crate::queries::live_claims(self.conn, None)?.len();
        if claims > 0 {
            unsealed.push(format!("{claims} live claims remain"));
        }
        let origin = if disconnected {
            CloseOrigin::Disconnect
        } else {
            t.origin
        };
        let v = verdict(origin, &facts, &unconfirmed, &unsealed);
        let summary = facts
            .iter()
            .map(|(a, f)| format!("{a}: {:?}, sealed={}", f.stop, f.sealed))
            .collect::<Vec<_>>()
            .join("\n");
        let items = self
            .conn
            .query(
                "SELECT attempt||': '||outcome FROM attempt_results WHERE outcome<>'SUCCEEDED'
                   AND attempt IN (SELECT attempt FROM shutdown_targets WHERE shutdown=?1)
                 UNION SELECT f.attempt||': effect '||f.operation_key||' ISSUED_UNKNOWN'
                   FROM attempt_effects f JOIN outbox o ON o.operation_key=f.operation_key
                   WHERE o.state='ISSUED_UNKNOWN'
                 UNION SELECT attempt||': recovery hold' FROM attempt_recovery_holds
                   WHERE attempt NOT IN (SELECT attempt FROM attempt_results)
                 ORDER BY 1",
                &[Value::Text(&s)],
                |r| r.text(0),
            )?
            .join("\n");
        let outcome = match v.outcome {
            ShutdownOutcome::Ready => "READY",
            ShutdownOutcome::Failed => "FAILED",
            ShutdownOutcome::Emergency => "EMERGENCY",
        };
        let event = self.append_event(
            "ShutdownReceipt",
            None,
            &Record::new()
                .text("shutdown", s.clone())
                .text("outcome", outcome),
        )?;
        self.conn.execute(
            "INSERT INTO shutdown_receipts VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            &[
                Value::Int(to_db(event.0)?),
                Value::Text(&s),
                Value::Text(outcome),
                Value::Int(i64::from(v.process_stop_confirmed)),
                Value::Int(i64::from(v.persistence_sealed)),
                Value::Text(&summary),
                Value::Text(&items),
                Value::Text(&v.failures.join("\n")),
            ],
        )?;
        if v.outcome == ShutdownOutcome::Ready {
            self.conn.execute(
                "UPDATE engine_sessions SET shutdown='CLEAN', admission='CLOSED', ended_at_unix_ms=?1
                 WHERE epoch=?2 AND shutdown='RUNNING'",
                &[Value::Int(self.now_unix_ms), Value::Int(to_db(self.epoch.0)?)],
            )?;
            self.append_event(
                "EngineStopped",
                None,
                &Record::new().text("shutdown", "CLEAN"),
            )?;
        }
        Ok(outcome.into())
    }
}

fn ticket_of(conn: &Connection, epoch: EngineEpoch) -> Result<Option<TicketRow>, StorageError> {
    ticket(conn, Some(epoch), None)
}

/// How long [`finish_close`] waits for runners to conclude, measured on the injected clock.
#[derive(Clone, Copy, Debug)]
pub struct CloseWait {
    pub budget: Duration,
    pub poll: Duration,
}

impl CloseWait {
    /// The full escalation of one runner plus two polls of stop-intent latency. Runners
    /// escalate in parallel, so the budget does not grow with the number of targets.
    pub fn for_budgets(b: StopBudgets, poll: Duration) -> Self {
        Self {
            budget: b.interrupt + b.terminate + b.kill + b.drain + poll * 2,
            poll,
        }
    }
}

/// The newest close of the current epoch.
pub fn close_status(ctx: &SettlementContext<'_>) -> Result<CloseStatus, String> {
    ctx.read(|v| v.close_status())
}

fn next(ids: &dyn IdGenerator) -> Result<OpaqueId, String> {
    ids.next_id().map_err(|_| "ids exhausted".to_owned())
}

pub fn prepare_close(
    ctx: &SettlementContext<'_>,
    origin: HumanOrigin,
) -> Result<CloseStatus, String> {
    let (ticket, shutdown) = (next(ctx.ids)?, next(ctx.ids)?);
    ctx.cmd("PrepareClose", ticket, move |tx| {
        tx.prepare_close(ticket, shutdown, &origin)
    })?;
    close_status(ctx)
}

pub fn cancel_close(
    ctx: &SettlementContext<'_>,
    ticket: OpaqueId,
    origin: HumanOrigin,
) -> Result<CloseStatus, String> {
    ctx.cmd("CancelClose", ticket, move |tx| {
        tx.cancel_close(ticket, &origin)
    })?;
    close_status(ctx)
}

pub fn confirm_close(
    ctx: &SettlementContext<'_>,
    ticket: OpaqueId,
    origin: HumanOrigin,
) -> Result<CloseStatus, String> {
    let shutdown = next(ctx.ids)?;
    ctx.cmd("ConfirmClose", ticket, move |tx| {
        tx.confirm_close(ticket, shutdown, &origin)
    })?;
    close_status(ctx)
}

/// Stdio EOF from the GUI: close admission and stop everything without confirmation.
pub fn emergency_stop(ctx: &SettlementContext<'_>) -> Result<CloseStatus, String> {
    let (ticket, shutdown) = (next(ctx.ids)?, next(ctx.ids)?);
    ctx.cmd("EmergencyClose", ticket, move |tx| {
        tx.emergency_close(ticket, shutdown)
    })?;
    close_status(ctx)
}

/// Waits (bounded) for every target runner to conclude, settles proven-absent targets,
/// then commits the receipt. Callable again after FAILED: only unresolved targets change.
/// An `Err` means no receipt was committed and nothing may be acknowledged. READY seals
/// the session in the same commit, so the helper then drops the engine (its writer is
/// fenced) instead of calling `Engine::shutdown`.
pub fn finish_close(
    ctx: &SettlementContext<'_>,
    clock: &dyn Clock,
    wait: CloseWait,
    disconnected: bool,
) -> Result<ShutdownReceipt, String> {
    let deadline = clock
        .now()
        .monotonic_nanos
        .saturating_add(u64::try_from(wait.budget.as_nanos()).unwrap_or(u64::MAX));
    let mut s = close_status(ctx)?;
    while s
        .targets
        .iter()
        .any(|(_, f)| f.stop == TargetStop::Stopping)
        && clock.now().monotonic_nanos < deadline
    {
        std::thread::sleep(wait.poll);
        s = close_status(ctx)?;
    }
    let Some(ticket) = s.ticket.as_ref().map(|t| t.ticket) else {
        return Err("no close to finish".into());
    };
    for (attempt, f) in &s.targets {
        // Settlement failures stay visible as unsealed targets in the receipt.
        if matches!(f.stop, TargetStop::Stopped { .. }) && !f.sealed {
            let _ = settle_attempt(ctx, *attempt, BlobFaults::default());
        }
    }
    ctx.cmd("FinalizeShutdown", ticket, move |tx| {
        tx.finalize_shutdown(disconnected)
    })?;
    close_status(ctx)?
        .receipt
        .ok_or_else(|| "receipt not readable".into())
}

impl ReadView<'_> {
    /// Durable receipt lookup across epochs; does not adopt or resume an old operation.
    pub fn shutdown_receipt(&self, id: OpaqueId) -> Result<Option<ShutdownReceipt>, StorageError> {
        let Some((t, Some((_, Some(shutdown))))) = ticket(self.conn, None, Some(id))? else {
            return Ok(None);
        };
        receipt(self.conn, t.ticket, &id_to_text(shutdown))
    }
}
