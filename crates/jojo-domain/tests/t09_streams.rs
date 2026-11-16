//! T09 pure stream framing/normalization. Synthetic records only; no process or I/O.
use jojo_domain::provider::{EventPayload, ProviderKind, SessionRef, TerminalStatus};
use jojo_domain::stream::*;

const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"s-1"}"#;
const OK: &str = r#"{"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":2,"output_tokens":3}}"#;

fn run(parser: &str, chunks: &[&[u8]]) -> (Vec<(u64, u64, EventPayload)>, StreamNormalizer) {
    let mut framer = LineFramer::new(1024);
    let mut n = StreamNormalizer::for_parser(parser).unwrap();
    let mut out = Vec::new();
    let mut frames = Vec::new();
    for c in chunks {
        frames.extend(framer.push(c));
    }
    frames.extend(framer.finish());
    for f in frames {
        for p in n.frame(&f) {
            out.push((f.start, f.end, p));
        }
    }
    (out, n)
}

fn payloads(v: &[(u64, u64, EventPayload)]) -> Vec<EventPayload> {
    v.iter().map(|(_, _, p)| p.clone()).collect()
}

#[test]
fn t09_fragmented_utf8_ndjson_is_identical_at_every_split_point() {
    let text = format!(
        "{INIT}\n{}\n{OK}\n",
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"é🙂ß"}]}}"#
    );
    let bytes = text.as_bytes();
    let (whole, _) = run(CLAUDE_STREAM_JSON, &[bytes]);
    assert_eq!(whole.len(), 5);
    for a in 0..bytes.len() {
        for b in [a, (a + 3).min(bytes.len()), bytes.len()] {
            let (parts, n) = run(
                CLAUDE_STREAM_JSON,
                &[&bytes[..a], &bytes[a..b], &bytes[b..]],
            );
            assert_eq!(parts, whole, "split at {a}/{b}");
            assert!(!n.protocol_failed());
        }
    }
    // Ranges cover exactly the raw record bytes, including the newline.
    let (start, end, _) = &whole[2];
    assert!(bytes[*start as usize..*end as usize].ends_with("é🙂ß\"}]}}\n".as_bytes()));
    assert!(matches!(&whole[2].2, EventPayload::TextDelta(t) if t == "é🙂ß"));
}

#[test]
fn t09_frame_limit_is_exact_bounded_and_resynchronizes() {
    let mut f = LineFramer::new(8);
    let frames = f.push(b"12345678\n123456789\nok\n");
    assert_eq!(frames[0].kind, FrameKind::Record(b"12345678".to_vec()));
    assert_eq!(
        (frames[1].start, frames[1].end, &frames[1].kind),
        (9, 19, &FrameKind::Oversized)
    );
    assert_eq!(frames[2].kind, FrameKind::Record(b"ok".to_vec()));
    // An over-long record spread across many chunks never retains more than limit+1 bytes.
    let mut f = LineFramer::new(4);
    for _ in 0..10_000 {
        assert!(f.push(b"xxxxxxxx").is_empty());
    }
    let tail = f.push(b"\n{}\n");
    assert_eq!(tail[0].kind, FrameKind::Oversized);
    assert_eq!(tail[1].kind, FrameKind::Record(b"{}".to_vec()));
    assert_eq!(f.offset(), 80_004);
}

#[test]
fn t09_oversized_unterminated_and_truncated_streams_fail_closed() {
    let big = format!("{INIT}\n{}\n{OK}\n", "x".repeat(2000));
    let (events, n) = run(CLAUDE_STREAM_JSON, &[big.as_bytes()]);
    let p = payloads(&events);
    assert!(p.contains(&EventPayload::StreamTruncated));
    assert!(n.protocol_failed());
    assert!(!p.iter().any(|e| matches!(
        e,
        EventPayload::ProviderTerminal {
            status: TerminalStatus::Success,
            ..
        }
    )));
    let (events, n) = run(CLAUDE_STREAM_JSON, &[format!("{INIT}\n{OK}").as_bytes()]);
    assert!(n.protocol_failed() && n.terminal().is_none());
    assert!(matches!(
        events.last().unwrap().2,
        EventPayload::ProtocolViolation(_)
    ));
    let mut n = StreamNormalizer::for_parser(CODEX_EXEC_JSON).unwrap();
    assert_eq!(n.output_truncated()[0], EventPayload::StreamTruncated);
    assert!(n.protocol_failed());
}

