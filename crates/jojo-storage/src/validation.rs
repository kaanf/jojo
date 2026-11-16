//! Independent validation receipts and candidate acceptance (P1-10).
//!
//! A receipt binds: the sealed candidate digest, the approved validation plan (digest equal
//! to the reviewed plan's `validation_digest`), the frozen base, repository-derived changed
//! paths and a validator report stored as a verified blob. The engine itself decides the
//! scope, baseline, inventory and approval-freshness verdicts; the validator only reports
//! which tests ran. Acceptance is a separate append-only assessment, never implied by
//! invocation success or by a passing receipt alone.
use jojo_domain::identity::{ArtifactId, OpaqueId, Revision};
use jojo_domain::validation::{TestStatus, ValidationPlan, check_inventory, check_scope};

use crate::evidence::{BlobFaults, BlobRef};
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::outcomes::SettlementContext;
use crate::sqlite::{Connection, Value};
use crate::worktrees::{CandidateChanges, inspect_candidate_changes};
use crate::{EventSeq, HumanOrigin, ReadView, StorageError, Tx, TxError, sha256_hex};
use jojo_domain::dependency::NodeRef;
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// Supplied by the host's validation runner, never decoded from developer provider output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidatorOrigin {
    IndependentFixture {
        name: String,
    },
    /// P2-03: the host validation runner (`validator`), which ran the approved command.
    HostRunner {
        name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatorReport {
    pub origin: ValidatorOrigin,
    pub attempt: OpaqueId,
    pub candidate: String,
    pub plan_digest: String,
    pub results: Vec<(String, TestStatus)>,
    /// Exact report bytes retained as evidence.
    pub raw: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationReceipt {
    pub event: EventSeq,
    pub attempt: OpaqueId,
    pub candidate: String,
    pub plan: String,
    pub passed: bool,
    pub reasons: Vec<String>,
    pub changes: Vec<String>,
    pub report: String,
}

impl Tx<'_> {
    /// Current development approval digest if the attempt's PRD/ADR/plan approvals are all
    /// still applicable and no invalidation stopped it; otherwise `None` (stale).
    fn current_attempt_approval(&self, id: &str) -> Result<Option<String>, TxError> {
        let Some((prd, adr, plan, bundle)) = self.conn.query_optional(
            "SELECT prd,adr,plan,bundle FROM approval_attempts WHERE id=?1",
            &[Value::Text(id)],
            |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?, r.text(3)?)),
        )?
        else {
            return Ok(None);
        };
        let invalidated = self.conn.query_optional(
            "SELECT 1 FROM attempt_stop_causes WHERE attempt=?1 AND cause='APPROVAL_INVALIDATED'",
            &[Value::Text(id)],
            |r| r.int(0),
        )?;
        let current = self.development_hash(
            ArtifactId(id_from_text(&prd)?),
            ArtifactId(id_from_text(&adr)?),
            ArtifactId(id_from_text(&plan)?),
        )?;
        Ok(
            (invalidated.is_none() && current.as_deref() == Some(bundle.as_str()))
                .then_some(bundle),
        )
    }

    /// The reviewed plan's validation digest and the attempt's exact step.
    fn approved_validation(&self, id: &str) -> Result<(String, NodeRef), TxError> {
        let (plan, revision, step, step_revision) = self
            .conn
            .query_optional(
                "SELECT plan,revision,step,step_revision FROM dependency_attempt_inputs WHERE attempt=?1",
                &[Value::Text(id)],
                |r| Ok((r.text(0)?, r.int(1)?, r.text(2)?, r.int(3)?)),
            )?
            .ok_or_else(|| deny("attempt has no frozen typed plan input"))?;
        let stored = crate::dependencies::load_plan(
            self.conn,
            ArtifactId(id_from_text(&plan)?),
            Revision(from_db(revision)?),
        )?
        .ok_or_else(|| deny("attempt plan missing"))?;
        Ok((
            stored.plan.validation_digest,
            NodeRef {
                id: id_from_text(&step)?,
                revision: Revision(from_db(step_revision)?),
            },
        ))
    }

    /// Appends one validation receipt (PASSED only if every engine-side check passes).
    pub fn record_validation(
        &mut self,
        plan: &ValidationPlan,
        evidence: &CandidateChanges,
        report: &ValidatorReport,
        blob: &BlobRef,
    ) -> Result<EventSeq, TxError> {
        let id = id_to_text(report.attempt);
        let (ValidatorOrigin::IndependentFixture { name } | ValidatorOrigin::HostRunner { name }) =
            &report.origin;
        if name.trim().is_empty() {
            return Err(deny("validator identity required"));
        }
        let candidate = self
            .conn
            .query_optional(
                "SELECT candidate FROM attempt_results WHERE attempt=?1",
                &[Value::Text(&id)],
                |r| r.optional_text(0),
            )?
            .ok_or_else(|| deny("validation needs a sealed attempt result"))?
            .ok_or_else(|| deny("sealed result has no candidate"))?;
        if evidence.attempt() != report.attempt
            || evidence.candidate() != candidate
            || report.candidate != candidate
            || sha256_hex(&[&report.raw]) != blob.digest
        {
            return Err(deny(
                "report, repository evidence and sealed candidate disagree",
            ));
        }
        self.reference_blob(blob)?;
        let canonical = plan.canonical();
        let digest = sha256_hex(&[canonical.as_bytes()]);
        let known = self.conn.query_optional(
            "SELECT 1 FROM validation_plans WHERE digest=?1",
            &[Value::Text(&digest)],
            |r| r.int(0),
        )?;
        if known.is_none() {
            let e = self.append_event("ValidationPlanStored", None, &digest)?;
            self.conn.execute(
                "INSERT INTO validation_plans VALUES (?1,?2,?3)",
                &[
                    Value::Text(&digest),
                    Value::Text(&canonical),
                    Value::Int(to_db(e.0)?),
                ],
            )?;
        }
        let mut reasons = Vec::new();
        let (approved, step) = self.approved_validation(&id)?;
        if approved != digest {
            reasons.push("validation plan is not the approved plan".to_owned());
        }
        if report.plan_digest != digest {
            reasons.push("validator ran a different plan".to_owned());
        }
        let bundle = self.current_attempt_approval(&id)?;
        if bundle.is_none() {
            reasons.push("approval is stale, revoked or invalidated".to_owned());
        }
        match plan.step(step) {
            None => reasons.push("approved plan has no entry for this step".to_owned()),
            Some(entry) => {
                let scope = check_scope(evidence.changes(), entry);
                reasons.extend(
                    scope
                        .denied
                        .iter()
                        .map(|p| format!("out-of-scope change {p:?}")),
                );
                reasons.extend(
                    scope
                        .baseline_changed
                        .iter()
                        .map(|p| format!("baseline modified {p:?}")),
                );
                reasons.extend(check_inventory(&entry.tests, &report.results));
            }
        }
        let status = if reasons.is_empty() {
            "PASSED"
        } else {
            "FAILED"
        };
        let changes: Vec<String> = evidence.changes().iter().map(|c| c.canonical()).collect();
        let event = self.append_event(
            "ValidationRecorded",
            None,
            &format!("{id}|{candidate}|{digest}|{status}"),
        )?;
        self.conn.execute(
            "INSERT INTO validation_receipts VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            &[
                Value::Int(to_db(event.0)?),
                Value::Text(&id),
                Value::Text(&candidate),
                Value::Text(&digest),
                Value::Text(evidence.base()),
                Value::Text(name),
                Value::Text(&blob.digest),
                Value::Text(&changes.join("\n")),
                Value::Text(status),
                Value::Text(&reasons.join("\n")),
                Value::Text(bundle.as_deref().unwrap_or("")),
            ],
        )?;
        Ok(event)
    }

    /// Human acceptance of a validated candidate. Rechecks freshness now: a receipt that
    /// was valid when recorded cannot authorize acceptance after approvals changed.
    pub fn accept_candidate(
        &mut self,
        attempt: OpaqueId,
        receipt: EventSeq,
        origin: HumanOrigin,
    ) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        let actor = match &origin {
            HumanOrigin::TrustedUi { actor } | HumanOrigin::FakeHumanFixture { actor } => actor,
        };
        if actor.trim().is_empty() {
            return Err(deny("human identity required"));
        }
        let latest = self.conn.query_optional(
            "SELECT event,status,plan,approval_bundle FROM validation_receipts
             WHERE attempt=?1 ORDER BY event DESC LIMIT 1",
            &[Value::Text(&id)],
            |r| Ok((r.int(0)?, r.text(1)?, r.text(2)?, r.text(3)?)),
        )?;
        let Some((event, status, plan, bundle)) = latest else {
            return Err(deny("no validation receipt"));
        };
        if from_db(event)? != receipt.0 || status != "PASSED" {
            return Err(deny("only the latest passing validation can be accepted"));
        }
        if self.current_attempt_approval(&id)?.as_deref() != Some(bundle.as_str())
            || self.approved_validation(&id)?.0 != plan
        {
            return Err(deny("validation is stale against current approvals"));
        }
        let outcome = self.conn.query_optional(
            "SELECT outcome FROM attempt_results WHERE attempt=?1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        if outcome.as_deref() != Some("SUCCEEDED") {
            return Err(deny(
                "only a succeeded invocation's candidate can be accepted",
            ));
        }
        if rejected(self.conn, &id)? {
            return Err(deny("CandidateDecided: the candidate was rejected"));
        }
        let e = self.append_event("CandidateAccepted", None, &format!("{id}|{}", receipt.0))?;
        self.conn.execute(
            "INSERT INTO candidate_acceptances VALUES (?1,?2,?3,?4)",
            &[
                Value::Text(&id),
                Value::Int(to_db(receipt.0)?),
                Value::Text(&format!("{origin:?}")),
                Value::Int(to_db(e.0)?),
            ],
        )?;
        Ok(())
    }
}

