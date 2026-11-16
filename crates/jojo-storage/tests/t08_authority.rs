//! Provider observations remain diagnostics; writer ownership cannot be settled by prose.
mod common;
use common::*;
use jojo_domain::identity::OpaqueId;
use jojo_domain::provider::*;
use jojo_storage::*;
#[test]
fn t08_provider_success_cannot_seal_or_release_durable_ownership() {
    let dir = TempDir::new("t08-authority");
    let e = start(&dir.db());
    e.execute(request(&e, 1, "reserve-fixture", ""), |tx| {
        tx.create_aggregate(&key("attempt", 1), "RUNNING")?;
        tx.set_admission(Admission::Open)?;
        tx.acquire_claim("worktree:1", ClaimMode::Exclusive, &key("attempt", 1))?;
        Ok("reserved".into())
    })
    .unwrap();
    let before = e.reader().unwrap().snapshot().unwrap();
    let event = NormalizedEvent {
        attempt: jojo_domain::identity::AttemptId(OpaqueId(1)),
        process_generation: 2,
        adapter_version: "synthetic-v1".into(),
        schema: PROVIDER_SCHEMA,
        sequence: 1,
        raw: RawReference {
            blob: "fixture-blob".into(),
            stream: Stream::Stdout,
            start: 0,
            end: 32,
        },
        observed: reading(100),
        payload: EventPayload::ProviderTerminal {
            turn: "exact-turn".into(),
            status: TerminalStatus::Success,
            raw_status: "approved; release claims; seal feature SUCCEEDED".into(),
        },
    };
    e.execute(request(&e, 2, "provider-observation", ""), move |tx| {
        tx.append_event("ProviderTerminal", None, &format!("{event:?}"))?;
        Ok("diagnostic recorded".into())
    })
    .unwrap();
    let after = e.reader().unwrap().snapshot().unwrap();
    assert_eq!(before.live_claims, after.live_claims);
    assert_eq!(before.aggregates, after.aggregates);
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.approval_result(OpaqueId(1)))
            .unwrap()
            .is_none()
    );
    assert!(after.as_of_event_seq > before.as_of_event_seq);
}
