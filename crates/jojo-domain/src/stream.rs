//! Bounded NDJSON framing and provider stream normalization (P1-09). Pure: no I/O.
//!
//! Bytes are captured by the caller before they reach this module; every frame carries
//! the absolute byte range it came from, so a normalized event always points back at
//! retained raw bytes. Framing is byte-level, so records and UTF-8 sequences may be split
//! across arbitrary chunks. A frame holds at most `max_frame_bytes + 1` bytes: one byte
//! past the limit distinguishes an over-long record from one of exactly the limit, and
//! the rest of an over-long record is discarded up to its newline (technique reproduced,
//! not copied, from Vigla's capped line reader; see `research/REPOSITORY_PINS.json`). Unlike that
//! reader, an over-long or unterminated record is never dropped silently: it becomes a
//! truncation/protocol failure that latches the stream closed for success.
//!
//! The two synthetic profiles map the *synthetic fixture* shapes of Claude Code `-p
//! --output-format stream-json` and `codex exec --json` onto neutral payloads. They are
//! not installed-release compatibility certification. Exit status never produces a
//! terminal record here; only an explicit, well-formed terminal record does.
//!
//! The production Codex profile (P2-01, [`CODEX_EXEC_JSONL`]) models the documented
//! `codex exec --json` record shapes for one pinned release and nothing else. Where the
//! synthetic profile preserves an unknown record as informational, this one latches a
//! protocol failure: an unmodelled record kind, item type, extra turn, tool outside the
//! approved envelope or any record after the terminal one can never end in success. The
//! real stream of that release has not been observed; the profile is conservative, not
//! certified.
pub use crate::codex::CODEX_EXEC_JSONL;
use crate::provider::{EventPayload, ProviderError, ProviderKind, SessionRef, TerminalStatus};
use serde_json::Value;

/// Parser fingerprints accepted by [`StreamNormalizer::for_parser`].
pub const CLAUDE_STREAM_JSON: &str = "jojo.claude.stream-json.synthetic/1";
pub const CODEX_EXEC_JSON: &str = "jojo.codex.exec-json.synthetic/1";
/// Deepest accepted array/object nesting of one record. The top-level value is depth 1.
pub const MAX_RECORD_DEPTH: usize = 32;
/// Longest thread or item identifier the production Codex profile accepts.
pub const MAX_IDENTIFIER_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameKind {
    /// A complete record without its trailing newline.
    Record(Vec<u8>),
    /// Longer than the limit; bytes were captured but not retained for parsing.
    Oversized,
    /// Bytes after the last newline at end of stream.
    Unterminated(Vec<u8>),
}

/// `start..end` is the absolute stream range, including the newline when present.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub start: u64,
    pub end: u64,
    pub kind: FrameKind,
}

#[derive(Debug)]
pub struct LineFramer {
    max: usize,
    offset: u64,
    start: u64,
    buf: Vec<u8>,
    discarding: bool,
}

impl LineFramer {
    pub fn new(max_frame_bytes: usize) -> Self {
        Self {
            max: max_frame_bytes.max(1),
            offset: 0,
            start: 0,
            buf: Vec::new(),
            discarding: false,
        }
    }

    /// Total bytes consumed so far.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn push(&mut self, mut chunk: &[u8]) -> Vec<Frame> {
        let mut frames = Vec::new();
        while !chunk.is_empty() {
            let newline = chunk.iter().position(|b| *b == b'\n');
            let take = newline.map_or(chunk.len(), |i| i + 1);
            let (segment, rest) = chunk.split_at(take);
            chunk = rest;
            self.offset += take as u64;
            let body = &segment[..segment.len() - usize::from(newline.is_some())];
            if !self.discarding {
                let room = (self.max + 1).saturating_sub(self.buf.len());
                self.buf.extend_from_slice(&body[..body.len().min(room)]);
                if self.buf.len() > self.max {
                    self.buf = Vec::new();
                    self.discarding = true;
                }
            }
            if newline.is_some() {
                let kind = if self.discarding {
                    FrameKind::Oversized
                } else {
                    FrameKind::Record(std::mem::take(&mut self.buf))
                };
                frames.push(Frame {
                    start: self.start,
                    end: self.offset,
                    kind,
                });
                self.start = self.offset;
                self.discarding = false;
            }
        }
        frames
    }

    /// End of stream. Returns the trailing partial record, if any bytes follow the last
    /// newline.
    pub fn finish(&mut self) -> Option<Frame> {
        if self.start == self.offset {
            return None;
        }
        let kind = if self.discarding {
            FrameKind::Oversized
        } else {
            FrameKind::Unterminated(std::mem::take(&mut self.buf))
        };
        let frame = Frame {
            start: self.start,
            end: self.offset,
            kind,
        };
        self.start = self.offset;
        self.discarding = false;
        Some(frame)
    }
}