/// P2-03: what is currently known about the validation of an attempt's candidate. Only
/// `Passed` is a success; a provider's own claim about tests never reaches this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationState {
    /// No validation was ever requested.
    NotRun,
    /// Requested, but no verdict exists: still running, interrupted, or it could not be
    /// performed (for example a tampered candidate). Never treated as success.
    Unknown(String),
    Passed(EventSeq),
    Failed(EventSeq),
}

/// One requested host validation run, from the append-only event history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationRun {
    pub run: OpaqueId,
    pub candidate: String,
    pub epoch: u64,
    /// `(receipt, detail)` once ended; a run without a receipt produced no verdict.
    pub ended: Option<(Option<EventSeq>, String)>,
}

fn runs(c: &Connection, id: &str) -> Result<Vec<ValidationRun>, StorageError> {
    let rows = c.query(
        "SELECT kind,epoch,payload FROM events
         WHERE kind IN ('ValidationRequested','ValidationEnded') AND substr(payload,1,33)=?1
         ORDER BY seq",
        &[Value::Text(&format!("{id}|"))],
        |r| Ok((r.text(0)?, r.int(1)?, r.text(2)?)),
    )?;
    let mut out: Vec<ValidationRun> = Vec::new();
    for (kind, epoch, payload) in rows {
        let mut f = payload.splitn(4, '|').skip(1);
        let bad = || StorageError::corrupt("malformed validation run event");
        if kind == "ValidationRequested" {
            let candidate = f.next().ok_or_else(bad)?.to_owned();
            out.push(ValidationRun {
                run: id_from_text(f.next().ok_or_else(bad)?)?,
                candidate,
                epoch: from_db(epoch)?,
                ended: None,
            });
        } else {
            let run = id_from_text(f.next().ok_or_else(bad)?)?;
            let receipt = match f.next().ok_or_else(bad)? {
                "-" => None,
                n => Some(EventSeq(n.parse().map_err(|_| bad())?)),
            };
            let detail = f.next().unwrap_or("").to_owned();
            if let Some(r) = out.iter_mut().find(|r| r.run == run) {
                r.ended = Some((receipt, detail));
            }
        }
    }
    Ok(out)
}

