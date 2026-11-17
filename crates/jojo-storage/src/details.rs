//! Protocol 1.9: the details of the rebuilt screens. `PlanDetail` describes each step of
//! one execution plan as the Execution Plan screen draws it; `AttemptDetail` what the
//! core records about one attempt beyond `Attempts` (Agent Runs, Source Control,
//! Verification & Evidence). Read-only and derived; nothing here is stored.
use crate::dependencies::load_plan;
use crate::features::{attempt_details, event_summary};
use crate::model::{from_db, id_from_text, id_to_text};
use crate::overview::{
    approval_state, cut, document_feature, feature_fields, identity_text, optional, step_names,
    time_of,
};
use crate::recovery::parse_session;
use crate::sqlite::{Connection, Value};
use crate::validation::{ValidationState, state};
use crate::{ReadView, StorageError};
use jojo_domain::dependency::NodeRef;
use jojo_domain::identity::{ArtifactId, OpaqueId, Revision};
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;
use jojo_domain::project::provider_name;
use jojo_domain::validation::{StepValidation, ValidationPlan};

/// The most events of one attempt's timeline, and the most claims and validation runs.
pub const DETAIL_ROWS: usize = 64;

fn texts(items: impl IntoIterator<Item = String>) -> Json {
    Json::List(items.into_iter().map(text).collect())
}

/// What a step's validation plan entry declares, as the screens show it.
fn checks_json(step: Option<&StepValidation>) -> [(&'static str, Json); 4] {
    let paths = |list: &[jojo_domain::validation::ScopePath]| {
        texts(list.iter().map(|p| p.as_str().to_owned()))
    };
    match step {
        Some(s) => [
            ("write_scope", paths(&s.write_scope)),
            ("baseline", paths(&s.baseline)),
            ("tests", texts(s.tests.clone())),
            ("command", texts(s.command.clone())),
        ],
        None => [
            ("write_scope", Json::List(vec![])),
            ("baseline", Json::List(vec![])),
            ("tests", Json::List(vec![])),
            ("command", Json::List(vec![])),
        ],
    }
}

/// One attempt of a step, oldest first: its ordinal, outcome, times and host validation.
fn step_attempts(c: &Connection, step: &str) -> Result<Vec<(String, Json)>, StorageError> {
    let rows = c.query(
        "SELECT a.attempt, r.outcome,
           (SELECT issue_event FROM process_launches WHERE attempt=a.attempt
            ORDER BY generation LIMIT 1),
           r.event, r.candidate
         FROM scheduler_attempts a LEFT JOIN attempt_results r ON r.attempt=a.attempt
         WHERE a.step=?1 ORDER BY a.rowid",
        &[Value::Text(step)],
        |r| {
            Ok((
                r.text(0)?,
                r.optional_text(1)?,
                r.optional_int(2)?,
                r.optional_int(3)?,
                r.optional_text(4)?,
            ))
        },
    )?;
    let mut out = Vec::new();
    for (index, (attempt, outcome, started, ended, candidate)) in rows.into_iter().enumerate() {
        let validation = if candidate.is_some() {
            Some(match state(c, &attempt)? {
                ValidationState::NotRun => "NotRun",
                ValidationState::Unknown(_) => "Unknown",
                ValidationState::Passed(_) => "Passed",
                ValidationState::Failed(_) => "Failed",
            })
        } else {
            None
        };
        let accepted = c
            .query_optional(
                "SELECT 1 FROM candidate_acceptances WHERE attempt=?1",
                &[Value::Text(&attempt)],
                |r| r.int(0),
            )?
            .is_some();
        let json = object([
            ("attempt", identity_text(&attempt)?),
            ("ordinal", number(index as u64 + 1)),
            (
                "outcome",
                optional(outcome.map(|o| {
                    match o.as_str() {
                        "SUCCEEDED" => "Succeeded",
                        "FAILED" => "Failed",
                        "CANCELLED" => "Cancelled",
                        _ => "Interrupted",
                    }
                    .to_owned()
                })),
            ),
            (
                "started_at_unix_ms",
                started.map_or(Ok(Json::Null), |s| time_of(c, s))?,
            ),
            (
                "ended_at_unix_ms",
                ended.map_or(Ok(Json::Null), |s| time_of(c, s))?,
            ),
            ("validation", optional(validation.map(str::to_owned))),
            ("accepted", Json::Bool(accepted)),
        ]);
        out.push((attempt, json));
    }
    Ok(out)
}

/// The state of a step of a plan that has no run, from its newest attempt.
fn state_without_run(c: &Connection, attempt: Option<&str>) -> Result<&'static str, StorageError> {
    let Some(attempt) = attempt else {
        return Ok("Waiting");
    };
    let row = c.query_optional(
        "SELECT outcome, candidate FROM attempt_results WHERE attempt=?1",
        &[Value::Text(attempt)],
        |r| Ok((r.text(0)?, r.optional_text(1)?)),
    )?;
    let Some((outcome, candidate)) = row else {
        return Ok("Running");
    };
    match outcome.as_str() {
        "SUCCEEDED" if candidate.is_some() => {}
        "SUCCEEDED" => return Ok("Failed"),
        "CANCELLED" => return Ok("Cancelled"),
        _ => return Ok("Failed"),
    }
    let accepted = c
        .query_optional(
            "SELECT 1 FROM candidate_acceptances WHERE attempt=?1",
            &[Value::Text(attempt)],
            |r| r.int(0),
        )?
        .is_some();
    if accepted {
        return Ok("Accepted");
    }
    Ok(match state(c, attempt)? {
        ValidationState::NotRun | ValidationState::Unknown(_) => "Validating",
        ValidationState::Passed(_) => "Review",
        ValidationState::Failed(_) => "Failed",
    })
}

