use jojo_domain::identity::OpaqueId;
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
fn e() -> Envelope {
    Envelope {
        major: 1,
        minor: 0,
        session: OpaqueId(1),
        epoch: 2,
        request: OpaqueId(3),
        revision: None,
        kind: "Response".into(),
        payload: object([]),
    }
}
#[test]
fn fragmented_fake_helper_frames_and_unicode_roundtrip() {
    let mut e = e();
    e.payload = object([("text", text("hello\n\"🌲"))]);
    let bytes = encode(&e.json());
    let mut decoder = Decoder::default();
    for b in bytes.bytes() {
        assert!(decoder.push(b).unwrap().is_none());
    }
    assert_eq!(decoder.push(b'\n').unwrap().unwrap().payload, e.payload);
    decoder.finish().unwrap();
}
#[test]
fn malformed_duplicate_oversized_and_partial_fake_responses_fail_closed() {
    assert!(Envelope::parse(b"{}").is_err());
    let mut d = Decoder::default();
    for _ in 0..MAX_FRAME {
        d.push(b' ').unwrap();
    }
    assert!(d.push(b' ').is_err());
    assert!(d.finish().is_err());
    let s = encode(&e().json()).replace("\"major\":1", "\"major\":1,\"major\":1");
    assert!(Envelope::parse(s.as_bytes()).is_err());
    let s = encode(&e().json()).replace("\"epoch\":2", "\"epoch\":-1");
    assert!(Envelope::parse(s.as_bytes()).is_err());
}
#[test]
fn event_projection_duplicates_out_of_order_gaps_and_epoch_changes() {
    let mut c = Cursor::default();
    c.snapshot(2, 10);
    assert_eq!(c.event(2, 11), Projection::Apply);
    assert_eq!(c.event(2, 11), Projection::Duplicate);
    assert_eq!(c.event(2, 9), Projection::Duplicate);
    assert_eq!(c.event(2, 13), Projection::Reload);
    assert_eq!(c.event(2, 12), Projection::Reload);
    c.snapshot(2, 13);
    assert_eq!(c.event(3, 14), Projection::Reload);
}
#[test]
fn helper_crash_and_exit_zero_without_ready_remain_unknown() {
    let mut c = ClientClose::new(OpaqueId(1), 2, OpaqueId(4));
    c.exited(0);
    assert_eq!(c.state(), Closure::Unknown);
    let mut c = ClientClose::new(OpaqueId(1), 2, OpaqueId(4));
    c.exited(9);
    assert_eq!(c.state(), Closure::Failed);
}
#[test]
fn ready_needs_matching_proof_and_observed_helper_exit() {
    let mut c = ClientClose::new(OpaqueId(1), 2, OpaqueId(4));
    let mut event = e();
    event.kind = "ShutdownReady".into();
    event.payload = object([
        ("ticket", identity(OpaqueId(4))),
        ("shutdown", identity(OpaqueId(5))),
        ("event_seq", number(8)),
        ("outcome", text("Ready")),
        ("process_stop_confirmed", Json::Bool(true)),
        ("persistence_sealed", Json::Bool(true)),
        ("failures", Json::List(vec![])),
    ]);
    c.response(&event).unwrap();
    assert_eq!(c.state(), Closure::ReadyAwaitingExit);
    c.exited(0);
    assert_eq!(c.state(), Closure::Clean);
    event.epoch = 3;
    assert!(
        ClientClose::new(OpaqueId(1), 2, OpaqueId(4))
            .response(&event)
            .is_err()
    );
    event.epoch = 2;
    event.payload = object([]);
    assert!(
        ClientClose::new(OpaqueId(1), 2, OpaqueId(4))
            .response(&event)
            .is_err()
    );
}
