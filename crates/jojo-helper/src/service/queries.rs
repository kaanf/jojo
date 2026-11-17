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
                    .read(|v| v.ipc_events(q.after, q.limit, e.minor >= 6, e.minor >= 7))
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
                    .read(|v| v.ipc_feature_chat(q.feature, q.after, e.minor >= 8))
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
                    .read(|v| v.ipc_chat_index(e.minor >= 6))
                    .map_err(|e| e.to_string())
            }
            // Protocol 1.3 (M3): project records.
            RequestKind::Projects => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                reader.read(|v| v.ipc_projects()).map_err(|e| e.to_string())
            }
            RequestKind::PlanRun => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::PlanCommand::parse(p)?;
                reader
                    .read(|v| v.ipc_plan_run(q.plan))
                    .map_err(|e| e.to_string())
            }
            // Protocol 1.4 (D3): what the helper knows about each provider's executable.
            RequestKind::Executables => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                self.executables(reader)
            }
            // Protocol 1.5 (M4): repairs, holds and reviews of plan steps.
            RequestKind::Repairs => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                reader.read(|v| v.ipc_repairs()).map_err(|e| e.to_string())
            }
            // Protocol 1.6: user-facing data; 1.7: the Dashboard's.
            RequestKind::Attempts | RequestKind::Documents | RequestKind::Features => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::OverviewQuery::parse(p)?;
                reader
                    .read(|v| match kind {
                        RequestKind::Attempts => v.ipc_attempts(q.before, e.minor >= 7),
                        RequestKind::Features => v.ipc_features(q.before),
                        _ => v.ipc_documents(q.before),
                    })
                    .map_err(|e| e.to_string())
            }
            // Protocol 1.9: the document, decision, plan and attempt details of the
            // rebuilt screens.
            RequestKind::Document => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::DocumentQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_document(q.document, q.revision, q.offset))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Decisions => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::OverviewQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_decisions(q.before))
                    .map_err(|e| e.to_string())
            }
            RequestKind::PlanDetail => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::PlanDetailQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_plan_detail(q.plan))
                    .map_err(|e| e.to_string())
            }
            RequestKind::AttemptDetail => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::AttemptDetailQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_attempt_detail(q.attempt))
                    .map_err(|e| e.to_string())
            }
            _ => unreachable!("not a snapshot query: {kind:?}"),
        }
    }
    /// D3: per provider, the executable as it is now (pinned, given by path, discovered
    /// or not configured) and, from the database, the one of its last run and the latest
    /// change between two runs. Asking may start a background check; it never waits.
    fn executables(&self, reader: &mut Reader) -> Result<Json, String> {
        let record = |r: &ExecutableRecord| {
            object([
                ("path", text(r.path.clone())),
                ("version", text(r.version.clone())),
                ("sha256", text(r.sha256.clone())),
                ("profiled", Json::Bool(r.profiled)),
                ("event", number(r.event)),
            ])
        };
        let mut rows = Vec::new();
        for provider in [ProviderKind::ClaudeCode, ProviderKind::Codex] {
            let now = self.executable_now(provider);
            let (last, change) = reader
                .read(|v| v.executable_record(provider))
                .map_err(|e| e.to_string())?;
            let found = now.found.as_ref();
            let optional = |value: Option<String>| value.map_or(Json::Null, text);
            rows.push(object([
                ("provider", text(format!("{provider:?}"))),
                ("source", text(now.source)),
                ("state", text(now.state)),
                ("detail", text(now.detail)),
                ("entry", optional(found.and_then(|f| f.entry.clone()))),
                ("path", optional(found.map(|f| f.path.clone()))),
                ("version", optional(found.map(|f| f.version.clone()))),
                ("sha256", optional(found.map(|f| f.sha256.clone()))),
                (
                    "profiled",
                    found.map_or(Json::Null, |f| Json::Bool(f.profiled)),
                ),
                ("last_run", last.as_ref().map_or(Json::Null, record)),
                (
                    "change",
                    change.as_ref().map_or(Json::Null, |c| {
                        object([
                            ("event", number(c.event)),
                            ("previous", record(&c.previous)),
                            ("current", record(&c.current)),
                        ])
                    }),
                ),
            ]));
        }
        Ok(object([("providers", Json::List(rows))]))
    }
    fn executable_now(&self, provider: ProviderKind) -> ExecutableNow {
        let pinned = match provider {
            ProviderKind::Codex => self
                .config
                .slice
                .as_ref()
                .and_then(|s| s.codex.as_ref())
                .map(|c| (c.executable.clone(), c.version.clone(), c.sha256.clone())),
            ProviderKind::ClaudeCode => self
                .claude
                .as_ref()
                .map(|c| (c.executable.clone(), c.version.clone(), c.sha256.clone())),
        };
        if let Some((path, version, sha256)) = pinned {
            let found = |state: &'static str, detail: String| ExecutableNow {
                source: "pinned",
                state,
                detail,
                found: Some(ExecutableShown {
                    entry: None,
                    path: path.display().to_string(),
                    profiled: version == profiled_version(provider),
                    version: version.clone(),
                    sha256: sha256.clone(),
                }),
            };
            return match jojo_storage::begin_check(&path) {
                ExecutableCheck::Checking => found("checking", String::new()),
                ExecutableCheck::Unusable(why) => found("refused", why),
                ExecutableCheck::Known if version != profiled_version(provider) => {
                    found("refused", "not the profiled version".into())
                }
                ExecutableCheck::Known => match executable_digest(&path) {
                    Ok(digest) if digest == sha256 => found("ready", String::new()),
                    Ok(_) => found("refused", "digest differs from the pinned digest".into()),
                    Err(why) => found("refused", why),
                },
            };
        }
        let Some(discovered) = &self.discovered else {
            return ExecutableNow::unconfigured();
        };
        let (Some(lookup), Some(unpinned)) =
            (discovered.find(provider), discovered.provider(provider))
        else {
            return ExecutableNow::unconfigured();
        };
        let source = if unpinned.executable.is_some() {
            "path"
        } else {
            "discovered"
        };
        match lookup {
            Lookup::Checking => ExecutableNow {
                source,
                state: "checking",
                detail: String::new(),
                found: None,
            },
            Lookup::Refused(why) => ExecutableNow {
                source,
                state: "refused",
                detail: why,
                found: None,
            },
            Lookup::Found(f) => ExecutableNow {
                source,
                state: "ready",
                detail: String::new(),
                found: Some(ExecutableShown {
                    entry: f.entry.map(|e| e.display().to_string()),
                    path: f.path.display().to_string(),
                    version: f.version,
                    sha256: f.sha256,
                    profiled: f.profiled,
                }),
            },
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

/// D3: one provider's executable as the helper knows it now.
struct ExecutableNow {
    source: &'static str,
    state: &'static str,
    detail: String,
    found: Option<ExecutableShown>,
}
struct ExecutableShown {
    entry: Option<String>,
    path: String,
    version: String,
    sha256: String,
    profiled: bool,
}
impl ExecutableNow {
    fn unconfigured() -> Self {
        Self {
            source: "none",
            state: "unconfigured",
            detail: String::new(),
            found: None,
        }
    }
}
