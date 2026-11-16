//! Original synthetic protocol fixtures; no upstream material or CLI compatibility claim.
use jojo_domain::{attempt::AttemptBindings, identity::*, provider::*, time::ClockReading};
use jojo_test_support::provider::*;
use std::path::PathBuf;
fn capabilities() -> Capabilities {
    Capabilities {
        evidence: "synthetic-v1".into(),
        structured_events: Support::Supported,
        explicit_sessions: Support::Supported,
        exact_continuation: Support::Supported,
        incremental_input: Support::Supported,
        question_ids: Support::Supported,
        permission_response: Support::Supported,
        turn_interrupt: Support::Supported,
        process_tree: Support::Unknown,
        continuation_cwd_change: Support::Unsupported,
        auth_status: Support::Supported,
        workspace_policy: Support::Supported,
    }
}
fn spec(provider: ProviderKind, n: u128) -> LaunchSpec {
    LaunchSpec {
        attempt: AttemptId(OpaqueId(n)),
        bindings: AttemptBindings {
            feature_id: FeatureId(OpaqueId(n)),
            plan_id: PlanId(OpaqueId(n)),
            plan_revision: Revision(1),
            step_id: StepId(OpaqueId(n)),
            step_revision: Revision(1),
            repository_id: RepositoryId(OpaqueId(n)),
            worktree_id: WorktreeId(OpaqueId(n)),
        },
        engine_epoch: OpaqueId(7),
        dispatch_generation: 2,
        fingerprint: ProviderFingerprint {
            provider,
            executable: "/synthetic/fixture".into(),
            version: "fake-v1".into(),
            schema: PROVIDER_SCHEMA,
            parser: "synthetic-parser-v1".into(),
            configuration: "config-digest".into(),
        },
        source: SourceBinding {
            common_dir: "/owned/repo.git".into(),
            base_commit: "commit".into(),
            candidate_source_vector: "vector-digest".into(),
            instruction_digest: "instructions-digest".into(),
        },
        prompt_digest: "prompt-digest".into(),
        prompt: b"synthetic task".to_vec(),
        continuation: None,
        role: Role::Developer,
        permissions: PermissionEnvelope {
            approved_digest: "permission-digest".into(),
            tools: vec![],
            filesystem_roots: vec!["/owned/worktree".into()],
            network: false,
            hooks: false,
            background: false,
        },
        cwd: "/owned/worktree".into(),
        environment_allowlist: vec!["LANG".into()],
        config_references: vec!["config-digest".into()],
        timeout_millis: 1000,
        max_output_bytes: 4096,
        max_frame_bytes: 1024,
        raw_output_dir: "/owned/output".into(),
        approval_ids: [OpaqueId(10), OpaqueId(11), OpaqueId(12)],
        required_outputs: vec!["candidate".into()],
        validation_plan: "validation-digest".into(),
    }
}
fn adapter(s: &LaunchSpec, c: Capabilities) -> FakeAdapter {
    FakeAdapter::new(
        s.fingerprint.clone(),
        c,
        AuthMode::NativeSubscription,
        Readiness::Ready,
    )
}
fn session(provider: ProviderKind, id: &str) -> SessionRef {
    SessionRef {
        provider,
        session: id.into(),
        turn: "turn-1".into(),
    }
}
fn record(payload: EventPayload) -> FixtureAction {
    FixtureAction::Record(FixtureRecord {
        raw: RawReference {
            blob: "synthetic-captured-blob".into(),
            stream: Stream::Stdout,
            start: 0,
            end: 32,
        },
        observed: ClockReading {
            unix_millis: 100,
            monotonic_nanos: 10,
            epoch: ClockEpochId(OpaqueId(7)),
        },
        payload,
    })
}
fn identify(run: &mut FakeSession, id: &str) {
    run.enqueue(record(EventPayload::SessionIdentified(session(
        run.spec().fingerprint.provider,
        id,
    ))));
    assert!(matches!(run.poll(), Ok(Poll::Event(_))));
}
fn target(run: &FakeSession) -> InputTarget {
    InputTarget {
        attempt: run.spec().attempt,
        generation: 2,
        session: run.session().unwrap().clone(),
        request: "q1".into(),
        expected_revision: Revision(3),
    }
}
fn question(run: &mut FakeSession) {
    run.enqueue(record(EventPayload::QuestionRequested {
        id: "q1".into(),
        revision: Revision(3),
    }));
    run.poll().unwrap();
}
#[test]
fn two_provider_sessions_interleave_wait_fail_without_crosstalk() {
    let a = spec(ProviderKind::ClaudeCode, 1);
    let b = spec(ProviderKind::Codex, 2);
    let mut x = FakeClaude(adapter(&a, capabilities()))
        .prepare(a.clone())
        .unwrap();
    let mut y = FakeCodex(adapter(&b, capabilities()))
        .prepare(b.clone())
        .unwrap();
    identify(&mut x, "claude-session");
    identify(&mut y, "codex-session");
    x.enqueue(FixtureAction::Wait("ready".into()));
    y.enqueue(record(EventPayload::TextDelta("codex".into())));
    assert_eq!(x.poll(), Ok(Poll::Waiting("ready".into())));
    let Poll::Event(e) = y.poll().unwrap() else {
        panic!()
    };
    assert_eq!(
        (e.attempt, e.sequence, e.process_generation),
        (b.attempt, 2, 2)
    );
    assert!(x.release("wrong").is_err());
    x.release("ready").unwrap();
    x.enqueue(FixtureAction::Fail("synthetic failure".into()));
    assert_eq!(x.poll(), Err(ProviderError::Protocol));
    assert_eq!(y.poll(), Ok(Poll::Exhausted));
    assert_eq!(y.session().unwrap().session, "codex-session");
}
#[test]
fn simultaneous_sessions_from_one_adapter_are_independent() {
    let s = spec(ProviderKind::Codex, 1);
    let a = adapter(&s, capabilities());
    let mut x = a.prepare(s.clone()).unwrap();
    let mut other = s;
    other.attempt = AttemptId(OpaqueId(2));
    let mut y = a.prepare(other).unwrap();
    identify(&mut x, "one");
    identify(&mut y, "two");
    question(&mut x);
    question(&mut y);
    assert!(y.supply_input(&target(&x), b"wrong").is_err());
    x.supply_input(&target(&x), b"yes").unwrap();
    y.supply_input(&target(&y), b"no").unwrap();
}
#[test]
fn continuation_uses_exact_reference_and_new_attempt() {
    for provider in [ProviderKind::ClaudeCode, ProviderKind::Codex] {
        let mut s = spec(provider, 2);
        s.continuation = Some(Continuation {
            predecessor: AttemptId(OpaqueId(1)),
            exact: session(provider, "exact-history"),
            previous_cwd: s.cwd.clone(),
            reviewed_plan: "reviewed".into(),
        });
        let a = adapter(&s, capabilities());
        let mut run = a.prepare(s.clone()).unwrap();
        assert_eq!(run.session(), s.continuation.as_ref().map(|c| &c.exact));
        run.enqueue(record(EventPayload::SessionIdentified(session(
            provider,
            "other-history",
        ))));
        assert_eq!(run.poll(), Err(ProviderError::Correlation));
        s.continuation.as_mut().unwrap().predecessor = s.attempt;
        assert!(a.prepare(s).is_err());
    }
}
#[test]
fn continuation_unknown_and_changed_cwd_fail_closed() {
    let mut s = spec(ProviderKind::ClaudeCode, 2);
    s.continuation = Some(Continuation {
        predecessor: AttemptId(OpaqueId(1)),
        exact: session(ProviderKind::ClaudeCode, "history"),
        previous_cwd: PathBuf::from("/old"),
        reviewed_plan: "reviewed".into(),
    });
    assert!(matches!(
        adapter(&s, capabilities()).prepare(s.clone()),
        Err(ProviderError::UnsupportedContinuation)
    ));
    let mut c = capabilities();
    c.continuation_cwd_change = Support::Supported;
    c.exact_continuation = Support::Unknown;
    assert!(matches!(
        adapter(&s, c).prepare(s),
        Err(ProviderError::UnsupportedContinuation)
    ));
}
#[test]
fn unknown_auth_key_mode_and_unready_reject_before_prepare() {
    let s = spec(ProviderKind::Codex, 1);
    for auth in [
        AuthMode::Unknown,
        AuthMode::ApiKey,
        AuthMode::NotSignedIn,
        AuthMode::UnsupportedMode,
    ] {
        let a = FakeAdapter::new(
            s.fingerprint.clone(),
            capabilities(),
            auth,
            Readiness::Ready,
        );
        assert!(matches!(
            a.prepare(s.clone()),
            Err(ProviderError::AuthenticationDenied)
        ));
    }
    for r in [
        Readiness::Unknown,
        Readiness::Unavailable,
        Readiness::Unsupported,
    ] {
        let a = FakeAdapter::new(
            s.fingerprint.clone(),
            capabilities(),
            AuthMode::NativeSubscription,
            r,
        );
        assert!(matches!(a.prepare(s.clone()), Err(ProviderError::NotReady)));
    }
    let mut c = capabilities();
    c.auth_status = Support::Unknown;
    assert!(matches!(
        adapter(&s, c).prepare(s),
        Err(ProviderError::AuthenticationDenied)
    ));
}
#[test]
fn schema_version_parser_provider_config_mismatch_fail_before_launch() {
    let s = spec(ProviderKind::Codex, 1);
    let a = adapter(&s, capabilities());
    for field in 0..6 {
        let mut bad = s.clone();
        match field {
            0 => bad.fingerprint.schema += 1,
            1 => bad.fingerprint.version.push('x'),
            2 => bad.fingerprint.parser.push('x'),
            3 => bad.fingerprint.configuration.push('x'),
            4 => bad.fingerprint.executable = "/different".into(),
            _ => bad.fingerprint.provider = ProviderKind::ClaudeCode,
        }
        assert!(matches!(
            a.prepare(bad),
            Err(ProviderError::FingerprintMismatch)
        ));
    }
    assert!(FakeClaude(a).prepare(s).is_err());
}
#[test]
fn unsupported_and_unknown_optional_operations_are_explicit() {
    for support in [Support::Unsupported, Support::Unknown] {
        let s = spec(ProviderKind::ClaudeCode, 1);
        let mut c = capabilities();
        c.incremental_input = support;
        c.permission_response = support;
        c.turn_interrupt = support;
        let mut r = adapter(&s, c).prepare(s).unwrap();
        identify(&mut r, "session");
        question(&mut r);
        assert_eq!(
            r.supply_input(&target(&r), b"answer"),
            Err(ProviderError::UnsupportedInput)
        );
        assert_eq!(
            r.request_interrupt(r.spec().attempt, 2),
            Err(ProviderError::UnsupportedInterrupt)
        );
        assert_eq!(
            r.answer_permission(&ToolPermissionAnswer {
                target: target(&r),
                decision: ToolDecision::AllowOnce,
                observed_millis: 100
            }),
            Err(ProviderError::UnsupportedPermission)
        );
    }
}
#[test]
fn input_requires_exact_attempt_generation_session_turn_question_revision() {
    let s = spec(ProviderKind::Codex, 1);
    let mut r = adapter(&s, capabilities()).prepare(s).unwrap();
    identify(&mut r, "s");
    question(&mut r);
    for field in 0..7 {
        let mut t = target(&r);
        match field {
            0 => t.attempt = AttemptId(OpaqueId(9)),
            1 => t.generation += 1,
            2 => t.session.session.push('x'),
            3 => t.session.turn.push('x'),
            4 => t.request.push('x'),
            5 => t.expected_revision.0 += 1,
            _ => t.session.provider = ProviderKind::ClaudeCode,
        }
        assert_eq!(
            r.supply_input(&t, b"answer"),
            Err(ProviderError::Correlation)
        );
    }
    let t = target(&r);
    r.supply_input(&t, b"answer").unwrap();
    assert_eq!(
        r.supply_input(&t, b"replay"),
        Err(ProviderError::Correlation)
    );
}
#[test]
fn tool_permission_is_separate_from_question_and_expires() {
    let s = spec(ProviderKind::Codex, 1);
    let approvals = s.approval_ids;
    let mut r = adapter(&s, capabilities()).prepare(s).unwrap();
    identify(&mut r, "s");
    r.enqueue(record(EventPayload::PermissionRequested {
        id: "q1".into(),
        revision: Revision(3),
        deadline_millis: 200,
    }));
    r.poll().unwrap();
    assert_eq!(
        r.supply_input(&target(&r), b"approve"),
        Err(ProviderError::Correlation)
    );
    let mut answer = ToolPermissionAnswer {
        target: target(&r),
        decision: ToolDecision::Deny,
        observed_millis: 201,
    };
    assert!(r.answer_permission(&answer).is_err());
    answer.observed_millis = 200;
    r.answer_permission(&answer).unwrap();
    assert_eq!(r.spec().approval_ids, approvals);
    assert!(r.answer_permission(&answer).is_err());
}
#[test]
fn terminal_success_is_only_evidence_and_does_not_mutate_launch_or_approvals() {
    let s = spec(ProviderKind::Codex, 1);
    let mut r = adapter(&s, capabilities()).prepare(s.clone()).unwrap();
    identify(&mut r, "s");
    r.enqueue(record(EventPayload::ProviderTerminal {
        turn: "turn-1".into(),
        status: TerminalStatus::Success,
        raw_status: "synthetic-success".into(),
    }));
    let Poll::Event(e) = r.poll().unwrap() else {
        panic!()
    };
    assert!(matches!(
        e.payload,
        EventPayload::ProviderTerminal {
            status: TerminalStatus::Success,
            ..
        }
    ));
    assert_eq!(r.spec(), &s);
    assert_eq!(r.poll(), Ok(Poll::Exhausted));
    assert!(r.supply_input(&target(&r), b"approve").is_err());
}
#[test]
fn adapter_cannot_assert_os_start_or_exit() {
    for payload in [
        EventPayload::ProcessStarted {
            process: "fake-pid".into(),
        },
        EventPayload::ProcessExited { code: Some(0) },
    ] {
        let s = spec(ProviderKind::Codex, 1);
        let mut r = adapter(&s, capabilities()).prepare(s).unwrap();
        r.enqueue(record(payload));
        assert_eq!(r.poll(), Err(ProviderError::Protocol));
    }
}
#[test]
fn malformed_raw_reference_is_sticky_and_cannot_be_followed_by_success() {
    let s = spec(ProviderKind::Codex, 1);
    let mut r = adapter(&s, capabilities()).prepare(s).unwrap();
    identify(&mut r, "s");
    let FixtureAction::Record(mut bad) = record(EventPayload::TextDelta("bad".into())) else {
        panic!()
    };
    bad.raw.end = 9000;
    r.enqueue(FixtureAction::Record(bad));
    r.enqueue(record(EventPayload::ProviderTerminal {
        turn: "turn-1".into(),
        status: TerminalStatus::Success,
        raw_status: "success".into(),
    }));
    assert_eq!(r.poll(), Err(ProviderError::Limit));
    assert_eq!(r.poll(), Err(ProviderError::Protocol));
}
#[test]
fn cancelled_session_emits_no_success_and_interrupt_ack_is_not_exit() {
    let s = spec(ProviderKind::ClaudeCode, 1);
    let mut r = adapter(&s, capabilities()).prepare(s).unwrap();
    assert!(r.request_interrupt(r.spec().attempt, 1).is_err());
    r.request_interrupt(r.spec().attempt, 2).unwrap();
    assert_eq!(r.poll(), Ok(Poll::Exhausted));
    r.cancel();
    assert_eq!(r.poll(), Err(ProviderError::Cancelled));
}
#[test]
fn launch_limits_paths_permissions_and_environment_fail_closed() {
    let s = spec(ProviderKind::Codex, 1);
    let a = adapter(&s, capabilities());
    for field in 0..13 {
        let mut bad = s.clone();
        match field {
            0 => bad.prompt = vec![0; MAX_PROMPT_BYTES + 1],
            1 => bad.cwd = "relative".into(),
            2 => bad.raw_output_dir = "/owned/../escape".into(),
            3 => bad.permissions.network = true,
            4 => bad.permissions.hooks = true,
            5 => bad.permissions.background = true,
            6 => bad.environment_allowlist.push("API_KEY".into()),
            7 => bad.max_frame_bytes = 5000,
            8 => bad.dispatch_generation = 0,
            9 => bad.validation_plan.clear(),
            10 => bad.permissions.filesystem_roots.clear(),
            11 => bad.bindings.step_revision = Revision(0),
            _ => bad.prompt_digest.clear(),
        }
        assert!(matches!(a.prepare(bad), Err(ProviderError::InvalidSpec)));
    }
}
#[test]
fn wrong_terminal_turn_and_missing_session_fail_closed() {
    let s = spec(ProviderKind::ClaudeCode, 1);
    let mut r = adapter(&s, capabilities()).prepare(s).unwrap();
    r.enqueue(record(EventPayload::ProviderTerminal {
        turn: "unknown".into(),
        status: TerminalStatus::Success,
        raw_status: "success".into(),
    }));
    assert_eq!(r.poll(), Err(ProviderError::Correlation));
}