fn rejected(c: &Connection, id: &str) -> Result<bool, StorageError> {
    Ok(c.query_optional(
        "SELECT 1 FROM events WHERE kind='CandidateRejected' AND substr(payload,1,33)=?1",
        &[Value::Text(&format!("{id}|"))],
        |r| r.int(0),
    )?
    .is_some())
}

fn state(c: &Connection, id: &str) -> Result<ValidationState, StorageError> {
    let receipt = c.query_optional(
        "SELECT event,status FROM validation_receipts WHERE attempt=?1 ORDER BY event DESC LIMIT 1",
        &[Value::Text(id)],
        |r| Ok((r.int(0)?, r.text(1)?)),
    )?;
    let epoch = c.query_one("SELECT max(epoch) FROM engine_sessions", &[], |r| r.int(0))?;
    let verdict = |receipt: Option<(i64, String)>| -> Result<ValidationState, StorageError> {
        Ok(match receipt {
            None => ValidationState::NotRun,
            Some((e, s)) if s == "PASSED" => ValidationState::Passed(EventSeq(from_db(e)?)),
            Some((e, _)) => ValidationState::Failed(EventSeq(from_db(e)?)),
        })
    };
    match runs(c, id)?.pop() {
        None => verdict(receipt),
        Some(run) => match run.ended {
            None if run.epoch == from_db(epoch)? => {
                Ok(ValidationState::Unknown("validation is running".into()))
            }
            None => Ok(ValidationState::Unknown(
                "validation was interrupted before it produced a result".into(),
            )),
            Some((None, detail)) => Ok(ValidationState::Unknown(detail)),
            // A later receipt (there is none in production) would supersede the run's own.
            Some((Some(e), _)) if receipt.as_ref().is_some_and(|r| r.0 as u64 >= e.0) => {
                verdict(receipt)
            }
            Some(_) => Ok(ValidationState::Unknown(
                "validation receipt is missing".into(),
            )),
        },
    }
}