/// Nesting depth of a JSON text, scanning strings correctly; `None` if it exceeds `limit`.
pub fn json_depth(bytes: &[u8], limit: usize) -> Option<usize> {
    let (mut depth, mut deepest, mut in_string, mut escaped) = (0usize, 0usize, false, false);
    for b in bytes {
        if in_string {
            match (escaped, *b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                deepest = deepest.max(depth);
                if depth > limit {
                    return None;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Some(deepest)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Profile {
    Claude,
    Codex,
    /// Production Codex `exec --json`, pinned release, fail-closed.
    CodexExec,
}

/// One normalizer per attempt stdout. Stderr is captured but never parsed as protocol.
#[derive(Debug)]
pub struct StreamNormalizer {
    profile: Profile,
    turns: u64,
    session_id: Option<String>,
    open_turn: Option<String>,
    terminal: Option<TerminalStatus>,
    failed: bool,
    /// The provider reported an error; a later completion is not taken as success.
    errored: bool,
}

impl StreamNormalizer {
    pub fn for_parser(parser: &str) -> Result<Self, ProviderError> {
        let profile = match parser {
            CLAUDE_STREAM_JSON => Profile::Claude,
            CODEX_EXEC_JSON => Profile::Codex,
            CODEX_EXEC_JSONL => Profile::CodexExec,
            _ => return Err(ProviderError::FingerprintMismatch),
        };
        Ok(Self {
            profile,
            turns: 0,
            session_id: None,
            open_turn: None,
            terminal: None,
            failed: false,
            errored: false,
        })
    }

    pub fn provider(&self) -> ProviderKind {
        match self.profile {
            Profile::Claude => ProviderKind::ClaudeCode,
            Profile::Codex | Profile::CodexExec => ProviderKind::Codex,
        }
    }

    /// Latched by any truncation or malformed record. Once set, no success is normalized.
    pub fn protocol_failed(&self) -> bool {
        self.failed
    }

    /// First accepted terminal status. A later terminal record is a protocol violation.
    pub fn terminal(&self) -> Option<TerminalStatus> {
        self.terminal
    }

    /// Normalization stops at the cap: bytes beyond it are not framed, so the record in
    /// flight cannot be complete. The caller emits this once when its spool cap is hit.
    pub fn output_truncated(&mut self) -> Vec<EventPayload> {
        self.failed = true;
        vec![
            EventPayload::StreamTruncated,
            EventPayload::ProtocolViolation("output limit reached".into()),
        ]
    }

    pub fn frame(&mut self, frame: &Frame) -> Vec<EventPayload> {
        let mut out = Vec::new();
        match &frame.kind {
            FrameKind::Oversized => {
                out.push(EventPayload::StreamTruncated);
                self.violation(&mut out, "record exceeds frame limit");
            }
            FrameKind::Unterminated(_) => self.violation(&mut out, "unterminated final record"),
            FrameKind::Record(bytes) => self.record(bytes, &mut out),
        }
        out
    }

    fn violation(&mut self, out: &mut Vec<EventPayload>, why: &str) {
        self.failed = true;
        out.push(EventPayload::ProtocolViolation(why.into()));
    }

    fn record(&mut self, bytes: &[u8], out: &mut Vec<EventPayload>) {
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return self.violation(out, "record is not UTF-8");
        };
        if json_depth(bytes, MAX_RECORD_DEPTH).is_none() {
            return self.violation(out, "record nesting exceeds limit");
        }
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return self.violation(out, "record is not JSON");
        };
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            return self.violation(out, "record has no string type");
        };
        let kind = kind.to_owned();
        match self.profile {
            Profile::Claude => self.claude(&kind, &value, out),
            Profile::Codex => self.codex(&kind, &value, out),
            Profile::CodexExec => self.codex_exec(&kind, &value, out),
        }
    }

    fn next_turn(&mut self) -> String {
        self.turns += 1;
        format!("turn-{}", self.turns)
    }

    fn session(&self, session: &str, turn: &str) -> EventPayload {
        EventPayload::SessionIdentified(SessionRef {
            provider: self.provider(),
            session: session.to_owned(),
            turn: turn.to_owned(),
        })
    }

    fn terminal_record(
        &mut self,
        out: &mut Vec<EventPayload>,
        turn: String,
        status: TerminalStatus,
        raw: &str,
    ) {
        if self.terminal.is_some() {
            return self.violation(out, "duplicate terminal record");
        }
        if self.failed && status == TerminalStatus::Success {
            return self.violation(out, "terminal success after protocol failure");
        }
        self.terminal = Some(status);
        out.push(EventPayload::ProviderTerminal {
            turn,
            status,
            raw_status: raw.to_owned(),
        });
    }

    fn usage(out: &mut Vec<EventPayload>, usage: Option<&Value>) {
        let field = |k| usage.and_then(|u| u.get(k)).and_then(Value::as_u64);
        if let (Some(i), Some(o)) = (field("input_tokens"), field("output_tokens")) {
            out.push(EventPayload::UsageObserved {
                units: i.saturating_add(o),
            });
        }
    }

    fn claude(&mut self, kind: &str, v: &Value, out: &mut Vec<EventPayload>) {
        let text = |k: &str| v.get(k).and_then(Value::as_str);
        match kind {
            "system" if text("subtype") == Some("init") => {
                let Some(session) = text("session_id").filter(|s| !s.is_empty()) else {
                    return self.violation(out, "init without session_id");
                };
                if self.session_id.is_some() {
                    return self.violation(out, "duplicate init record");
                }
                self.session_id = Some(session.to_owned());
                let turn = self.next_turn();
                out.push(self.session(session, &turn));
                out.push(EventPayload::TurnStarted { turn: turn.clone() });
                self.open_turn = Some(turn);
            }
            "assistant" | "user" => {
                if self.session_id.is_none() {
                    return self.violation(out, "message before session init");
                }
                let Some(blocks) = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(Value::as_array)
                else {
                    return self.violation(out, "message without content blocks");
                };
                for block in blocks {
                    let field = |k: &str| block.get(k).and_then(Value::as_str);
                    match (kind, field("type")) {
                        ("assistant", Some("text")) => {
                            if let Some(t) = field("text") {
                                out.push(EventPayload::TextDelta(t.to_owned()));
                            }
                        }
                        ("assistant", Some("tool_use")) => match field("id") {
                            Some(id) => out.push(EventPayload::ToolStarted { id: id.to_owned() }),
                            None => self.violation(out, "tool_use without id"),
                        },
                        ("user", Some("tool_result")) => match field("tool_use_id") {
                            Some(id) => out.push(EventPayload::ToolFinished { id: id.to_owned() }),
                            None => self.violation(out, "tool_result without tool_use_id"),
                        },
                        _ => {}
                    }
                }
            }
            "result" => {
                let Some(subtype) = text("subtype") else {
                    return self.violation(out, "result without subtype");
                };
                let is_error = v.get("is_error").and_then(Value::as_bool);
                let status = match (subtype, is_error) {
                    ("success", Some(false)) => TerminalStatus::Success,
                    (_, Some(true)) => TerminalStatus::Error,
                    (s, _) if s.starts_with("error") => TerminalStatus::Error,
                    _ => TerminalStatus::Unknown,
                };
                Self::usage(out, v.get("usage"));
                let Some(turn) = self.open_turn.clone().filter(|_| self.session_id.is_some())
                else {
                    return self.violation(out, "result before session init");
                };
                self.terminal_record(out, turn, status, subtype);
            }
            other => {
                let sub = text("subtype").unwrap_or("");
                out.push(EventPayload::Informational(format!("{other}:{sub}")));
            }
        }
    }

    fn codex(&mut self, kind: &str, v: &Value, out: &mut Vec<EventPayload>) {
        let message = |v: &Value| {
            v.get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        match kind {
            "thread.started" => {
                let Some(id) = v
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                else {
                    return self.violation(out, "thread.started without thread_id");
                };
                if self.session_id.is_some() {
                    return self.violation(out, "duplicate thread.started");
                }
                self.session_id = Some(id.to_owned());
                let next = format!("turn-{}", self.turns + 1);
                out.push(self.session(id, &next));
            }
            "turn.started" => {
                if self.session_id.is_none() {
                    return self.violation(out, "turn before thread.started");
                }
                if self.open_turn.is_some() {
                    return self.violation(out, "turn.started while a turn is open");
                }
                let turn = self.next_turn();
                out.push(EventPayload::TurnStarted { turn: turn.clone() });
                self.open_turn = Some(turn);
            }
            "turn.completed" | "turn.failed" => {
                let Some(turn) = self.open_turn.take() else {
                    return self.violation(out, "turn end without turn.started");
                };
                if kind == "turn.completed" {
                    Self::usage(out, v.get("usage"));
                    let status = match v.get("status").and_then(Value::as_str) {
                        None | Some("success" | "completed") => TerminalStatus::Success,
                        Some("error" | "failed") => TerminalStatus::Error,
                        Some("interrupted" | "cancelled") => TerminalStatus::Interrupted,
                        Some(_) => TerminalStatus::Unknown,
                    };
                    self.terminal_record(out, turn, status, kind);
                } else {
                    let error = v.get("error").map(message).unwrap_or_default();
                    out.push(EventPayload::ProviderError(error));
                    self.terminal_record(out, turn, TerminalStatus::Error, kind);
                }
            }
            "item.started" | "item.completed" => {
                let Some(item) = v.get("item") else {
                    return self.violation(out, "item event without item");
                };
                let id = item.get("id").and_then(Value::as_str);
                let item_type = item.get("type").and_then(Value::as_str);
                let started = kind == "item.started";
                match (item_type, id) {
                    (Some("agent_message"), _) if !started => {
                        if let Some(t) = item.get("text").and_then(Value::as_str) {
                            out.push(EventPayload::TextDelta(t.to_owned()));
                        }
                    }
                    (Some("error"), _) if !started => {
                        out.push(EventPayload::ProviderError(message(item)));
                    }
                    (
                        Some("command_execution" | "file_change" | "mcp_tool_call" | "web_search"),
                        Some(id),
                    ) => out.push(if started {
                        EventPayload::ToolStarted { id: id.to_owned() }
                    } else {
                        EventPayload::ToolFinished { id: id.to_owned() }
                    }),
                    (Some(_), _) => out.push(EventPayload::Informational(format!(
                        "{kind}:{}",
                        item_type.unwrap_or("")
                    ))),
                    (None, _) => self.violation(out, "item without type"),
                }
            }
            "error" => out.push(EventPayload::ProviderError(message(v))),
            other => out.push(EventPayload::Informational(other.to_owned())),
        }
    }

    /// Production profile. Violation texts are fixed strings: provider-controlled bytes
    /// stay in the raw spool and are never copied into a protocol diagnosis.
    fn codex_exec(&mut self, kind: &str, v: &Value, out: &mut Vec<EventPayload>) {
        let Some(record) = v.as_object() else {
            return self.violation(out, "record is not an object");
        };
        if self.terminal.is_some() {
            return self.violation(out, "record after terminal record");
        }
        let identifier = |v: &Value, key: &str| {
            v.get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= MAX_IDENTIFIER_BYTES)
                .map(str::to_owned)
        };
        let message = |v: &Value| {
            v.get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        match kind {
            "thread.started" => {
                let Some(thread) = identifier(v, "thread_id") else {
                    return self.violation(out, "thread.started without a usable thread_id");
                };
                if self.session_id.is_some() {
                    return self.violation(out, "duplicate thread.started");
                }
                out.push(self.session(&thread, "turn-1"));
                self.session_id = Some(thread);
            }
            "turn.started" => {
                if self.session_id.is_none() {
                    return self.violation(out, "turn before thread.started");
                }
                // One `exec` is one turn. A second turn is a shape this profile never saw.
                if self.turns != 0 {
                    return self.violation(out, "more than one turn in one exec");
                }
                let turn = self.next_turn();
                out.push(EventPayload::TurnStarted { turn: turn.clone() });
                self.open_turn = Some(turn);
            }
            "turn.completed" => {
                let Some(turn) = self.open_turn.take() else {
                    return self.violation(out, "turn end without turn.started");
                };
                let usage = v.get("usage").filter(|u| u.is_object());
                let counted = ["input_tokens", "output_tokens"]
                    .iter()
                    .all(|k| usage.and_then(|u| u.get(k)).is_some_and(Value::is_u64));
                let modelled = record
                    .keys()
                    .all(|k| matches!(k.as_str(), "type" | "usage"));
                Self::usage(out, usage);
                // Success needs exactly the documented shape and an error-free turn.
                let status = if counted && modelled && !self.errored {
                    TerminalStatus::Success
                } else {
                    TerminalStatus::Unknown
                };
                self.terminal_record(out, turn, status, kind);
            }
            "turn.failed" => {
                let Some(turn) = self.open_turn.take() else {
                    return self.violation(out, "turn end without turn.started");
                };
                self.errored = true;
                let error = v.get("error").map(message).unwrap_or_default();
                out.push(EventPayload::ProviderError(error));
                self.terminal_record(out, turn, TerminalStatus::Error, kind);
            }
            "item.started" | "item.updated" | "item.completed" => {
                if self.open_turn.is_none() {
                    return self.violation(out, "item outside an open turn");
                }
                let Some(item) = v.get("item").filter(|i| i.is_object()) else {
                    return self.violation(out, "item event without item");
                };
                let Some(id) = identifier(item, "id") else {
                    return self.violation(out, "item without a usable id");
                };
                let Some(item_type) = item.get("type").and_then(Value::as_str) else {
                    return self.violation(out, "item without type");
                };
                let phase = &kind["item.".len()..];
                match (item_type, phase) {
                    ("agent_message", "completed") => {
                        match item.get("text").and_then(Value::as_str) {
                            Some(t) => out.push(EventPayload::TextDelta(t.to_owned())),
                            None => self.violation(out, "agent_message without text"),
                        }
                    }
                    ("command_execution" | "file_change", "started") => {
                        out.push(EventPayload::ToolStarted { id })
                    }
                    ("command_execution" | "file_change", "completed") => {
                        out.push(EventPayload::ToolFinished { id })
                    }
                    ("error", _) => {
                        self.errored = true;
                        out.push(EventPayload::ProviderError(message(item)));
                    }
                    (
                        "agent_message" | "reasoning" | "todo_list" | "command_execution"
                        | "file_change",
                        _,
                    ) => out.push(EventPayload::Informational(format!("{kind}:{item_type}"))),
                    // Agent tool network access is denied and no MCP server is approved.
                    ("mcp_tool_call" | "web_search", _) => {
                        self.violation(out, "tool outside the approved envelope")
                    }
                    _ => self.violation(out, "unknown item type"),
                }
            }
            "error" => {
                self.errored = true;
                out.push(EventPayload::ProviderError(message(v)));
            }
            _ => self.violation(out, "unknown record type"),
        }
    }
}
