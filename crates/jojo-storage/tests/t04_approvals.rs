mod common;
use common::*;
use jojo_domain::identity::{ArtifactId, FeatureId, OpaqueId, Revision};
use jojo_storage::*;
use jojo_test_support::COMPLETE_PRD_JSON;
const P: ArtifactId = ArtifactId(OpaqueId(101));
const A: ArtifactId = ArtifactId(OpaqueId(102));
const L: ArtifactId = ArtifactId(OpaqueId(103));
const RUN: OpaqueId = OpaqueId(104);
fn review(e: &Engine, id: ArtifactId) -> ReviewBundle {
    e.reader()
        .unwrap()
        .read(|v| v.review_bundle(id))
        .unwrap()
        .unwrap()
}
fn decide(
    e: &Engine,
    n: u128,
    b: ReviewBundle,
    a: DecisionAction,
) -> Result<Receipt, CommandError> {
    e.human_decide(
        CommandId(OpaqueId(n)),
        HumanOrigin::FakeHumanFixture {
            actor: "T04 human".into(),
        },
        b,
        a,
    )
}
fn doc(
    e: &Engine,
    n: u128,
    id: ArtifactId,
    expected: Option<Revision>,
    adr: Option<ArtifactId>,
) -> Result<Receipt, CommandError> {
    e.execute(
        request(e, n, "document", &format!("{id:?}{expected:?}{adr:?}")),
        move |tx| {
            tx.put_review_document(
                id,
                expected,
                P,
                adr,
                "All bound content, graph, source, policy, validation and evidence inputs.",
                "Displayed review bytes",
            )?;
            Ok("ok".into())
        },
    )
}
fn fixture() -> (TempDir, Engine) {
    let d = TempDir::new("t04");
    let e = start(&d.db());
    let p = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
    e.execute(request(&e, 1, "prd", p.content()), move |tx| {
        tx.create_artifact(
            P,
            FeatureId(OpaqueId(1)),
            &p,
            &Provenance::new(Author::Model, "draft, no authority"),
        )?;
        Ok("ok".into())
    })
    .unwrap();
    (d, e)
}
fn gates(e: &Engine) {
    decide(e, 2, review(e, P), DecisionAction::Approve).unwrap();
    doc(e, 3, A, None, None).unwrap();
    decide(e, 4, review(e, A), DecisionAction::Approve).unwrap();
    doc(e, 5, L, None, Some(A)).unwrap();
    decide(e, 6, review(e, L), DecisionAction::Approve).unwrap();
}
fn reserve(e: &Engine) {
    e.execute(request(e, 7, "reserve", ""), |tx| {
        tx.reserve_approved_attempt(RUN, P, A, L)?;
        Ok("ok".into())
    })
    .unwrap();
}
fn launch(e: &Engine, n: u128) -> Result<Receipt, CommandError> {
    e.execute(request(e, n, "start", ""), |tx| {
        tx.start_approved_attempt(RUN)?;
        Ok("ok".into())
    })
}
fn stop(e: &Engine) -> bool {
    e.reader()
        .unwrap()
        .read(|v| v.approval_stop_requested(RUN))
        .unwrap()
}
#[test]
fn stale_revision_canonical_digest_and_displayed_bundle_conflict() {
    let (_d, e) = fixture();
    let original = review(&e, P);
    for n in 0..4 {
        let mut b = original.clone();
        match n {
            0 => b.revision = Revision(9),
            1 => b.content_hash.push('a'),
            2 => b.displayed_hash.push('a'),
            _ => b.hash.push('a'),
        };
        assert!(decide(&e, 20 + n, b, DecisionAction::Approve).is_err());
    }
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.human_decisions(P))
            .unwrap()
            .is_empty()
    );
}
#[test]
fn duplicate_decisions_replay_once_and_new_duplicate_is_rejected() {
    let (_d, e) = fixture();
    let b = review(&e, P);
    let first = decide(&e, 2, b.clone(), DecisionAction::Approve).unwrap();
    let again = decide(&e, 2, b.clone(), DecisionAction::Approve).unwrap();
    assert!(again.replayed);
    assert_eq!(first.event_seq, again.event_seq);
    assert!(decide(&e, 3, review(&e, P), DecisionAction::Approve).is_err());
    assert!(decide(&e, 4, b, DecisionAction::Reject).is_err());
    assert_eq!(
        e.reader()
            .unwrap()
            .read(|v| v.human_decisions(P))
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn rejection_and_revocation_are_immutable_human_history() {
    let (_d, e) = fixture();
    decide(&e, 2, review(&e, P), DecisionAction::Reject).unwrap();
    assert!(doc(&e, 3, A, None, None).is_err());
    decide(&e, 4, review(&e, P), DecisionAction::Approve).unwrap();
    decide(&e, 5, review(&e, P), DecisionAction::Revoke).unwrap();
    let ds = e.reader().unwrap().read(|v| v.human_decisions(P)).unwrap();
    assert_eq!(
        ds.iter().map(|d| d.action.as_str()).collect::<Vec<_>>(),
        ["Reject", "Approve", "Revoke"]
    );
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.applicable_decision(P))
            .unwrap()
            .is_none()
    );
}
#[test]
fn all_three_gates_and_exact_parent_relations_required() {
    let (_d, e) = fixture();
    assert!(doc(&e, 3, A, None, None).is_err());
    assert!(
        e.execute(request(&e, 8, "reserve", ""), |tx| {
            tx.reserve_approved_attempt(RUN, P, A, L)?;
            Ok("".into())
        })
        .is_err()
    );
    gates(&e);
    assert!(
        e.execute(request(&e, 9, "reserve-wrong-parent", ""), |tx| {
            tx.reserve_approved_attempt(RUN, P, L, A)?;
            Ok("".into())
        })
        .is_err()
    );
    reserve(&e);
    assert!(launch(&e, 10).is_ok());
}
fn edit(e: &Engine) {
    let json = COMPLETE_PRD_JSON.replace(
        "Let one person keep a private list of articles to read later.",
        "Let a person retain articles for later reading.",
    );
    let p = PreparedPrd::import(json.as_bytes()).unwrap();
    e.execute(request(e, 20, "edit", p.content()), move |tx| {
        tx.revise_artifact(P, Revision(1), &p, &Provenance::new(Author::Human, "edit"))?;
        Ok("".into())
    })
    .unwrap();
}
#[test]
fn changed_prd_invalidates_adr_plan_and_readiness_transactionally() {
    let (_d, e) = fixture();
    gates(&e);
    reserve(&e);
    let historical = review(&e, L);
    edit(&e);
    assert!(stop(&e));
    for id in [P, A, L] {
        assert!(
            e.reader()
                .unwrap()
                .read(|v| v.applicable_decision(id))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            e.reader()
                .unwrap()
                .read(|v| v.human_decisions(id))
                .unwrap()
                .len(),
            1
        );
    }
    assert!(launch(&e, 21).is_err());
    assert!(decide(&e, 22, historical, DecisionAction::Approve).is_err());
    decide(&e, 23, review(&e, P), DecisionAction::Approve).unwrap();
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.applicable_decision(A))
            .unwrap()
            .is_none()
    );
}
#[test]
fn reserved_revocation_and_cancellation_prevent_start_even_after_reapproval() {
    for cancel in [false, true] {
        let (_d, e) = fixture();
        gates(&e);
        reserve(&e);
        if cancel {
            e.execute(request(&e, 10, "cancel", ""), |tx| {
                tx.cancel_approved_attempt(RUN)?;
                Ok("".into())
            })
            .unwrap();
        } else {
            decide(&e, 10, review(&e, L), DecisionAction::Revoke).unwrap();
            decide(&e, 11, review(&e, L), DecisionAction::Approve).unwrap();
        }
        assert!(stop(&e));
        assert!(launch(&e, 12).is_err());
    }
}
#[test]
fn post_launch_revocation_persists_stop_and_excludes_sealed_result_without_mutation() {
    let (d, e) = fixture();
    gates(&e);
    reserve(&e);
    launch(&e, 8).unwrap();
    e.execute(request(&e, 80, "running", ""), |tx| {
        tx.mark_approval_attempt_running(RUN)?;
        Ok("".into())
    })
    .unwrap();
    e.execute(request(&e, 9, "seal", ""), |tx| {
        tx.seal_approval_result(RUN, "sealed evidence bytes")?;
        Ok("".into())
    })
    .unwrap();
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.approval_result_applicable(RUN))
            .unwrap()
    );
    decide(&e, 10, review(&e, P), DecisionAction::Revoke).unwrap();
    assert!(stop(&e));
    assert!(
        !e.reader()
            .unwrap()
            .read(|v| v.approval_result_applicable(RUN))
            .unwrap()
    );
    assert!(
        e.execute(request(&e, 11, "overwrite", ""), |tx| {
            tx.seal_approval_result(RUN, "changed")?;
            Ok("".into())
        })
        .is_err()
    );
    drop(e);
    let e = start(&d.db());
    assert!(stop(&e));
    assert_eq!(
        e.reader()
            .unwrap()
            .read(|v| v.approval_result(RUN))
            .unwrap()
            .as_deref(),
        Some("sealed evidence bytes")
    );
}
#[test]
fn active_run_stop_is_durable_before_result_arrives() {
    let (d, e) = fixture();
    gates(&e);
    reserve(&e);
    launch(&e, 8).unwrap();
    e.execute(request(&e, 80, "running", ""), |tx| {
        tx.mark_approval_attempt_running(RUN)?;
        Ok("".into())
    })
    .unwrap();
    decide(&e, 9, review(&e, A), DecisionAction::Revoke).unwrap();
    assert!(stop(&e));
    drop(e);
    let e = start(&d.db());
    assert!(stop(&e));
    e.execute(request(&e, 10, "late-result", ""), |tx| {
        tx.seal_approval_result(RUN, "late success")?;
        Ok("".into())
    })
    .unwrap();
    assert!(
        !e.reader()
            .unwrap()
            .read(|v| v.approval_result_applicable(RUN))
            .unwrap()
    );
}
#[test]
fn fake_provider_events_and_markdown_checkboxes_never_authorize() {
    let (_d, e) = fixture();
    e.execute(request(&e, 10, "ProviderEvent", "approved"), |tx| {
        tx.append_event(
            "HumanDecision",
            None,
            &jojo_domain::payload::Record::new()
                .text("note", "[x] approved")
                .text("action", "Approve")
                .text("origin", "TrustedUi"),
        )?;
        Ok("approved".into())
    })
    .unwrap();
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.applicable_decision(P))
            .unwrap()
            .is_none()
    );
    assert!(doc(&e, 11, A, None, None).is_err());
}
#[test]
fn invalidation_rolls_back_with_failed_command() {
    let (_d, e) = fixture();
    gates(&e);
    reserve(&e);
    assert!(
        e.execute(request(&e, 10, "rollback", ""), |tx| {
            tx.cancel_approved_attempt(RUN)?;
            Err(TxError::Rejected("injected failure".into()))
        })
        .is_err()
    );
    assert!(!stop(&e));
    assert!(launch(&e, 11).is_ok());
}
#[test]
fn changed_adr_and_display_only_plan_edit_require_new_decisions() {
    let (_d, e) = fixture();
    gates(&e);
    reserve(&e);
    e.execute(request(&e, 10, "plan-display-edit", ""), |tx| {
        tx.put_review_document(
            L,
            Some(Revision(1)),
            P,
            Some(A),
            "All bound content, graph, source, policy, validation and evidence inputs.",
            "Displayed review bytes ",
        )?;
        Ok("".into())
    })
    .unwrap();
    assert!(stop(&e));
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.applicable_decision(L))
            .unwrap()
            .is_none()
    );
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.applicable_decision(A))
            .unwrap()
            .is_some()
    );
    doc(&e, 11, A, Some(Revision(1)), None).unwrap();
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.applicable_decision(A))
            .unwrap()
            .is_none()
    );
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.applicable_decision(L))
            .unwrap()
            .is_none()
    );
}

