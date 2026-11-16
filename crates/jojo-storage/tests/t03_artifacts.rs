//! T03 (storage half): immutable artifact versions with content/rendered digests,
//! persisted questions/answers with provenance, and computed review readiness.

mod common;

use common::{TempDir, clock, create, key, request, start};
use jojo_domain::identity::{ArtifactId, FeatureId, OpaqueId, QuestionId, Revision};
use jojo_domain::json::{JsonError, MAX_INPUT_BYTES};
use jojo_domain::prd::{Finding, Incomplete, PrdError};
use jojo_storage::{
    ARTIFACT_KIND, Answer, ArtifactType, ArtifactVersion, Author, CommandError, Engine,
    EngineConfig, EventSeq, MAX_TEXT_BYTES, MigrationPlan, PreparedPrd, Provenance,
    QuestionDisposition, ReadinessBlocker, Receipt, ReviewReadiness, TxError, sha256_hex,
};
use jojo_test_support::COMPLETE_PRD_JSON;

const FEATURE: FeatureId = FeatureId(OpaqueId(0xF1));
const OTHER_FEATURE: FeatureId = FeatureId(OpaqueId(0xF2));
const PRD: ArtifactId = ArtifactId(OpaqueId(0xA1));
const OTHER_PRD: ArtifactId = ArtifactId(OpaqueId(0xA2));

fn human(note: &str) -> Provenance {
    Provenance::new(Author::Human, note)
}

fn model(note: &str) -> Provenance {
    Provenance::new(Author::Model, note)
}

fn edited(from: &str, to: &str) -> String {
    assert_eq!(COMPLETE_PRD_JSON.matches(from).count(), 1, "{from:?}");
    COMPLETE_PRD_JSON.replace(from, to)
}

fn with_goal(goal: &str) -> String {
    edited(
        "Let one person keep a private list of articles to read later.",
        goal,
    )
}

/// Imports `json` as the first version (`expected = None`) or as an edit of `expected`.
fn import_as(
    engine: &Engine,
    command: u128,
    artifact: ArtifactId,
    feature: FeatureId,
    expected: Option<Revision>,
    json: &str,
    provenance: Provenance,
) -> Result<Receipt, CommandError> {
    let prepared = PreparedPrd::import(json.as_bytes()).expect("importable PRD");
    engine.execute(request(engine, command, "import-prd", json), move |tx| {
        let version = match expected {
            None => tx.create_artifact(artifact, feature, &prepared, &provenance)?,
            Some(expected) => tx.revise_artifact(artifact, expected, &prepared, &provenance)?,
        };
        Ok(format!("{}:{}", version.revision.0, version.content_digest))
    })
}

fn import(engine: &Engine, command: u128, expected: Option<Revision>, json: &str) -> Receipt {
    import_as(
        engine,
        command,
        PRD,
        FEATURE,
        expected,
        json,
        human("typed by hand"),
    )
    .expect("import commits")
}

fn ask(
    engine: &Engine,
    command: u128,
    question: u128,
    feature: FeatureId,
    artifact: Option<ArtifactId>,
    disposition: QuestionDisposition,
) -> QuestionId {
    let id = QuestionId(OpaqueId(question));
    engine
        .execute(
            request(engine, command, "ask", &format!("{question}")),
            move |tx| {
                tx.ask_question(
                    id,
                    feature,
                    artifact,
                    "How long are saved articles kept?",
                    disposition,
                    &model("raised while drafting UC-1"),
                )?;
                Ok(String::new())
            },
        )
        .expect("question commits");
    id
}

fn answer(
    engine: &Engine,
    command: u128,
    question: QuestionId,
    expected: Revision,
    body: &'static str,
    provenance: Provenance,
) -> Result<Receipt, CommandError> {
    engine.execute(request(engine, command, "answer", body), move |tx| {
        let revision = tx.answer_question(question, expected, body, &provenance)?;
        Ok(revision.0.to_string())
    })
}