#[test]
fn t09_claude_profile_maps_session_tools_usage_and_explicit_terminal() {
    let tool = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1"}]}}"#;
    let res =
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}"#;
    let text = format!("{INIT}\n{tool}\n{res}\n{{\"type\":\"future_kind\"}}\n{OK}\n");
    let (events, n) = run(CLAUDE_STREAM_JSON, &[text.as_bytes()]);
    assert_eq!(
        payloads(&events),
        vec![
            EventPayload::SessionIdentified(SessionRef {
                provider: ProviderKind::ClaudeCode,
                session: "s-1".into(),
                turn: "turn-1".into()
            }),
            EventPayload::TurnStarted {
                turn: "turn-1".into()
            },
            EventPayload::ToolStarted { id: "t1".into() },
            EventPayload::ToolFinished { id: "t1".into() },
            EventPayload::Informational("future_kind:".into()),
            EventPayload::UsageObserved { units: 5 },
            EventPayload::ProviderTerminal {
                turn: "turn-1".into(),
                status: TerminalStatus::Success,
                raw_status: "success".into()
            },
        ]
    );
    assert_eq!(n.terminal(), Some(TerminalStatus::Success));
    let err = r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#;
    let odd = r#"{"type":"result","subtype":"success"}"#;
    for (record, status) in [(err, TerminalStatus::Error), (odd, TerminalStatus::Unknown)] {
        let (_, n) = run(
            CLAUDE_STREAM_JSON,
            &[format!("{INIT}\n{record}\n").as_bytes()],
        );
        assert_eq!(n.terminal(), Some(status), "{record}");
    }
}

#[test]
fn t09_codex_profile_maps_dotted_events_and_failed_turn_is_not_success() {
    let ok = concat!(
        r#"{"type":"thread.started","thread_id":"th"}"#,
        "\n",
        r#"{"type":"turn.started"}"#,
        "\n",
        r#"{"type":"item.started","item":{"id":"c1","type":"command_execution"}}"#,
        "\n",
        r#"{"type":"item.completed","item":{"id":"c1","type":"command_execution"}}"#,
        "\n",
        r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"hi"}}"#,
        "\n",
        r#"{"type":"item.completed","item":{"id":"r1","type":"reasoning"}}"#,
        "\n",
        r#"{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}"#,
        "\n",
    );
    let (events, n) = run(CODEX_EXEC_JSON, &[ok.as_bytes()]);
    let p = payloads(&events);
    assert!(
        matches!(&p[0], EventPayload::SessionIdentified(s) if s.session == "th" && s.provider == ProviderKind::Codex)
    );
    assert_eq!(p[2], EventPayload::ToolStarted { id: "c1".into() });
    assert_eq!(p[3], EventPayload::ToolFinished { id: "c1".into() });
    assert_eq!(p[4], EventPayload::TextDelta("hi".into()));
    assert_eq!(n.terminal(), Some(TerminalStatus::Success));
    let failed = concat!(
        r#"{"type":"thread.started","thread_id":"th"}"#,
        "\n",
        r#"{"type":"turn.started"}"#,
        "\n",
        r#"{"type":"error","message":"transient"}"#,
        "\n",
        r#"{"type":"turn.failed","error":{"message":"boom"}}"#,
        "\n",
    );
    let (events, n) = run(CODEX_EXEC_JSON, &[failed.as_bytes()]);
    assert_eq!(n.terminal(), Some(TerminalStatus::Error));
    assert!(payloads(&events).contains(&EventPayload::ProviderError("boom".into())));
    // A turn end without a started turn is a protocol violation, never a terminal.
    let (_, n) = run(
        CODEX_EXEC_JSON,
        &[br#"{"type":"turn.completed","usage":{}}
"#],
    );
    assert!(n.protocol_failed() && n.terminal().is_none());
}