#[test]
fn racing_human_commands_on_one_displayed_review_have_one_winner() {
    let (_d, e) = fixture();
    let b = review(&e, P);
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let handles: Vec<_> = [DecisionAction::Approve, DecisionAction::Reject]
            .into_iter()
            .enumerate()
            .map(|(n, action)| {
                let (e, b, barrier) = (&e, b.clone(), &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    decide(e, 50 + n as u128, b, action)
                })
            })
            .collect();
        let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    });
    assert_eq!(
        e.reader()
            .unwrap()
            .read(|v| v.human_decisions(P))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn failed_prd_edit_rolls_back_versions_events_and_stop_intent() {
    let (_d, e) = fixture();
    gates(&e);
    reserve(&e);
    let p = PreparedPrd::import(
        COMPLETE_PRD_JSON
            .replace(
                "Let one person keep a private list of articles to read later.",
                "Keep articles for reading.",
            )
            .as_bytes(),
    )
    .unwrap();
    let before = e.reader().unwrap().snapshot().unwrap();
    assert!(
        e.execute(request(&e, 30, "failed-edit", p.content()), move |tx| {
            tx.revise_artifact(P, Revision(1), &p, &Provenance::new(Author::Human, "edit"))?;
            Err(TxError::Rejected("fail after invalidation".into()))
        })
        .is_err()
    );
    assert_eq!(e.reader().unwrap().snapshot().unwrap(), before);
    assert!(!stop(&e));
    assert!(launch(&e, 31).is_ok());
}

#[test]
fn a_new_blocking_question_invalidates_approval_and_requests_stop() {
    let (_d, e) = fixture();
    gates(&e);
    reserve(&e);
    launch(&e, 8).unwrap();
    e.execute(request(&e, 80, "running", ""), |tx| {
        tx.mark_approval_attempt_running(RUN)?;
        Ok("".into())
    })
    .unwrap();
    e.execute(request(&e, 30, "question", ""), |tx| {
        tx.ask_question(
            jojo_domain::identity::QuestionId(OpaqueId(999)),
            FeatureId(OpaqueId(1)),
            Some(P),
            "Which scope is intended?",
            QuestionDisposition::Blocking,
            &Provenance::new(Author::Human, "review"),
        )?;
        Ok("".into())
    })
    .unwrap();
    assert!(stop(&e));
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.applicable_decision(L))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_changed_human_answer_invalidates_the_review_bundle_without_a_prd_edit() {
    let (_d, e) = fixture();
    let q = jojo_domain::identity::QuestionId(OpaqueId(999));
    e.execute(request(&e, 90, "deferred-question", ""), move |tx| {
        tx.ask_question(
            q,
            FeatureId(OpaqueId(1)),
            Some(P),
            "Which label?",
            QuestionDisposition::Deferred,
            &Provenance::new(Author::Human, "review"),
        )?;
        Ok("".into())
    })
    .unwrap();
    gates(&e);
    reserve(&e);
    let before = review(&e, P);
    e.execute(request(&e, 91, "answer", ""), move |tx| {
        tx.answer_question(
            q,
            Revision(1),
            "Use the updated label.",
            &Provenance::new(Author::Human, "review"),
        )?;
        Ok("".into())
    })
    .unwrap();
    let after = review(&e, P);
    assert_eq!(before.content_hash, after.content_hash);
    assert_ne!(before.hash, after.hash);
    assert!(stop(&e));
    assert!(decide(&e, 92, before, DecisionAction::Approve).is_err());
}

#[test]
fn rejection_is_available_for_incomplete_content_and_revocation_for_stale_upstream() {
    let (_d, e) = fixture();
    gates(&e);
    edit(&e);
    let stale = review(&e, L);
    assert!(decide(&e, 95, stale.clone(), DecisionAction::Approve).is_err());
    decide(&e, 96, stale, DecisionAction::Revoke).unwrap();
    let p=PreparedPrd::import(br#"{"schema_version":1,"title":"T","goal":"G","non_goals":[],"actors":[],"use_cases":[],"acceptance_criteria":[],"nfrs":[],"assumptions":[],"blockers":[]}"#).unwrap();
    let incomplete = ArtifactId(OpaqueId(900));
    e.execute(request(&e, 97, "incomplete", ""), move |tx| {
        tx.create_artifact(
            incomplete,
            FeatureId(OpaqueId(2)),
            &p,
            &Provenance::new(Author::Model, "draft"),
        )?;
        Ok("".into())
    })
    .unwrap();
    let b = review(&e, incomplete);
    assert!(decide(&e, 98, b.clone(), DecisionAction::Approve).is_err());
    decide(&e, 99, b, DecisionAction::Reject).unwrap();
}