/// The latest revision of the reviewed document whose bytes have this digest.
fn plan_document(
    c: &Connection,
    digest: &str,
) -> Result<Option<(ArtifactId, String)>, StorageError> {
    let rows = c.query(
        "SELECT id,content FROM review_documents d
         WHERE revision=(SELECT max(revision) FROM review_documents WHERE id=d.id)",
        &[],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?;
    for (id, content) in rows {
        if sha256_hex(&[content.as_bytes()]) == digest {
            return Ok(Some((ArtifactId(id_from_text(&id)?), content)));
        }
    }
    Ok(None)
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(512)
        .collect()
}

impl Tx<'_> {
    /// P2-03: durable acceptance of one human validation request. Everything that makes
    /// the request stale is refused here, before any command runs: a different sealed
    /// candidate, a decided candidate, a run already active in this epoch, stale or
    /// revoked approvals, or an approved plan without a command for the step.
    pub fn begin_validation(
        &mut self,
        run: OpaqueId,
        attempt: OpaqueId,
        candidate: &str,
    ) -> Result<String, TxError> {
        let id = id_to_text(attempt);
        let sealed = self
            .conn
            .query_optional(
                "SELECT candidate FROM attempt_results WHERE attempt=?1",
                &[Value::Text(&id)],
                |r| r.optional_text(0),
            )?
            .ok_or_else(|| deny("validation needs a sealed attempt result"))?
            .ok_or_else(|| deny("sealed result has no candidate"))?;
        if sealed != candidate {
            return Err(deny("StaleCandidate: this is not the sealed candidate"));
        }
        if rejected(self.conn, &id)? || self.accepted(&id)? {
            return Err(deny("CandidateDecided: the candidate was already decided"));
        }
        if runs(self.conn, &id)?
            .iter()
            .any(|r| r.ended.is_none() && r.epoch == self.epoch.0)
        {
            return Err(deny(
                "ValidationActive: a validation of this candidate is running",
            ));
        }
        self.invalidate_approvals()?;
        if self.current_attempt_approval(&id)?.is_none() {
            return Err(deny("approval is stale, revoked or invalidated"));
        }
        let (digest, step) = self.approved_validation(&id)?;
        let (document, text) = plan_document(self.conn, &digest)?
            .ok_or_else(|| deny("approved validation plan is missing"))?;
        if self.current_artifact_approval(document)?.is_none() {
            return Err(deny("missing or stale approval: validation plan"));
        }
        let plan = ValidationPlan::import(text.as_bytes()).map_err(deny)?;
        if plan.step(step).is_none_or(|s| s.command.is_empty()) {
            return Err(deny(
                "approved validation plan names no command for this step",
            ));
        }
        self.append_event(
            "ValidationRequested",
            None,
            &format!("{id}|{candidate}|{}", id_to_text(run)),
        )?;
        Ok(format!("validation requested attempt={id}"))
    }

    /// Ends a requested run exactly once, with its receipt or with the reason no verdict
    /// exists.
    pub fn end_validation(
        &mut self,
        run: OpaqueId,
        attempt: OpaqueId,
        receipt: Option<EventSeq>,
        detail: &str,
    ) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        if !runs(self.conn, &id)?
            .iter()
            .any(|r| r.run == run && r.ended.is_none())
        {
            return Err(deny("no such unfinished validation run"));
        }
        self.append_event(
            "ValidationEnded",
            None,
            &format!(
                "{id}|{}|{}|{}",
                id_to_text(run),
                receipt.map_or("-".to_owned(), |e| e.0.to_string()),
                one_line(detail)
            ),
        )?;
        Ok(())
    }

    fn accepted(&self, id: &str) -> Result<bool, TxError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT 1 FROM candidate_acceptances WHERE attempt=?1",
                &[Value::Text(id)],
                |r| r.int(0),
            )?
            .is_some())
    }

    /// P2-03 human acceptance as the protocol performs it: the human names the candidate
    /// digest and the receipt they reviewed. The candidate must still be the sealed one,
    /// the receipt must be the current passing verdict for exactly that candidate, and
    /// [`Tx::accept_candidate`] then rechecks approval freshness.
    pub fn accept_reviewed_candidate(
        &mut self,
        attempt: OpaqueId,
        candidate: &str,
        receipt: EventSeq,
        origin: HumanOrigin,
    ) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        self.invalidate_approvals()?;
        let bound = self.conn.query_optional(
            "SELECT 1 FROM validation_receipts v JOIN attempt_results r ON r.attempt=v.attempt
             WHERE v.event=?1 AND v.attempt=?2 AND v.candidate=?3 AND r.candidate=?3",
            &[
                Value::Int(to_db(receipt.0)?),
                Value::Text(&id),
                Value::Text(candidate),
            ],
            |r| r.int(0),
        )?;
        if bound.is_none() {
            return Err(deny(
                "StaleCandidate: the receipt does not validate this sealed candidate",
            ));
        }
        if state(self.conn, &id)? != ValidationState::Passed(receipt) {
            return Err(deny("only the latest passing validation can be accepted"));
        }
        self.accept_candidate(attempt, receipt, origin)
    }

    /// Human rejection of a sealed candidate. Final: a rejected candidate is never
    /// accepted, and an accepted one is never rejected.
    pub fn reject_candidate(
        &mut self,
        attempt: OpaqueId,
        candidate: &str,
        origin: HumanOrigin,
    ) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        let sealed = self.conn.query_optional(
            "SELECT candidate FROM attempt_results WHERE attempt=?1",
            &[Value::Text(&id)],
            |r| r.optional_text(0),
        )?;
        if sealed.flatten().as_deref() != Some(candidate) {
            return Err(deny("StaleCandidate: this is not the sealed candidate"));
        }
        if rejected(self.conn, &id)? || self.accepted(&id)? {
            return Err(deny("CandidateDecided: the candidate was already decided"));
        }
        self.append_event(
            "CandidateRejected",
            None,
            &format!("{id}|{candidate}|{origin:?}"),
        )?;
        Ok(())
    }
}