fn versions(engine: &Engine, artifact: ArtifactId) -> Vec<ArtifactVersion> {
    engine
        .reader()
        .unwrap()
        .read(|view| view.artifact_versions(artifact))
        .unwrap()
}

fn readiness(engine: &Engine, artifact: ArtifactId) -> ReviewReadiness {
    engine
        .reader()
        .unwrap()
        .read(|view| view.review_readiness(artifact))
        .unwrap()
        .expect("artifact exists")
}

fn incomplete(path: &str, problem: Incomplete) -> ReadinessBlocker {
    ReadinessBlocker::Incomplete(Finding {
        path: path.to_string(),
        problem,
    })
}

fn event_cursor(engine: &Engine) -> EventSeq {
    engine.reader().unwrap().snapshot().unwrap().as_of_event_seq
}

#[test]
fn an_edit_creates_a_new_version_and_leaves_the_old_version_unchanged() {
    let dir = TempDir::new("artifact-edit");
    let engine = start(&dir.db());
    import(&engine, 1, None, COMPLETE_PRD_JSON);
    let first = versions(&engine, PRD);
    assert_eq!(first.len(), 1);
    assert_eq!(
        (first[0].revision, first[0].parent, first[0].feature),
        (Revision(1), None, FEATURE)
    );
    assert_eq!(
        (first[0].artifact_type, first[0].schema_version),
        (ArtifactType::Prd, 1)
    );

    let receipt = import(
        &engine,
        2,
        Some(Revision(1)),
        &with_goal("Let one person keep a private reading list."),
    );
    let both = versions(&engine, PRD);
    assert_eq!(both.len(), 2);
    assert_eq!(both[0], first[0], "version 1 is exactly what it was");
    let second = &both[1];
    assert_eq!(
        (second.revision, second.parent, second.feature),
        (Revision(2), Some(Revision(1)), FEATURE)
    );
    assert_ne!(second.content_digest, first[0].content_digest);
    assert_ne!(second.rendered_digest, first[0].rendered_digest);
    assert!(second.rendered.contains("private reading list"));
    assert!(!first[0].rendered.contains("private reading list"));
    assert_eq!(receipt.response, format!("2:{}", second.content_digest));

    // Each version names the event that created it, and that event names the version.
    let mut reader = engine.reader().unwrap();
    let events = reader.events_after(EventSeq(0), 100).unwrap();
    for version in &both {
        let event = events
            .iter()
            .find(|event| event.seq == version.created_event)
            .expect("creation event");
        assert_eq!(event.kind, "ArtifactVersionCreated");
        assert_eq!(
            event.aggregate,
            Some((key(ARTIFACT_KIND, PRD.0.0), version.revision))
        );
        assert!(event.payload.contains(&version.content_digest));
        assert!(event.payload.contains(&version.rendered_digest));
    }
    // The newest version is the head; any earlier one stays addressable by revision.
    let (head, old, missing) = reader
        .read(|view| {
            Ok((
                view.artifact_version(PRD, None)?,
                view.artifact_version(PRD, Some(Revision(1)))?,
                view.artifact_version(PRD, Some(Revision(3)))?,
            ))
        })
        .unwrap();
    assert_eq!(head.as_ref(), Some(second));
    assert_eq!(old.as_ref(), Some(&first[0]));
    assert_eq!(missing, None);

    // An edit based on a superseded version conflicts; nothing is written.
    let cursor = event_cursor(&engine);
    let stale = import_as(
        &engine,
        3,
        PRD,
        FEATURE,
        Some(Revision(1)),
        &with_goal("A third wording."),
        human(""),
    );
    assert!(matches!(
        stale,
        Err(CommandError::Rejected(TxError::RevisionConflict {
            expected: Revision(1),
            actual: Some(Revision(2)),
            ..
        }))
    ));
    // Re-importing the current content is not an edit, and an existing artifact cannot be
    // created again.
    let same = import_as(
        &engine,
        4,
        PRD,
        FEATURE,
        Some(Revision(2)),
        &second.content,
        human(""),
    );
    assert!(matches!(
        same,
        Err(CommandError::Rejected(TxError::Rejected(_)))
    ));
    let again = import_as(&engine, 5, PRD, FEATURE, None, COMPLETE_PRD_JSON, human(""));
    assert!(matches!(
        again,
        Err(CommandError::Rejected(TxError::AggregateExists(_)))
    ));
    let unknown = import_as(
        &engine,
        6,
        OTHER_PRD,
        FEATURE,
        Some(Revision(1)),
        COMPLETE_PRD_JSON,
        human(""),
    );
    assert!(matches!(
        unknown,
        Err(CommandError::Rejected(TxError::RevisionConflict {
            actual: None,
            ..
        }))
    ));
    assert_eq!(versions(&engine, PRD), both);
    assert_eq!(versions(&engine, OTHER_PRD), []);
    assert_eq!(event_cursor(&engine), cursor);

    // Reverting is a new version with the old content, not a return to the old version.
    import(&engine, 7, Some(Revision(2)), COMPLETE_PRD_JSON);
    let three = versions(&engine, PRD);
    assert_eq!(three[..2], both[..]);
    assert_eq!(three[2].content_digest, first[0].content_digest);
    assert_eq!(
        (three[2].revision, three[2].parent),
        (Revision(3), Some(Revision(2)))
    );
}