#[test]
fn t09_malformed_records_latch_failure_and_block_later_success() {
    for bad in [
        &b"not json"[..],
        b"\xff\xfe{}",
        b"[1,2]",
        br#"{"no_type":1}"#,
    ] {
        let mut text = format!("{INIT}\n").into_bytes();
        text.extend_from_slice(bad);
        text.extend_from_slice(format!("\n{OK}\n").as_bytes());
        let (events, n) = run(CLAUDE_STREAM_JSON, &[&text]);
        assert!(n.protocol_failed(), "{bad:?}");
        assert_eq!(n.terminal(), None, "{bad:?}");
        assert!(matches!(
            events.last().unwrap().2,
            EventPayload::ProtocolViolation(_)
        ));
    }
    let (events, n) = run(
        CLAUDE_STREAM_JSON,
        &[format!("{INIT}\n{OK}\n{OK}\n").as_bytes()],
    );
    assert_eq!(n.terminal(), Some(TerminalStatus::Success));
    assert!(n.protocol_failed());
    assert_eq!(
        events.last().unwrap().2,
        EventPayload::ProtocolViolation("duplicate terminal record".into())
    );
}

#[test]
fn t09_nesting_and_parser_fingerprints_are_bounded() {
    let deep = format!("{}{}", "[".repeat(40), "]".repeat(40));
    assert_eq!(json_depth(deep.as_bytes(), MAX_RECORD_DEPTH), None);
    assert_eq!(
        json_depth(br#"{"a":"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[["}"#, 4),
        Some(1)
    );
    let (_, n) = run(
        CLAUDE_STREAM_JSON,
        &[format!("{{\"type\":\"x\",\"v\":{deep}}}\n").as_bytes()],
    );
    assert!(n.protocol_failed());
    assert!(StreamNormalizer::for_parser("synthetic-parser-v1").is_err());
    // Blank lines are captured raw but produce no event.
    let (events, n) = run(CLAUDE_STREAM_JSON, &[b"\n  \n"]);
    assert!(events.is_empty() && !n.protocol_failed());
}

#[test]
fn t09_missing_or_changed_session_cannot_normalize_success() {
    for text in [
        "{\"type\":\"turn.started\"}\n{\"type\":\"turn.completed\"}\n".to_owned(),
        "{\"type\":\"thread.started\",\"thread_id\":\"a\"}\n{\"type\":\"thread.started\",\"thread_id\":\"b\"}\n{\"type\":\"turn.started\"}\n{\"type\":\"turn.completed\"}\n".to_owned(),
    ] {
        let (_, n) = run(CODEX_EXEC_JSON, &[text.as_bytes()]);
        assert!(n.protocol_failed());
        assert_ne!(n.terminal(), Some(TerminalStatus::Success));
    }
    let (_, n) = run(CLAUDE_STREAM_JSON, &[format!("{OK}\n").as_bytes()]);
    assert!(n.protocol_failed() && n.terminal().is_none());
}

#[test]
fn t09_explicit_codex_error_status_is_never_promoted_to_success() {
    let prefix = "{\"type\":\"thread.started\",\"thread_id\":\"a\"}\n{\"type\":\"turn.started\"}\n";
    for (status, expected) in [
        ("error", TerminalStatus::Error),
        ("cancelled", TerminalStatus::Interrupted),
        ("future-status", TerminalStatus::Unknown),
    ] {
        let record = format!("{prefix}{{\"type\":\"turn.completed\",\"status\":\"{status}\"}}\n");
        let (_, n) = run(CODEX_EXEC_JSON, &[record.as_bytes()]);
        assert_eq!(n.terminal(), Some(expected));
    }
}
