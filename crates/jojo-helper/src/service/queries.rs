//! Read-only requests of the helper: Hello, the queries of protocol 1.0 to 1.2 and the
//! P2-03 review queries. Split out of `handle` (R-02, helper part); behaviour unchanged.
use super::*;

impl Service {
    pub(super) fn hello(&mut self, e: &Envelope) -> Result<Json, String> {
        if self.session.is_some_and(|s| s != e.session) {
            return Err("SessionMismatch".into());
        }
        self.session = Some(e.session);
        self.compatible = e.major == MAJOR as u64 && e.minor <= MINOR as u64;
        Ok(object([
            ("compatible", Json::Bool(self.compatible)),
            ("epoch", number(self.engine.epoch().0)),
            ("max_frame", number(MAX_FRAME as u64)),
            ("max_page", number(8)),
            ("admission", text("closed until reviewed recovery start")),
            ("commands", text(protocol::names(Class::Mutation, e.minor))),
            ("queries", text(protocol::names(Class::Query, e.minor))),
            (
                "slice",
                Json::Bool(e.minor >= 1 && self.config.slice.is_some()),
            ),
        ]))
    }
    /// The queries answered from one read snapshot.
    pub(super) fn query(
        &mut self,
        kind: RequestKind,
        e: &Envelope,
        reader: &mut Reader,
    ) -> Result<Json, String> {
        let p = &e.payload;
        match kind {
            RequestKind::Snapshot => {
                let q = protocol::SnapshotQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_snapshot(&q.section, &q.after, q.cursor, q.limit))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Events => {
                let q = protocol::EventsQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_events(q.after, q.limit))
                    .map_err(|e| e.to_string())
            }
            RequestKind::EventDetail => {
                let q = protocol::EventDetailQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_event_detail(q.seq, q.offset))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Log => {
                let q = protocol::LogQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_log(q.attempt, q.sequence, q.offset))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Receipt => {
                let q = protocol::ReceiptQuery::parse(p)?;
                reader
                    .receipt(CommandId(q.command))
                    .map(|r| r.map_or(Json::Null, receipt))
                    .map_err(|e| e.to_string())
            }
            RequestKind::ShutdownReceipt => {
                let q = protocol::ShutdownReceiptQuery::parse(p)?;
                reader
                    .read(|v| v.shutdown_receipt(q.ticket))
                    .map(|r| r.as_ref().map_or(Json::Null, shutdown_json))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Recovery => self.recovery_rows(e),
            RequestKind::CloseStatus => Ok(status_json(self.status()?)),
            RequestKind::ReviewBundle => {
                let q = protocol::ReviewBundleQuery::parse(p)?;
                let b = reader
                    .read(|v| v.review_bundle(q.subject))
                    .map_err(|e| e.to_string())?;
                Ok(b.map_or(Json::Null, review_bundle_json))
            }
            RequestKind::ApprovedPlan => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::ApprovedPlanQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_approved_plan(q.after))
                    .map_err(|e| e.to_string())
            }
            // Protocol 1.2 (M2): Feature Chat.
            RequestKind::FeatureChat => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::FeatureChatQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_feature_chat(q.feature, q.after))
                    .map_err(|e| e.to_string())
            }
            RequestKind::ChatText => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::ChatTextQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_chat_text(q.subject, q.part, q.offset))
                    .map_err(|e| e.to_string())
            }
            RequestKind::ChatIndex => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                reader
                    .read(|v| v.ipc_chat_index())
                    .map_err(|e| e.to_string())
            }
            // Protocol 1.3 (M3): project records.
            RequestKind::Projects => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                reader.read(|v| v.ipc_projects()).map_err(|e| e.to_string())
            }
            _ => unreachable!("not a snapshot query: {kind:?}"),
        }
    }
    fn recovery_rows(&self, e: &Envelope) -> Result<Json, String> {
        let q = protocol::RecoveryQuery::parse(&e.payload)?;
        // Protocol 1.1 rows also carry the assessment as structured fields.
        let structured = e.minor >= 1;
        let rows = self
            .recovery
            .subjects()
            .iter()
            .skip(q.offset)
            .take(q.limit)
            .map(|s| {
                let mut row = vec![
                    ("attempt", identity(s.attempt())),
                    ("assessment", text(format!("{:?}", s.assessment()))),
                    (
                        "findings",
                        text(s.findings().chars().take(512).collect::<String>()),
                    ),
                    (
                        "detail_complete",
                        Json::Bool(s.findings().chars().count() <= 512),
                    ),
                ];
                if structured {
                    row.extend(protocol::assessment_fields(s.assessment()));
                }
                object(row)
            })
            .collect();
        Ok(object([
            ("digest", text(self.recovery.digest())),
            ("rows", Json::List(rows)),
        ]))
    }
    /// Protocol 1.1: a fresh inspection; its digest binds the next resolution.
    pub(super) fn recovery_inspect(&mut self) -> Result<Json, String> {
        self.reinspect()?;
        Ok(object([
            ("digest", text(self.recovery.digest())),
            ("subjects", number(self.recovery.subjects().len() as u64)),
            (
                "other_quarantined",
                number(self.recovery.other_quarantined().len() as u64),
            ),
        ]))
    }
    /// P2-03 read-only review queries. Changed paths and the diff are derived from the
    /// repository each time, after the sealed digest is re-checked; nothing here reads a
    /// path, file list or test result supplied by a provider or by the client.
    pub(super) fn review_query(&mut self, kind: RequestKind, e: &Envelope) -> Result<Json, String> {
        let p = &e.payload;
        if !kind.shaped(p) {
            return Err("InvalidPayload: unexpected or missing fields".into());
        }
        let attempt = protocol::attempt(p)?;
        let mut reader = self.engine.reader().map_err(|e| e.to_string())?;
        let page = |body: &str, offset: u64, complete: bool| {
            let total = body.chars().count() as u64;
            let chunk: String = body.chars().skip(offset as usize).take(4096).collect();
            object([
                ("attempt", identity(attempt)),
                ("offset", number(offset)),
                (
                    "next",
                    number(offset.saturating_add(chunk.chars().count() as u64)),
                ),
                ("total", number(total)),
                ("complete", Json::Bool(complete)),
                ("text", text(chunk)),
            ])
        };
        match kind {
            RequestKind::Candidate => reader
                .read(|v| v.ipc_candidate(attempt))
                .map_err(|e| e.to_string()),
            RequestKind::CandidateChanges => {
                let protocol::CandidateChangesQuery {
                    candidate,
                    offset,
                    limit,
                    ..
                } = protocol::CandidateChangesQuery::parse(p)?;
                let plan = reader
                    .read(|v| v.attempt_validation_plan(attempt))
                    .map_err(|e| e.to_string())?;
                drop(reader);
                let changes = verify_sealed_candidate(&self.engine, attempt, &candidate)
                    .map_err(|why| format!("CandidateUnavailable: {why}"))?;
                let entry = plan.and_then(|(_, step, text)| {
                    ValidationPlan::import(text.as_bytes())
                        .ok()
                        .and_then(|plan| plan.step(step).cloned())
                });
                // Without the approved scope there is no verdict, and never a pass.
                let verdict = entry
                    .as_ref()
                    .map(|s| jojo_domain::validation::check_scope(changes.changes(), s));
                let scope = |path: &str| match &verdict {
                    None => "UNKNOWN",
                    Some(v) if v.baseline_changed.iter().any(|b| b == path) => "BASELINE",
                    Some(v) if v.denied.iter().any(|d| d == path) => "OUT_OF_SCOPE",
                    Some(_) => "ALLOWED",
                };
                Ok(object([
                    ("attempt", identity(attempt)),
                    ("candidate", text(candidate)),
                    ("base", text(changes.base())),
                    ("total", number(changes.changes().len() as u64)),
                    (
                        "scope",
                        text(match &verdict {
                            None => "UNKNOWN",
                            Some(v) if v.passed() => "PASS",
                            Some(_) => "FAIL",
                        }),
                    ),
                    (
                        "rows",
                        Json::List(
                            changes
                                .changes()
                                .iter()
                                .skip(offset)
                                .take(limit)
                                .map(|c| {
                                    object([
                                        (
                                            "path",
                                            text(c.path.chars().take(512).collect::<String>()),
                                        ),
                                        (
                                            "detail_complete",
                                            Json::Bool(c.path.chars().count() <= 512),
                                        ),
                                        ("kind", text(format!("{:?}", c.kind))),
                                        ("scope", text(scope(&c.path))),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ]))
            }
            RequestKind::CandidateDiff => {
                let protocol::CandidateDiffQuery {
                    candidate, offset, ..
                } = protocol::CandidateDiffQuery::parse(p)?;
                drop(reader);
                let (diff, complete) = candidate_diff(&self.engine, attempt, &candidate)
                    .map_err(|why| format!("CandidateUnavailable: {why}"))?;
                Ok(page(&diff, offset, complete))
            }
            _ => {
                let protocol::ValidationReportQuery {
                    receipt: event,
                    offset,
                    ..
                } = protocol::ValidationReportQuery::parse(p)?;
                let found = reader
                    .read(|v| v.validation_receipts(attempt))
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .find(|r| r.event.0 == event);
                let Some(found) = found else {
                    return Ok(Json::Null);
                };
                let bytes = self
                    .store
                    .read(&found.report)
                    .map_err(|e| format!("ReportUnavailable: {e}"))?;
                Ok(page(&String::from_utf8_lossy(&bytes), offset, true))
            }
        }
    }
}

fn review_bundle_json(b: ReviewBundle) -> Json {
    object([
        ("subject", identity(b.subject.0)),
        ("revision", number(b.revision.0)),
        ("hash", text(b.hash)),
        ("content_hash", text(b.content_hash)),
        ("displayed_hash", text(b.displayed_hash)),
        ("context_hash", text(b.context_hash)),
        (
            "upstream",
            Json::List(b.upstream.into_iter().map(|s| number(s.0)).collect()),
        ),
        (
            "prior_decision",
            b.prior_decision.map_or(Json::Null, |s| number(s.0)),
        ),
    ])
}