#[test]
fn a_rendering_round_trip_preserves_the_canonical_revision_and_digests() {
    let dir = TempDir::new("artifact-round-trip");
    let stored = {
        let engine = start(&dir.db());
        import(&engine, 1, None, COMPLETE_PRD_JSON);
        // The same content in a different layout is the same canonical content.
        let relaid = edited(
            r#"{"id": "ACT-READER", "name": "Reader"}"#,
            "{\"name\":\"R\\u0065ader\",\n\n\"id\":\"ACT-READER\"}",
        );
        import_as(&engine, 2, OTHER_PRD, FEATURE, None, &relaid, human("")).unwrap();
        let stored = versions(&engine, PRD).remove(0);
        let other = versions(&engine, OTHER_PRD).remove(0);
        assert_eq!(other.content, stored.content);
        assert_eq!(
            (&other.content_digest, &other.rendered_digest),
            (&stored.content_digest, &stored.rendered_digest)
        );
        engine.shutdown().unwrap();
        stored
    };

    // The digests are plain SHA-256 of the stored bytes: the reviewed bytes are exactly
    // identifiable by anyone holding the digest.
    assert_eq!(
        sha256_hex(&[stored.content.as_bytes()]),
        stored.content_digest
    );
    assert_eq!(
        sha256_hex(&[stored.rendered.as_bytes()]),
        stored.rendered_digest
    );
    assert!(stored.rendered.starts_with("# PRD: Reading list\n"));

    let engine = start(&dir.db());
    let reloaded = versions(&engine, PRD).remove(0);
    assert_eq!(
        reloaded, stored,
        "a restart changes nothing about a version"
    );

    // Stored canonical content -> typed document -> canonical content and rendering:
    // the same bytes and the same digests, at the same revision.
    let again = PreparedPrd::import(reloaded.content.as_bytes()).unwrap();
    assert_eq!(again.content(), stored.content);
    assert_eq!(again.content_digest(), stored.content_digest);
    assert_eq!(again.rendered(), stored.rendered);
    assert_eq!(again.rendered_digest(), stored.rendered_digest);
    let unchanged = import_as(
        &engine,
        3,
        PRD,
        FEATURE,
        Some(Revision(1)),
        &reloaded.content,
        human(""),
    );
    assert!(matches!(
        unchanged,
        Err(CommandError::Rejected(TxError::Rejected(_)))
    ));
    let ready = readiness(&engine, PRD);
    assert_eq!(
        (
            ready.revision,
            &ready.content_digest,
            &ready.rendered_digest
        ),
        (Revision(1), &stored.content_digest, &stored.rendered_digest)
    );
    assert_eq!(versions(&engine, PRD), [stored]);
}