/// Each step's wave: 1 without prerequisites, else one more than its latest prerequisite.
/// `None` for a step on a cycle.
fn waves(depends: &[Vec<usize>]) -> Vec<Option<usize>> {
    let mut wave: Vec<Option<usize>> = vec![None; depends.len()];
    for _ in 0..depends.len() {
        for i in 0..depends.len() {
            if wave[i].is_some() {
                continue;
            }
            let prior: Option<Vec<usize>> = depends[i].iter().map(|&d| wave[d]).collect();
            if let Some(prior) = prior {
                wave[i] = Some(prior.into_iter().max().map_or(1, |w| w + 1));
            }
        }
    }
    wave
}

impl ReadView<'_> {
    /// Protocol 1.9: the newest revision of an execution plan, step by step: names, kind,
    /// provider, prerequisites, wave, what the validation plan declares, state (from the
    /// plan's run, else from the step's newest attempt), whether it could start now, its
    /// attempts and what blocks it; and the plan checks (a cycle, missing prerequisites,
    /// steps whose write scopes overlap). `null` when the core has no such plan.
    pub fn ipc_plan_detail(&self, plan: ArtifactId) -> Result<Json, StorageError> {
        let c = self.conn;
        let id = id_to_text(plan.0);
        let Some(revision) = c
            .query_optional(
                "SELECT max(revision) FROM execution_plans WHERE plan=?1",
                &[Value::Text(&id)],
                |r| r.optional_int(0),
            )?
            .flatten()
        else {
            return Ok(Json::Null);
        };
        let Some(stored) = load_plan(c, plan, Revision(from_db(revision)?))? else {
            return Ok(Json::Null);
        };
        let turn = c.query_optional(
            "SELECT turn, validation FROM lead_plan_subjects WHERE plan=?1",
            &[Value::Text(&id)],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )?;
        let checks = match &turn {
            Some((_, validation)) => self
                .review_document_content(ArtifactId(id_from_text(validation)?))?
                .and_then(|d| ValidationPlan::import(d.as_bytes()).ok()),
            None => None,
        };
        let kinds: Vec<Option<String>> = match &turn {
            Some((turn, _)) => {
                let reply = c.query_optional(
                    "SELECT reply FROM lead_turn_results WHERE turn=?1",
                    &[Value::Text(turn)],
                    |r| r.text(0),
                )?;
                match reply.as_deref().map(|r| {
                    jojo_domain::planner::parse_reply(jojo_domain::planner::TurnKind::Plan, r)
                }) {
                    Some(Ok(jojo_domain::planner::LeadReply::Plan(draft))) => {
                        draft.steps.into_iter().map(|s| s.kind).collect()
                    }
                    _ => vec![],
                }
            }
            None => vec![],
        };
        let run = self.plan_run(plan)?;
        let (approval, _, _) = approval_state(c, plan, false)?;
        let nodes: Vec<NodeRef> = stored.plan.execution.nodes.clone();
        let index_of = |id: OpaqueId| nodes.iter().position(|n| n.id == id);
        let mut missing = Vec::new();
        let mut depends: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
        for edge in &stored.plan.execution.edges {
            match (index_of(edge.prerequisite.id), index_of(edge.dependent.id)) {
                (Some(p), Some(d)) => depends[d].push(p),
                (None, Some(_)) => missing.push(identity(edge.prerequisite.id)),
                _ => {}
            }
        }
        let wave = waves(&depends);
        let cycle = wave.iter().any(Option::is_none);
        let mut keys = Vec::new();
        for node in &nodes {
            let (key, _) = step_names(c, &id_to_text(node.id))?;
            keys.push(match key {
                Json::Text(k) => Some(k),
                _ => None,
            });
        }
        let label = |i: usize| keys[i].clone().unwrap_or_else(|| format!("step {}", i + 1));
        let entries: Vec<Option<&StepValidation>> = nodes
            .iter()
            .map(|n| checks.as_ref().and_then(|v| v.step(*n)))
            .collect();
        let mut conflicts = Vec::new();
        for a in 0..nodes.len() {
            for b in a + 1..nodes.len() {
                let (Some(x), Some(y)) = (entries[a], entries[b]) else {
                    continue;
                };
                let shared: Vec<String> = x
                    .write_scope
                    .iter()
                    .filter(|p| y.write_scope.iter().any(|q| p.overlaps(q)))
                    .map(|p| p.as_str().to_owned())
                    .collect();
                if !shared.is_empty() {
                    conflicts.push(object([
                        ("steps", texts([label(a), label(b)])),
                        ("paths", texts(shared)),
                    ]));
                }
            }
        }
        let mut states: Vec<&'static str> = Vec::new();
        let mut details: Vec<String> = Vec::new();
        let mut histories = Vec::new();
        for node in &nodes {
            let step = id_to_text(node.id);
            let attempts = step_attempts(c, &step)?;
            let from_run = run
                .as_ref()
                .and_then(|r| r.steps.iter().find(|s| s.step == *node));
            match from_run {
                Some(s) => {
                    states.push(s.state.name());
                    details.push(s.detail.chars().take(512).collect());
                }
                None => {
                    states.push(state_without_run(
                        c,
                        attempts.last().map(|(a, _)| a.as_str()),
                    )?);
                    details.push(String::new());
                }
            }
            histories.push(attempts);
        }
        let mut steps = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            let step = id_to_text(node.id);
            let (key, title) = step_names(c, &step)?;
            let open = c.query(
                "SELECT q.prompt, q.blocking FROM step_questions q WHERE q.step=?1
                 AND NOT EXISTS(SELECT 1 FROM step_answers a WHERE a.question=q.question)",
                &[Value::Text(&step)],
                |r| Ok((r.text(0)?, r.int(1)? != 0)),
            )?;
            let mut blockers = Vec::new();
            if approval != "Approved" {
                blockers.push(object([
                    ("kind", text("Approval")),
                    ("reason", text("This plan revision is not approved.")),
                ]));
            }
            for (prompt, blocking) in &open {
                if *blocking {
                    blockers.push(object([
                        ("kind", text("Question")),
                        ("reason", text(cut(prompt))),
                    ]));
                }
            }
            let unmet: Vec<String> = depends[i]
                .iter()
                .filter(|&&p| !matches!(states[p], "Accepted" | "Integrated"))
                .map(|&p| label(p))
                .collect();
            if !unmet.is_empty() {
                blockers.push(object([
                    ("kind", text("Dependency")),
                    ("reason", text(format!("Waits for {}.", unmet.join(", ")))),
                ]));
            }
            if matches!(
                states[i],
                "Blocked" | "NeedsHuman" | "Conflict" | "Refused" | "Failed"
            ) && !details[i].is_empty()
            {
                blockers.push(object([
                    ("kind", text("Step")),
                    ("reason", text(details[i].clone())),
                ]));
            }
            let eligible = match &run {
                Some(_) => states[i] == "Ready",
                None => histories[i].is_empty() && blockers.is_empty(),
            };
            let provider = stored
                .plan
                .resources
                .iter()
                .find(|r| r.step == *node)
                .map(|r| text(provider_name(r.provider)));
            let mut fields = vec![
                ("step", identity(node.id)),
                ("key", key),
                ("title", title),
                ("kind", optional(kinds.get(i).cloned().flatten())),
                ("provider", provider.unwrap_or(Json::Null)),
                ("depends_on", texts(depends[i].iter().map(|&p| label(p)))),
                ("wave", wave[i].map_or(Json::Null, |w| number(w as u64))),
            ];
            fields.extend(checks_json(entries[i]));
            fields.extend([
                ("state", text(states[i])),
                ("detail", text(details[i].clone())),
                ("eligible", Json::Bool(eligible)),
                (
                    "attempts",
                    Json::List(histories[i].iter().map(|(_, j)| j.clone()).collect()),
                ),
                ("blockers", Json::List(blockers)),
            ]);
            steps.push(object(fields));
        }
        let (feature, feature_name) = feature_fields(c, document_feature(c, &id)?)?;
        Ok(object([
            ("plan", identity(plan.0)),
            ("revision", Json::Int(revision)),
            ("approval", text(approval)),
            ("feature", feature),
            ("feature_name", feature_name),
            ("run", optional(run.as_ref().map(|r| r.state.clone()))),
            ("steps", Json::List(steps)),
            (
                "checks",
                object([
                    ("cycle", Json::Bool(cycle)),
                    ("missing", Json::List(missing)),
                    ("conflicts", Json::List(conflicts)),
                ]),
            ),
        ]))
    }

    /// Protocol 1.9: one attempt beyond `Attempts`: its provider session, root process,
    /// newest heartbeat, worktree and candidate head, the attempt it continues, its
    /// resource claims, what its step's validation plan declares, the events about it with
    /// their sentences (oldest first, at most 64), its host validation runs ("V-n") and the
    /// executable it ran. `null` when the core has no such attempt.
    pub fn ipc_attempt_detail(&self, attempt: OpaqueId) -> Result<Json, StorageError> {
        let c = self.conn;
        let id = id_to_text(attempt);
        let Some(step) = c.query_optional(
            "SELECT step FROM scheduler_attempts WHERE attempt=?1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?
        else {
            return Ok(Json::Null);
        };
        let plan = c.query_optional(
            "SELECT plan FROM dependency_attempt_inputs WHERE attempt=?1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        let feature = match &plan {
            Some(p) => document_feature(c, p)?,
            None => None,
        };
        let (key, title) = step_names(c, &step)?;
        let mut fields: Vec<(&'static str, Json)> = vec![
            ("attempt", identity(attempt)),
            ("step", identity_text(&step)?),
            ("key", key),
            ("title", title),
            (
                "plan",
                plan.as_deref().map_or(Ok(Json::Null), identity_text)?,
            ),
        ];
        let (feature_id, feature_name) = feature_fields(c, feature.clone())?;
        fields.push(("feature", feature_id));
        fields.push(("feature_name", feature_name));
        fields.extend(attempt_details(
            c,
            &id,
            plan.as_deref(),
            feature.as_deref(),
        )?);
        let provider = c.query_optional(
            "SELECT provider FROM process_launches WHERE attempt=?1 ORDER BY generation DESC LIMIT 1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        fields.push(("provider", optional(provider)));
        let session = c
            .query(
                "SELECT payload FROM provider_events WHERE attempt=?1 AND kind='SessionIdentified'
                 ORDER BY sequence DESC LIMIT 1",
                &[Value::Text(&id)],
                |r| r.text(0),
            )?
            .first()
            .and_then(|p| parse_session(p))
            .map(|s| s.session);
        fields.push(("session", optional(session)));
        let process = c.query_optional(
            "SELECT pid, pgid FROM process_records WHERE attempt=?1 AND role='ROOT'",
            &[Value::Text(&id)],
            |r| Ok((r.int(0)?, r.int(1)?)),
        )?;
        fields.push((
            "process",
            process.map_or(Json::Null, |(pid, pgid)| {
                object([("pid", Json::Int(pid)), ("pgid", Json::Int(pgid))])
            }),
        ));
        let heartbeat = c.query_optional(
            "SELECT event FROM scheduler_heartbeats WHERE attempt=?1 ORDER BY event DESC LIMIT 1",
            &[Value::Text(&id)],
            |r| r.int(0),
        )?;
        fields.push((
            "heartbeat_at_unix_ms",
            heartbeat.map_or(Ok(Json::Null), |s| time_of(c, s))?,
        ));
        let worktree = c.query_optional(
            "SELECT root, base, state, snapshot FROM owned_worktrees WHERE attempt=?1",
            &[Value::Text(&id)],
            |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?, r.optional_text(3)?)),
        )?;
        fields.push((
            "worktree",
            worktree.map_or(Json::Null, |(root, base, state, snapshot)| {
                object([
                    ("root", text(root)),
                    ("base", text(base)),
                    ("state", text(state)),
                    ("snapshot", optional(snapshot)),
                ])
            }),
        ));
        let head = c.query_optional(
            "SELECT w.head FROM worktree_candidates w JOIN attempt_results r
               ON r.attempt=w.attempt AND r.candidate=w.digest WHERE w.attempt=?1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        fields.push(("candidate_head", optional(head)));
        let continues = c.query_optional(
            "SELECT predecessor FROM continuation_plans WHERE successor=?1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        let continued = c.query_optional(
            "SELECT successor FROM continuation_plans WHERE predecessor=?1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        fields.push((
            "continues",
            continues.as_deref().map_or(Ok(Json::Null), identity_text)?,
        ));
        fields.push((
            "continued_by",
            continued.as_deref().map_or(Ok(Json::Null), identity_text)?,
        ));
        let claims = c.query(
            "SELECT resource_key, mode, state FROM resource_claims WHERE holder_id=?1
             ORDER BY claim_id LIMIT ?2",
            &[Value::Text(&id), Value::Int(DETAIL_ROWS as i64)],
            |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?)),
        )?;
        fields.push((
            "claims",
            Json::List(
                claims
                    .into_iter()
                    .map(|(resource, mode, state)| {
                        object([
                            ("resource", text(resource)),
                            ("mode", text(mode)),
                            ("state", text(state)),
                        ])
                    })
                    .collect(),
            ),
        ));
        let entry = self
            .attempt_validation_plan(attempt)?
            .and_then(|(_, node, bytes)| {
                ValidationPlan::import(bytes.as_bytes())
                    .ok()
                    .and_then(|p| p.step(node).cloned())
            });
        fields.extend(checks_json(entry.as_ref()));
        let events = c.query(
            "SELECT seq, epoch, kind, aggregate_id, payload FROM events
             WHERE aggregate_id=?1 OR instr(payload, ?1)>0 ORDER BY seq LIMIT ?2",
            &[Value::Text(&id), Value::Int(DETAIL_ROWS as i64)],
            |r| {
                Ok((
                    r.int(0)?,
                    r.int(1)?,
                    r.text(2)?,
                    r.optional_text(3)?,
                    r.text(4)?,
                ))
            },
        )?;
        let mut timeline = Vec::new();
        for (seq, epoch, kind, aggregate, payload) in events {
            let summary = event_summary(c, seq, epoch, &kind, aggregate.as_deref(), &payload)?;
            timeline.push(object([
                ("seq", Json::Int(seq)),
                ("at_unix_ms", time_of(c, seq)?),
                ("kind", text(kind)),
                ("summary", text(summary)),
            ]));
        }
        fields.push(("events", Json::List(timeline)));
        let receipts = c.query(
            "SELECT event, candidate, base, validator, status, reasons, report, approval_bundle
             FROM validation_receipts WHERE attempt=?1 ORDER BY event LIMIT ?2",
            &[Value::Text(&id), Value::Int(DETAIL_ROWS as i64)],
            |r| {
                Ok((
                    r.int(0)?,
                    r.text(1)?,
                    r.text(2)?,
                    r.text(3)?,
                    r.text(4)?,
                    r.text(5)?,
                    r.text(6)?,
                    r.text(7)?,
                ))
            },
        )?;
        let mut runs = Vec::new();
        for (index, (event, candidate, base, validator, status, reasons, report, bundle)) in
            receipts.into_iter().enumerate()
        {
            let reasons: Vec<String> = reasons
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect();
            runs.push(object([
                ("ordinal", number(index as u64 + 1)),
                ("receipt", Json::Int(event)),
                ("at_unix_ms", time_of(c, event)?),
                ("candidate", text(candidate)),
                ("base", text(base)),
                ("validator", text(validator)),
                ("passed", Json::Bool(status == "PASSED")),
                ("reasons", texts(reasons)),
                ("report", text(report)),
                ("approvals", text(bundle)),
            ]));
        }
        fields.push(("validations", Json::List(runs)));
        let executable = c.query_optional(
            "SELECT i.version, i.sha256, i.profiled FROM attempt_executables a
             JOIN executable_identities i ON i.identity=a.identity WHERE a.attempt=?1",
            &[Value::Text(&id)],
            |r| Ok((r.text(0)?, r.text(1)?, r.int(2)? != 0)),
        )?;
        fields.push((
            "executable",
            executable.map_or(Json::Null, |(version, sha256, profiled)| {
                object([
                    ("version", text(version)),
                    ("sha256", text(sha256)),
                    ("profiled", Json::Bool(profiled)),
                ])
            }),
        ));
        let evidence = c.query_one(
            "SELECT count(*) FROM attempt_evidence WHERE attempt=?1",
            &[Value::Text(&id)],
            |r| r.int(0),
        )?;
        fields.push(("evidence_records", Json::Int(evidence)));
        Ok(object(fields))
    }
}