impl ReadView<'_> {
    pub fn validation_state(&self, attempt: OpaqueId) -> Result<ValidationState, StorageError> {
        state(self.conn, &id_to_text(attempt))
    }

    pub fn validation_runs(&self, attempt: OpaqueId) -> Result<Vec<ValidationRun>, StorageError> {
        runs(self.conn, &id_to_text(attempt))
    }

    pub fn candidate_rejected(&self, attempt: OpaqueId) -> Result<bool, StorageError> {
        rejected(self.conn, &id_to_text(attempt))
    }

    /// The approved validation plan an attempt is bound to: its digest, the attempt's
    /// step and the plan bytes, or `None` when the bound document cannot be found.
    pub fn attempt_validation_plan(
        &self,
        attempt: OpaqueId,
    ) -> Result<Option<(String, NodeRef, String)>, StorageError> {
        let Some((plan, revision, step, step_revision)) = self.conn.query_optional(
            "SELECT plan,revision,step,step_revision FROM dependency_attempt_inputs WHERE attempt=?1",
            &[Value::Text(&id_to_text(attempt))],
            |r| Ok((r.text(0)?, r.int(1)?, r.text(2)?, r.int(3)?)),
        )?
        else {
            return Ok(None);
        };
        let Some(stored) = crate::dependencies::load_plan(
            self.conn,
            ArtifactId(id_from_text(&plan)?),
            Revision(from_db(revision)?),
        )?
        else {
            return Ok(None);
        };
        let step = NodeRef {
            id: id_from_text(&step)?,
            revision: Revision(from_db(step_revision)?),
        };
        let digest = stored.plan.validation_digest;
        Ok(plan_document(self.conn, &digest)?.map(|(_, text)| (digest, step, text)))
    }

    /// P2-03 bounded review projection of one attempt: its sealed result, the validation
    /// state and the human decision. `PASS` appears only for a recorded passing receipt of
    /// the host validation; the provider's terminal status is reported separately and is
    /// never a validation state.
    pub fn ipc_candidate(&self, attempt: OpaqueId) -> Result<Json, StorageError> {
        let id = id_to_text(attempt);
        let cut = |s: &str, n: usize| text(s.chars().take(n).collect::<String>());
        let Some(result) = self.attempt_result(attempt)? else {
            return Ok(object([
                ("attempt", identity(attempt)),
                ("sealed", Json::Bool(false)),
            ]));
        };
        let records = self.conn.query_one(
            "SELECT coalesce(max(sequence),0) FROM provider_events WHERE attempt=?1",
            &[Value::Text(&id)],
            |r| r.int(0),
        )?;
        let (name, detail, receipt) = match state(self.conn, &id)? {
            ValidationState::NotRun => ("NOT_RUN", String::new(), None),
            ValidationState::Unknown(why) => ("UNKNOWN", why, None),
            ValidationState::Passed(e) => ("PASS", String::new(), Some(e)),
            ValidationState::Failed(e) => ("FAIL", String::new(), Some(e)),
        };
        let found = receipt.and_then(|e| {
            self.validation_receipts(attempt)
                .map(|all| all.into_iter().find(|r| r.event == e))
                .transpose()
        });
        let found = found.transpose()?;
        let detail = match (&found, runs(self.conn, &id)?.pop().and_then(|r| r.ended)) {
            (Some(_), Some((Some(_), why))) => why,
            _ => detail,
        };
        let decision = if self.candidate_acceptance(attempt)?.is_some() {
            "ACCEPTED"
        } else if rejected(self.conn, &id)? {
            "REJECTED"
        } else {
            "NONE"
        };
        Ok(object([
            ("attempt", identity(attempt)),
            ("sealed", Json::Bool(true)),
            ("outcome", text(format!("{:?}", result.outcome))),
            ("reason", cut(&result.reason, 512)),
            ("provider_terminal", cut(&result.terminal, 128)),
            ("candidate", result.candidate.map_or(Json::Null, text)),
            ("log_records", Json::Int(records)),
            (
                "validation",
                object([
                    ("state", text(name)),
                    ("detail", cut(&detail, 512)),
                    ("receipt", receipt.map_or(Json::Null, |e| number(e.0))),
                    (
                        "candidate",
                        found.as_ref().map_or(Json::Null, |r| text(&r.candidate)),
                    ),
                    (
                        "reasons",
                        Json::List(
                            found
                                .iter()
                                .flat_map(|r| r.reasons.iter().take(16))
                                .map(|r| cut(r, 256))
                                .collect(),
                        ),
                    ),
                    (
                        "reasons_total",
                        number(found.as_ref().map_or(0, |r| r.reasons.len() as u64)),
                    ),
                ]),
            ),
            ("decision", text(decision)),
        ]))
    }

    pub fn validation_receipts(
        &self,
        attempt: OpaqueId,
    ) -> Result<Vec<ValidationReceipt>, StorageError> {
        let rows = self.conn.query(
            "SELECT event,candidate,plan,status,reasons,changes,report FROM validation_receipts
             WHERE attempt=?1 ORDER BY event",
            &[Value::Text(&id_to_text(attempt))],
            |r| {
                Ok((
                    r.int(0)?,
                    r.text(1)?,
                    r.text(2)?,
                    r.text(3)?,
                    r.text(4)?,
                    r.text(5)?,
                    r.text(6)?,
                ))
            },
        )?;
        let lines = |s: String| -> Vec<String> { s.lines().map(str::to_owned).collect() };
        rows.into_iter()
            .map(
                |(event, candidate, plan, status, reasons, changes, report)| {
                    Ok(ValidationReceipt {
                        event: EventSeq(from_db(event)?),
                        attempt,
                        candidate,
                        plan,
                        passed: status == "PASSED",
                        reasons: lines(reasons),
                        changes: lines(changes),
                        report,
                    })
                },
            )
            .collect()
    }

    pub fn candidate_acceptance(
        &self,
        attempt: OpaqueId,
    ) -> Result<Option<EventSeq>, StorageError> {
        self.conn
            .query_optional(
                "SELECT receipt FROM candidate_acceptances WHERE attempt=?1",
                &[Value::Text(&id_to_text(attempt))],
                |r| r.int(0),
            )?
            .map(|e| from_db(e).map(EventSeq))
            .transpose()
    }
}