#[test]
fn an_answer_survives_restart_with_its_provenance() {
    let dir = TempDir::new("answer-restart");
    let question;
    let (asked, answered): (_, Vec<Answer>);
    {
        let engine = start(&dir.db());
        import(&engine, 1, None, COMPLETE_PRD_JSON);
        question = ask(
            &engine,
            2,
            0xB1,
            FEATURE,
            Some(PRD),
            QuestionDisposition::Blocking,
        );
        let receipt = answer(
            &engine,
            3,
            question,
            Revision(1),
            "Until the reader removes them.",
            human("answered by the product owner in the review UI"),
        )
        .unwrap();
        assert_eq!(receipt.response, "2");
        // A stale revision cannot append an answer.
        let stale = answer(&engine, 4, question, Revision(1), "Thirty days.", human(""));
        assert!(matches!(
            stale,
            Err(CommandError::Rejected(TxError::RevisionConflict { .. }))
        ));
        let blank = answer(&engine, 5, question, Revision(2), " \n", human(""));
        assert!(matches!(
            blank,
            Err(CommandError::Rejected(TxError::Rejected(_)))
        ));
        let mut reader = engine.reader().unwrap();
        (asked, answered) = reader
            .read(|view| Ok((view.question(question)?, view.answers(question)?)))
            .unwrap();
        // No clean shutdown: the engine is dropped as if the helper had been killed.
    }
    let asked = asked.expect("question persisted");
    assert_eq!(
        (
            asked.feature,
            asked.artifact,
            asked.revision,
            asked.disposition
        ),
        (
            FEATURE,
            Some(PRD),
            Revision(2),
            QuestionDisposition::Blocking
        )
    );
    assert_eq!(asked.prompt, "How long are saved articles kept?");
    assert_eq!(asked.provenance, model("raised while drafting UC-1"));
    assert_eq!(answered.len(), 1);
    assert_eq!(
        (answered[0].ordinal, answered[0].body.as_str()),
        (1, "Until the reader removes them.")
    );
    assert_eq!(
        answered[0].provenance,
        human("answered by the product owner in the review UI")
    );

    let engine = start(&dir.db());
    assert_eq!(engine.epoch().0, 2);
    let mut reader = engine.reader().unwrap();
    let (asked_after, answered_after) = reader
        .read(|view| Ok((view.question(question)?, view.answers(question)?)))
        .unwrap();
    assert_eq!(asked_after, Some(asked));
    assert_eq!(answered_after, answered);

    // The original command replays its stored response instead of answering twice.
    let replay = answer(
        &engine,
        3,
        question,
        Revision(1),
        "Until the reader removes them.",
        human("answered by the product owner in the review UI"),
    )
    .unwrap();
    assert!(replay.replayed && replay.response == "2");

    // A later answer is a new version of the answer; the earlier one is kept.
    answer(
        &engine,
        6,
        question,
        Revision(2),
        "For one year.",
        human("revised"),
    )
    .unwrap();
    let all = reader.read(|view| view.answers(question)).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0], answered[0]);
    assert_eq!((all[1].ordinal, all[1].body.as_str()), (2, "For one year."));
    assert!(all[1].created_event > all[0].created_event);
}

