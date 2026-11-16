//! T13 uses the helper's production service and worker registry with real fixture trees.
mod common;
use common::attempts::*;
use jojo_domain::{identity::*, ipc::*, json::Json, provider::*, shutdown::*};
use jojo_helper::service::*;
use jojo_storage::*;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
fn envelope(s: &Service, n: u128, kind: &str, payload: Json) -> Envelope {
    Envelope {
        major: 1,
        minor: 0,
        session: OpaqueId(5),
        epoch: s.engine.epoch().0,
        request: OpaqueId(100000 + n),
        revision: None,
        kind: kind.into(),
        payload,
    }
}
fn command(s: &mut Service, n: u128, kind: &str, payload: Json) -> Envelope {
    let e = envelope(s, n, kind, payload);
    s.request(&e)
}
fn hello(s: &mut Service) {
    let e = envelope(s, 1, "Hello", object([]));
    assert_eq!(s.request(&e).kind, "Response");
}
fn service(f: &mut F) -> Service {
    Service::new(Arc::new(f.engine.take().unwrap())).unwrap()
}
fn launch(s: &mut Service, spec: LaunchSpec, launch: ProcessLaunch, hooks: LaunchHooks) {
    s.launch(
        spec,
        launch,
        StopPlan::new(
            &caps(Support::Supported, Support::Supported),
            StopBudgets {
                interrupt: Duration::from_millis(100),
                terminate: Duration::from_millis(100),
                kill: Duration::from_secs(5),
                drain: Duration::from_millis(100),
            },
        ),
        hooks,
    )
    .unwrap();
}
fn finish(s: &mut Service, disconnected: bool) -> ShutdownReceipt {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = s.finish(disconnected).unwrap() {
            return r;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
fn read<T>(s: &Service, f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>) -> T {
    s.engine.reader().unwrap().read(f).unwrap()
}
#[test]
fn t13_worker_pool_cancel_then_confirm_joins_parallel_trees_and_settles() {
    let mut f = F::with_providers(&[ProviderKind::ClaudeCode, ProviderKind::Codex]);
    let specs = [
        f.spec(101, 1, "wait-release"),
        f.spec(102, 2, "codex-wait-release"),
    ];
    let launches = specs.each_ref().map(|spec| f.launch(spec));
    let mut s = service(&mut f);
    hello(&mut s);
    let (tx, rx) = mpsc::channel();
    for (spec, l) in specs.into_iter().zip(launches) {
        let tx = tx.clone();
        launch(
            &mut s,
            spec,
            l,
            LaunchHooks {
                inspector: Some(ps()),
                on_event: Some(Box::new(move |e| {
                    if matches!(e, EventPayload::SessionIdentified(_)) {
                        tx.send(()).unwrap();
                    }
                })),
                ..Default::default()
            },
        );
    }
    for _ in 0..2 {
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
    }
    assert_eq!(
        command(&mut s, 2, "PrepareClose", object([])).kind,
        "Response"
    );
    let ticket = s.status().unwrap().ticket.unwrap().ticket;
    assert_eq!(
        command(
            &mut s,
            3,
            "CancelClose",
            object([("ticket", identity(ticket))])
        )
        .kind,
        "Response"
    );
    assert_eq!(s.status().unwrap().state, CloseState::Open);
    for a in [101, 102] {
        assert!(!read(&s, |v| v.approval_stop_requested(OpaqueId(a))));
    }
    command(&mut s, 4, "PrepareClose", object([]));
    let ticket = s.status().unwrap().ticket.unwrap().ticket;
    command(
        &mut s,
        5,
        "ConfirmClose",
        object([("ticket", identity(ticket))]),
    );
    let r = finish(&mut s, false);
    assert_eq!(r.outcome, ShutdownOutcome::Ready);
    assert!(r.process_stop_confirmed && r.persistence_sealed);
    for a in [101, 102] {
        let result = read(&s, |v| v.attempt_result(OpaqueId(a))).unwrap();
        assert_eq!(
            result.outcome,
            jojo_domain::attempt::AttemptOutcome::Interrupted
        );
    }
}
#[test]
fn t13_disconnect_stops_worker_and_restart_preserves_unclean_recovery() {
    let mut f = F::new(1);
    let spec = f.spec(101, 1, "ignore-signals");
    let l = f.launch(&spec);
    let mut s = service(&mut f);
    let (tx, rx) = mpsc::channel();
    launch(
        &mut s,
        spec,
        l,
        LaunchHooks {
            inspector: Some(ps()),
            on_event: Some(Box::new(move |e| {
                if matches!(e, EventPayload::SessionIdentified(_)) {
                    tx.send(()).unwrap();
                }
            })),
            ..Default::default()
        },
    );
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    s.emergency().unwrap();
    let r = finish(&mut s, true);
    assert_eq!(r.outcome, ShutdownOutcome::Emergency);
    assert!(r.process_stop_confirmed);
    drop(s);
    f.restart();
    assert_ne!(
        f.e().startup_report().epoch.previous.unwrap().1,
        ShutdownMarker::Clean
    );
    assert_eq!(f.admission(), Admission::Closed);
    assert!(f.result(101).is_some());
}
#[test]
fn t13_receipt_after_dropped_response_duplicate_conflict_and_epoch_fences() {
    let mut f = F::new(1);
    let mut s = service(&mut f);
    hello(&mut s);
    let e = envelope(&s, 10, "PrepareClose", object([]));
    let first = s.request(&e);
    assert_eq!(first.kind, "Response");
    let replay = s.request(&e);
    assert_eq!(get(&replay.payload, "replayed").unwrap(), &Json::Bool(true));
    let q = command(
        &mut s,
        11,
        "Receipt",
        object([("command", identity(OpaqueId(100010)))]),
    );
    assert_eq!(
        get(&q.payload, "event_seq").unwrap(),
        get(&first.payload, "event_seq").unwrap()
    );
    let mut conflict = e.clone();
    conflict.payload = object([("different", Json::Bool(true))]);
    assert_eq!(s.request(&conflict).kind, "Error");
    conflict.epoch += 1;
    assert_eq!(s.request(&conflict).kind, "Error");
    let mut mismatch = envelope(&s, 12, "Hello", object([]));
    mismatch.major = 2;
    s.request(&mismatch);
    assert_eq!(s.request(&e).kind, "Error");
    assert_eq!(
        command(&mut s, 13, "CloseStatus", object([])).kind,
        "Response"
    );
    s.emergency().unwrap();
    let emergency = finish(&mut s, true);
    drop(s);
    f.restart();
    let mut reopened = service(&mut f);
    hello(&mut reopened);
    let q = command(
        &mut reopened,
        14,
        "Receipt",
        object([("command", identity(OpaqueId(100010)))]),
    );
    assert_eq!(
        get(&q.payload, "event_seq").unwrap(),
        get(&first.payload, "event_seq").unwrap()
    );
    let q = command(
        &mut reopened,
        15,
        "ShutdownReceipt",
        object([("ticket", identity(emergency.ticket))]),
    );
    assert_eq!(get(&q.payload, "outcome").unwrap(), &text("Emergency"));
    reopened.emergency().unwrap();
    finish(&mut reopened, true);
}
#[test]
fn t13_snapshot_boundary_and_bounded_queries_keep_committed_events() {
    let mut f = F::new(1);
    let mut s = service(&mut f);
    hello(&mut s);
    let page = command(
        &mut s,
        20,
        "Snapshot",
        object([
            ("section", text("aggregates")),
            ("after", text("")),
            ("cursor", Json::Null),
            ("limit", number(8)),
        ]),
    );
    assert_eq!(page.kind, "Response");
    let cursor = uint(get(&page.payload, "as_of_event_seq").unwrap()).unwrap();
    command(&mut s, 21, "PrepareClose", object([]));
    let events = command(
        &mut s,
        22,
        "Events",
        object([("after", number(cursor)), ("limit", number(8))]),
    );
    assert_eq!(events.kind, "Response");
    if let Json::List(rows) = events.payload {
        assert!(!rows.is_empty());
        assert_eq!(uint(get(&rows[0], "seq").unwrap()).unwrap(), cursor + 1);
    } else {
        panic!()
    };
    assert_eq!(
        command(
            &mut s,
            23,
            "Snapshot",
            object([
                ("section", text("claims")),
                ("after", text("")),
                ("cursor", number(cursor)),
                ("limit", number(8))
            ])
        )
        .kind,
        "Error"
    );
    assert_eq!(
        command(
            &mut s,
            24,
            "Events",
            object([("after", number(0)), ("limit", number(999999))])
        )
        .kind,
        "Error"
    );
    s.emergency().unwrap();
    finish(&mut s, true);
}
#[test]
fn t13_stalled_worker_never_ready_until_runner_returns() {
    let mut f = F::new(1);
    let spec = f.spec(101, 1, "wait-release");
    let l = f.launch(&spec);
    let mut s = service(&mut f);
    hello(&mut s);
    let (tx, rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    launch(
        &mut s,
        spec,
        l,
        LaunchHooks {
            inspector: Some(ps()),
            after_issue: Some(Box::new(move || {
                tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })),
            ..Default::default()
        },
    );
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    command(&mut s, 30, "PrepareClose", object([]));
    let ticket = s.status().unwrap().ticket.unwrap().ticket;
    command(
        &mut s,
        31,
        "ConfirmClose",
        object([("ticket", identity(ticket))]),
    );
    assert!(s.finish(false).unwrap().is_none());
    assert_eq!(read(&s, |v| v.session()).shutdown, ShutdownMarker::Running);
    release_tx.send(()).unwrap();
    assert_eq!(finish(&mut s, false).outcome, ShutdownOutcome::Ready);
}
#[test]
fn t13_child_does_not_inherit_helper_control_fds() {
    let mut f = F::new(1);
    let spec = f.spec(101, 1, "fd-report");
    let l = f.launch(&spec);
    let mut s = service(&mut f);
    launch(
        &mut s,
        spec,
        l,
        LaunchHooks {
            inspector: Some(ps()),
            ..Default::default()
        },
    );
    until(|| s.workers.reap());
    let events = read(&s, |v| v.provider_events(OpaqueId(101)));
    assert!(
        events.iter().any(|e| e.payload.contains("fds=[]")),
        "{events:?}"
    );
    let p = read(&s, |v| v.ipc_log(OpaqueId(101), 1, 0));
    assert!(encode(&p).len() < MAX_FRAME);
}
#[test]
fn t13_failed_writer_emergency_still_stops_through_runner_and_preserves_claims() {
    let mut f = F::new(1);
    let spec = f.spec(101, 1, "wait-release");
    let l = f.launch(&spec);
    let mut s = service(&mut f);
    let (tx, rx) = mpsc::channel();
    let (root_tx, root_rx) = mpsc::channel();
    launch(
        &mut s,
        spec,
        l,
        LaunchHooks {
            inspector: Some(ps()),
            after_spawn: Some(Box::new(move |p| {
                root_tx.send(p.clone()).unwrap();
            })),
            on_event: Some(Box::new(move |e| {
                if matches!(e, EventPayload::SessionIdentified(_)) {
                    tx.send(()).unwrap();
                }
            })),
            ..Default::default()
        },
    );
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    let records = read(&s, |v| v.process_records(OpaqueId(101)));
    assert!(!records.is_empty());
    let result = s.engine.execute(
        CommandRequest::new(
            CommandId(OpaqueId(999999)),
            s.engine.epoch(),
            "test-writer-failure",
            "",
        ),
        |_| panic!("injected writer loss"),
    );
    assert!(result.is_err());
    assert!(s.emergency().is_err());
    until(|| s.workers.reap());
    assert!(s.workers.failed());
    assert_eq!(read(&s, |v| v.session()).shutdown, ShutdownMarker::Running);
    assert!(!read(&s, |v| v.live_claims()).is_empty());
    let identities = vec![root_rx.recv_timeout(Duration::from_secs(20)).unwrap()];
    assert!(
        inspect_recorded(&ps(), &identities)
            .iter()
            .all(|(_, c)| *c == IdentityCheck::Absent)
    );
}
#[test]
fn t13_unconfirmed_escaped_holder_has_failure_receipt_and_claims() {
    let mut f = F::new(1);
    let barriers = f.barriers();
    let go = barriers.join("go");
    let spec = f.spec(101, 1, "escaped-holder");
    let l = f.launch(&spec);
    let mut s = service(&mut f);
    hello(&mut s);
    let (tx, rx) = mpsc::channel();
    launch(
        &mut s,
        spec,
        l,
        LaunchHooks {
            inspector: Some(ps()),
            on_event: Some(Box::new(move |e| match e {
                EventPayload::SessionIdentified(_) => std::fs::write(&go, b"").unwrap(),
                EventPayload::ProviderTerminal { .. } => {
                    tx.send(()).unwrap();
                }
                _ => {}
            })),
            ..Default::default()
        },
    );
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    command(&mut s, 50, "PrepareClose", object([]));
    let t = s.status().unwrap().ticket.unwrap().ticket;
    command(
        &mut s,
        51,
        "ConfirmClose",
        object([("ticket", identity(t))]),
    );
    let check = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let r = finish(&mut s, false);
        assert_eq!(r.outcome, ShutdownOutcome::Failed);
        assert!(!r.process_stop_confirmed);
        assert!(!read(&s, |v| v.live_claims()).is_empty());
        assert!(read(&s, |v| v.attempt_result(OpaqueId(101))).is_none());
    }));
    std::fs::write(barriers.join("release-linger"), b"").unwrap();
    until(|| barriers.join("linger-released").exists());
    check.unwrap();
}
#[test]
fn t13_worker_output_flood_does_not_starve_control_or_forge_ready() {
    let mut f = F::new(1);
    let spec = f.spec(101, 1, "stderr-flood");
    let l = f.launch(&spec);
    let mut s = service(&mut f);
    hello(&mut s);
    let (tx, rx) = mpsc::channel();
    launch(
        &mut s,
        spec,
        l,
        LaunchHooks {
            inspector: Some(ps()),
            on_event: Some(Box::new(move |e| {
                if matches!(e, EventPayload::SessionIdentified(_)) {
                    tx.send(()).unwrap();
                }
            })),
            ..Default::default()
        },
    );
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
    let before = Instant::now();
    assert_eq!(
        command(&mut s, 60, "PrepareClose", object([])).kind,
        "Response"
    );
    assert!(before.elapsed() < Duration::from_secs(5));
    let ticket = s.status().unwrap().ticket.unwrap().ticket;
    command(
        &mut s,
        61,
        "ConfirmClose",
        object([("ticket", identity(ticket))]),
    );
    let r = finish(&mut s, false);
    assert!(
        r.outcome != ShutdownOutcome::Ready || r.process_stop_confirmed && r.persistence_sealed
    );
}
#[test]
fn t13_review_decision_binds_exact_display_and_replays_original_receipt() {
    let mut f = F::new(1);
    let mut s = service(&mut f);
    hello(&mut s);
    let q = command(
        &mut s,
        80,
        "ReviewBundle",
        object([("subject", identity(OpaqueId(903)))]),
    );
    assert_eq!(q.kind, "Response");
    let revision = uint(get(&q.payload, "revision").unwrap()).unwrap();
    let Json::Object(mut fields) = q.payload else {
        panic!()
    };
    fields.retain(|(k, _)| k != "revision");
    fields.push(("action".into(), text("Revoke")));
    let payload = Json::Object(fields);
    let mut e = envelope(&s, 81, "HumanDecision", payload.clone());
    e.revision = Some(revision);
    let mut stale = e.clone();
    stale.request = OpaqueId(100082);
    if let Json::Object(fields) = &mut stale.payload {
        fields
            .iter_mut()
            .find(|(k, _)| k == "displayed_hash")
            .unwrap()
            .1 = text("wrong displayed bytes");
    }
    assert_eq!(s.request(&stale).kind, "Error");
    assert_eq!(s.request(&e).kind, "Response");
    assert_eq!(
        get(&s.request(&e).payload, "replayed").unwrap(),
        &Json::Bool(true)
    );
    s.emergency().unwrap();
    finish(&mut s, true);
}
