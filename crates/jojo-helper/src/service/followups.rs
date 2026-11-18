//! Follow-ups (protocol 1.14): the human's repair of a lead turn and the two advisory
//! reviews. Each is accepted under the client's command identity, which is also the
//! identity of the turn or review it starts, and launched through the same paths as a
//! chat turn (M2) and a plan step's review (M4).
use super::commands::rejected;
use super::*;
use jojo_domain::identity::RepositoryId;
use jojo_domain::provider::MAX_PROMPT_BYTES;
use jojo_domain::review::{
    DocumentReviewInput, ReviewInput, document_review_prompt, review_prompt,
};

impl Service {
    /// The human asks for a failed or interrupted turn again; the new turn is told why the
    /// failed one failed. Checked and accepted like a chat turn.
    pub(super) fn repair_turn(
        &mut self,
        request: CommandRequest,
        e: &Envelope,
    ) -> Result<Json, String> {
        let c = protocol::RepairTurnCommand::parse(&e.payload)?;
        let turn = e.request;
        let slice = self.config.slice.clone().ok_or("SliceNotConfigured")?;
        if self.workers.running(Work::Turn) != 0 {
            return Err("TurnRefused: another lead turn is running".into());
        }
        let failed = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.turn(c.turn)))
            .map_err(|e| e.to_string())?
            .ok_or("UnknownTurn")?;
        self.engine
            .reader()
            .and_then(|mut r| r.read(|v| v.capacity_left("a lead turn")))
            .map_err(|e| e.to_string())??;
        let adapter = self.adapter(failed.provider, &slice)?;
        let path = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.local_repository_path(failed.repository)))
            .map_err(|e| e.to_string())?
            .ok_or("TurnRefused: the repository is not registered")?;
        let repository = DisposableRoot::inspect(&slice.disposable_root)?.repository(&path)?;
        if repository.id() != failed.repository {
            return Err("TurnRefused: repository identity changed".into());
        }
        let base = repository_head(&repository).map_err(|e| format!("TurnRefused: {e}"))?;
        let fingerprint = adapter.fingerprint().clone();
        let (failed_id, note) = (c.turn, c.note);
        let r = self
            .engine
            .execute(request, move |tx| {
                let receipt = tx.accept_turn_repair(turn, failed_id, &note, &base)?;
                tx.record_turn_executable(turn, &fingerprint)?;
                Ok(receipt)
            })
            .map_err(rejected)?;
        let with = |r: Receipt, dispatch: &str| {
            let Json::Object(mut fields) = receipt(r) else {
                unreachable!("receipt is an object")
            };
            fields.push(("turn".into(), identity(turn)));
            fields.push(("dispatch".into(), text(dispatch)));
            Json::Object(fields)
        };
        if r.replayed {
            return Ok(with(r, "not repeated"));
        }
        self.finalized = false;
        match self.dispatch_turn(turn, &repository, &adapter) {
            Ok(()) => Ok(with(r, "launched")),
            Err(why) => {
                let detail = format!("not spawned: {why}");
                self.command("FinishLeadTurn", turn, move |tx| {
                    tx.finish_turn(turn, TurnOutcome::Failed, &detail, None, &[])
                })?;
                Err(format!(
                    "TurnFailed: {why}; command {:032x} is committed and nothing was launched",
                    turn.0
                ))
            }
        }
    }

    /// The other provider reviews the sealed candidate of an attempt started by hand.
    pub(super) fn review_attempt(
        &mut self,
        request: CommandRequest,
        e: &Envelope,
    ) -> Result<Json, String> {
        let c = protocol::ReviewAttemptCommand::parse(&e.payload)?;
        let subjects = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.attempt_review_subjects(c.attempt, &c.candidate)))
            .map_err(|e| e.to_string())??;
        let material = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.review_material(&subjects.subjects, subjects.step)))
            .map_err(|e| e.to_string())?
            .ok_or("ReviewRefused: the approved documents of the step are missing")?;
        let (diff, _) = candidate_diff(&self.engine, c.attempt, &c.candidate)
            .map_err(|e| format!("ReviewRefused: the diff could not be read: {e}"))?;
        let prompt = review_prompt(
            &ReviewInput {
                key: &material.key,
                title: &material.title,
                prd_revision: material.prd_revision,
                prd: &material.prd,
                adr: &material.adr,
                write_scope: &material.write_scope,
                baseline: &material.baseline,
                command: &material.command,
                diff: &diff,
                repairs: 0,
            },
            MAX_PROMPT_BYTES,
        );
        let subject = AdvisorySubject::Attempt {
            attempt: c.attempt,
            candidate: c.candidate,
        };
        self.advisory_review(
            request,
            e.request,
            subject,
            subjects.developer,
            subjects.repository,
            prompt,
        )
    }

    /// The other provider reviews one revision of a document a lead drafted.
    pub(super) fn review_document(
        &mut self,
        request: CommandRequest,
        e: &Envelope,
    ) -> Result<Json, String> {
        let c = protocol::ReviewDocumentCommand::parse(&e.payload)?;
        let subjects = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.document_review_subjects(c.document, c.revision.0)))
            .map_err(|e| e.to_string())??;
        let prompt = document_review_prompt(
            &DocumentReviewInput {
                kind: subjects.kind,
                revision: c.revision.0,
                body: &subjects.body,
                requirements: subjects
                    .requirements
                    .as_ref()
                    .map(|(revision, text)| (*revision, text.as_str())),
            },
            MAX_PROMPT_BYTES,
        );
        let subject = AdvisorySubject::Document {
            document: c.document,
            revision: c.revision.0,
        };
        self.advisory_review(
            request,
            e.request,
            subject,
            subjects.drafter,
            subjects.repository,
            prompt,
        )
    }

    /// Accepts and launches one advisory review by the provider other than `other`, in the
    /// registered repository at its HEAD. One that cannot be launched is sealed as failed.
    fn advisory_review(
        &mut self,
        request: CommandRequest,
        review: OpaqueId,
        subject: AdvisorySubject,
        other: ProviderKind,
        repository: RepositoryId,
        prompt: String,
    ) -> Result<Json, String> {
        let slice = self.config.slice.clone().ok_or("SliceNotConfigured")?;
        if self.workers.running(Work::Review) != 0 {
            return Err("ReviewRefused: another review is running".into());
        }
        let reviewer = reviewer_of(other);
        let adapter = self.adapter(reviewer, &slice)?;
        self.engine
            .reader()
            .and_then(|mut r| r.read(|v| v.capacity_left("a review")))
            .map_err(|e| e.to_string())??;
        let path = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.local_repository_path(repository)))
            .map_err(|e| e.to_string())?
            .ok_or("ReviewRefused: the repository is not registered")?;
        let local = DisposableRoot::inspect(&slice.disposable_root)?.repository(&path)?;
        if local.id() != repository {
            return Err("ReviewRefused: repository identity changed".into());
        }
        let base = repository_head(&local).map_err(|e| format!("ReviewRefused: {e}"))?;
        let accepted = AdvisoryRequest {
            review,
            subject,
            provider: reviewer,
            repository,
            base,
            prompt,
        };
        let fingerprint = adapter.fingerprint().clone();
        let r = self
            .engine
            .execute(request, move |tx| {
                let receipt = tx.accept_advisory_review(&accepted)?;
                tx.record_review_executable(review, &fingerprint)?;
                Ok(receipt)
            })
            .map_err(rejected)?;
        let with = |r: Receipt, dispatch: &str| {
            let Json::Object(mut fields) = receipt(r) else {
                unreachable!("receipt is an object")
            };
            fields.push(("review".into(), identity(review)));
            fields.push(("dispatch".into(), text(dispatch)));
            Json::Object(fields)
        };
        if r.replayed {
            return Ok(with(r, "not repeated"));
        }
        self.finalized = false;
        match self.dispatch_review(review, &local, &adapter) {
            Ok(()) => Ok(with(r, "launched")),
            Err(why) => {
                let detail = format!("not spawned: {why}");
                self.command("FinishStepReview", review, move |tx| {
                    tx.finish_review(review, ReviewOutcome::Failed, &detail, None)
                })?;
                Err(format!(
                    "ReviewFailed: {why}; command {:032x} is committed and nothing was launched",
                    review.0
                ))
            }
        }
    }
}