#[test]
fn incomplete_content_and_unanswered_blocking_questions_prevent_review_readiness() {
    let dir = TempDir::new("readiness");
    let engine = start(&dir.db());
    import(&engine, 1, None, COMPLETE_PRD_JSON);
    assert!(readiness(&engine, PRD).is_ready());

    // An unanswered blocking question about this artifact blocks.
    let question = ask(
        &engine,
        2,
        0xB1,
        FEATURE,
        Some(PRD),
        QuestionDisposition::Blocking,
    );
    let unanswered = [ReadinessBlocker::UnansweredQuestion(question)];
    assert_eq!(readiness(&engine, PRD).blockers, unanswered);
    // A model's answer is recorded but does not settle a blocking question.
    answer(
        &engine,
        3,
        question,
        Revision(1),
        "Probably forever.",
        model("guess"),
    )
    .unwrap();
    assert_eq!(readiness(&engine, PRD).blockers, unanswered);
    answer(
        &engine,
        4,
        question,
        Revision(2),
        "Until removed.",
        human("owner"),
    )
    .unwrap();
    assert!(readiness(&engine, PRD).is_ready());
    // A newer model answer supersedes the human one, so the question blocks again.
    answer(
        &engine,
        5,
        question,
        Revision(3),
        "Actually 30 days.",
        model("guess"),
    )
    .unwrap();
    assert_eq!(readiness(&engine, PRD).blockers, unanswered);
    answer(
        &engine,
        6,
        question,
        Revision(4),
        "Until removed.",
        human("owner"),
    )
    .unwrap();
    assert!(readiness(&engine, PRD).is_ready());

    // A feature-wide blocking question blocks until it is explicitly deferred; questions
    // about another artifact or another feature never block this one.
    let feature_wide = ask(
        &engine,
        7,
        0xB2,
        FEATURE,
        None,
        QuestionDisposition::Blocking,
    );
    import_as(
        &engine,
        8,
        OTHER_PRD,
        OTHER_FEATURE,
        None,
        COMPLETE_PRD_JSON,
        human(""),
    )
    .unwrap();
    ask(
        &engine,
        9,
        0xB3,
        OTHER_FEATURE,
        None,
        QuestionDisposition::Blocking,
    );
    let elsewhere = ask(
        &engine,
        10,
        0xB4,
        OTHER_FEATURE,
        Some(OTHER_PRD),
        QuestionDisposition::Blocking,
    );
    ask(
        &engine,
        11,
        0xB5,
        FEATURE,
        None,
        QuestionDisposition::Deferred,
    );
    assert_eq!(
        readiness(&engine, PRD).blockers,
        [ReadinessBlocker::UnansweredQuestion(feature_wide)]
    );
    assert_eq!(readiness(&engine, OTHER_PRD).blockers.len(), 2);
    assert!(
        readiness(&engine, OTHER_PRD)
            .blockers
            .contains(&ReadinessBlocker::UnansweredQuestion(elsewhere))
    );
    let set = |command, expected, disposition| {
        engine.execute(request(&engine, command, "disposition", ""), move |tx| {
            tx.set_question_disposition(feature_wide, expected, disposition)?;
            Ok(String::new())
        })
    };
    set(12, Revision(1), QuestionDisposition::Deferred).unwrap();
    assert!(readiness(&engine, PRD).is_ready());
    assert!(matches!(
        set(13, Revision(1), QuestionDisposition::Blocking),
        Err(CommandError::Rejected(TxError::RevisionConflict { .. }))
    ));
    set(14, Revision(2), QuestionDisposition::Blocking).unwrap();
    assert_eq!(
        readiness(&engine, PRD).blockers,
        [ReadinessBlocker::UnansweredQuestion(feature_wide)]
    );
    set(15, Revision(3), QuestionDisposition::Deferred).unwrap();

    // Structural gaps in the newest version block with their field paths. The draft is
    // stored (it is a real version) but is not ready; fixing it is another version.
    let alternate = r#"[
        {
          "id": "UC-1-A1",
          "title": "Article already saved",
          "steps": ["The system shows the existing entry and stores nothing."]
        }
      ]"#;
    import(&engine, 16, Some(Revision(1)), &edited(alternate, "[]"));
    let draft = readiness(&engine, PRD);
    assert_eq!(draft.revision, Revision(2));
    assert_eq!(
        draft.blockers,
        [incomplete(
            "use_cases[0].alternate_flows",
            Incomplete::Empty
        )]
    );
    import(&engine, 17, Some(Revision(2)), &with_goal("TBD"));
    assert_eq!(
        readiness(&engine, PRD).blockers,
        [incomplete("goal", Incomplete::Placeholder)]
    );
    let blocker = r#""blockers": [{"id": "BLK-1", "statement": "Retention is undecided."}]"#;
    import(
        &engine,
        18,
        Some(Revision(3)),
        &edited(r#""blockers": []"#, blocker).replace(r#"["AC-1", "AC-2"]"#, "[]"),
    );
    set(19, Revision(4), QuestionDisposition::Blocking).unwrap();
    assert_eq!(
        readiness(&engine, PRD).blockers,
        [
            incomplete("use_cases[0].acceptance_ids", Incomplete::Empty),
            incomplete("blockers[0]", Incomplete::OpenBlocker),
            ReadinessBlocker::UnansweredQuestion(feature_wide),
        ]
    );
    set(20, Revision(5), QuestionDisposition::Deferred).unwrap();
    import(&engine, 21, Some(Revision(4)), COMPLETE_PRD_JSON);
    let fixed = readiness(&engine, PRD);
    assert!(fixed.is_ready());
    assert_eq!(fixed.revision, Revision(5));
    assert_eq!(
        fixed.content_digest,
        versions(&engine, PRD)[0].content_digest
    );
    assert_eq!(
        versions(&engine, PRD).len(),
        5,
        "every draft is still there"
    );

    let absent = engine
        .reader()
        .unwrap()
        .read(|view| view.review_readiness(ArtifactId(OpaqueId(0xDEAD))))
        .unwrap();
    assert_eq!(absent, None);
}