/// I/O stage then one writer transaction. Malformed plans, an unreproducible candidate or
/// unsupported repository state return an error and record no receipt at all.
pub fn validate_candidate(
    ctx: &SettlementContext<'_>,
    plan_bytes: &[u8],
    report: ValidatorReport,
) -> Result<ValidationReceipt, String> {
    let plan = ValidationPlan::import(plan_bytes)?;
    let attempt = report.attempt;
    let candidate = ctx
        .engine
        .reader()
        .and_then(|mut r| r.read(|v| v.attempt_result(attempt)))
        .map_err(|e| e.to_string())?
        .ok_or("validation needs a sealed attempt result")?
        .candidate
        .ok_or("sealed result has no candidate")?;
    let evidence = inspect_candidate_changes(ctx.engine, attempt, &candidate)?;
    let blob = ctx.store.put(&report.raw, BlobFaults::default())?;
    let event = ctx.cmd("RecordValidation", attempt, move |tx| {
        Ok(tx
            .record_validation(&plan, &evidence, &report, &blob)?
            .0
            .to_string())
    })?;
    let event: u64 = event.parse().map_err(|_| "malformed receipt event")?;
    ctx.engine
        .reader()
        .and_then(|mut r| r.read(|v| v.validation_receipts(attempt)))
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|r| r.event.0 == event)
        .ok_or_else(|| "receipt not readable".into())
}