#[test]
fn rejected_input_never_becomes_a_version() {
    // Refused before any transaction exists: there is nothing to prepare.
    let duplicate_key = edited(
        r#""title": "Reading list","#,
        r#""title": "Reading list", "title": "Reading list","#,
    );
    assert!(matches!(
        PreparedPrd::import(duplicate_key.as_bytes()),
        Err(PrdError::Json(JsonError::DuplicateKey { .. }))
    ));
    let oversized = format!("{COMPLETE_PRD_JSON}{}", " ".repeat(MAX_INPUT_BYTES));
    assert!(matches!(
        PreparedPrd::import(oversized.as_bytes()),
        Err(PrdError::Json(JsonError::TooLarge { .. }))
    ));
    let deep = edited(
        r#""blockers": []"#,
        &format!(r#""blockers": {}"#, "[".repeat(64)),
    );
    assert!(matches!(
        PreparedPrd::import(deep.as_bytes()),
        Err(PrdError::Json(JsonError::TooDeep { .. }))
    ));
    let duplicate_id = edited(r#""id": "AC-2""#, r#""id": "AC-1""#);
    assert!(matches!(
        PreparedPrd::import(duplicate_id.as_bytes()),
        Err(PrdError::Field { .. })
    ));

    // Refused inside the transaction: the artifact, its event and the receipt all vanish.
    let dir = TempDir::new("artifact-rejected");
    let engine = start(&dir.db());
    let cursor = event_cursor(&engine);
    let long_note = "x".repeat(MAX_TEXT_BYTES + 1);
    let refused = import_as(
        &engine,
        1,
        PRD,
        FEATURE,
        None,
        COMPLETE_PRD_JSON,
        human(&long_note),
    );
    assert!(matches!(
        refused,
        Err(CommandError::Rejected(TxError::Rejected(_)))
    ));
    let prepared = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
    let failed_late = engine.execute(request(&engine, 2, "import-then-fail", ""), move |tx| {
        tx.create_artifact(PRD, FEATURE, &prepared, &human(""))?;
        tx.ask_question(
            QuestionId(OpaqueId(0xB1)),
            FEATURE,
            Some(OTHER_PRD),
            "About an artifact that does not exist?",
            QuestionDisposition::Blocking,
            &human(""),
        )?;
        Ok(String::new())
    });
    assert!(matches!(
        failed_late,
        Err(CommandError::Rejected(TxError::Rejected(_)))
    ));
    assert_eq!(versions(&engine, PRD), []);
    assert_eq!(event_cursor(&engine), cursor);
    let mut reader = engine.reader().unwrap();
    assert_eq!(reader.snapshot().unwrap().aggregates, []);
    assert_eq!(
        reader
            .receipt(jojo_storage::CommandId(OpaqueId(2)))
            .unwrap(),
        None
    );
    // The same command ID can then be used for a correct import.
    import(&engine, 2, None, COMPLETE_PRD_JSON);
    assert_eq!(versions(&engine, PRD).len(), 1);
}

#[test]
fn provenance_text_of_adapted_content_is_retained_verbatim() {
    let dir = TempDir::new("artifact-provenance");
    let notice = "Section headings adapted from the Example Spec template, commit 0123abc.\n\
                  Licensed under the MIT License; copyright (c) Example Authors.\n\
                  Modifications: reduced to one use case; statements rewritten.";
    {
        let engine = start(&dir.db());
        import_as(
            &engine,
            1,
            PRD,
            FEATURE,
            None,
            COMPLETE_PRD_JSON,
            human(notice),
        )
        .unwrap();
        import_as(
            &engine,
            2,
            PRD,
            FEATURE,
            Some(Revision(1)),
            &with_goal("Keep a private reading list."),
            model("rewording proposed by a fake provider fixture"),
        )
        .unwrap();
        engine.shutdown().unwrap();
    }
    let engine = start(&dir.db());
    let stored = versions(&engine, PRD);
    assert_eq!(stored[0].provenance, human(notice));
    assert_eq!(
        stored[1].provenance,
        model("rewording proposed by a fake provider fixture")
    );
    // Provenance is about the version, not part of the reviewed content or its digests.
    assert!(!stored[0].content.contains("Example Spec"));
    assert!(!stored[0].rendered.contains("Example Spec"));
}

#[test]
fn a_version_1_database_upgrades_with_a_backup_and_keeps_its_history() {
    let dir = TempDir::new("artifact-upgrade");
    let counter = key("counter", 1);
    {
        let mut config = EngineConfig::new(dir.db(), clock());
        config.migrations =
            MigrationPlan::new(MigrationPlan::production().steps()[..1].to_vec()).unwrap();
        let engine = Engine::start(config).unwrap();
        assert_eq!(engine.startup_report().migration.to_version, 1);
        create(&engine, 1, &counter);
        engine.shutdown().unwrap();
    }
    let engine = start(&dir.db());
    let report = &engine.startup_report().migration;
    assert_eq!((report.from_version, report.to_version), (1, 11));
    assert!(report.backup.as_ref().is_some_and(|backup| backup.exists()));
    assert_eq!(
        engine
            .reader()
            .unwrap()
            .snapshot()
            .unwrap()
            .aggregates
            .len(),
        1
    );
    assert!(create(&engine, 1, &counter).replayed);
    import(&engine, 2, None, COMPLETE_PRD_JSON);
    assert!(readiness(&engine, PRD).is_ready());
}

#[test]
fn canonical_fixture_keeps_its_pre_hardening_content_hash() {
    // Independently computed from the fixture with Python json + hashlib; field
    // order and escaping match the unchanged P1-03 canonical writer.
    let prepared = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
    assert_eq!(
        prepared.content_digest(),
        "eea050d59d0c77467673379b55fd9fe8ae504c3cbde9d3fef3fdd691d35bb10a"
    );
}
