//! R-04 step 1: the payload of a stored row (an event, an outbox effect, a provider event,
//! a process observation the core reads back) is a canonical JSON object with named
//! members and a version, written through [`Record`] and read through [`Fields`]. Nothing
//! is stored as `Debug` text, so a renamed field or variant cannot silently change what a
//! stored row means.
use crate::identity::{OpaqueId, Revision};
use crate::ipc::{encode, identity, number, text};
use crate::json::{self, Json};
use crate::provider::{EventPayload, ProviderKind, SessionRef, TerminalStatus};

/// The `v` member of every record.
pub const VERSION: i64 = 1;

/// Named members of one payload, in the order they were given; [`Record::encode`] sorts
/// them, so equal records encode to equal text.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Record(Vec<(&'static str, Json)>);

impl Record {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with(mut self, key: &'static str, value: Json) -> Self {
        self.0.push((key, value));
        self
    }
    pub fn text(self, key: &'static str, value: impl Into<String>) -> Self {
        self.with(key, text(value))
    }
    pub fn int(self, key: &'static str, value: i64) -> Self {
        self.with(key, Json::Int(value))
    }
    pub fn count(self, key: &'static str, value: u64) -> Self {
        self.with(key, number(value))
    }
    pub fn id(self, key: &'static str, id: OpaqueId) -> Self {
        self.with(key, identity(id))
    }
    pub fn optional(self, key: &'static str, value: Option<impl Into<String>>) -> Self {
        self.with(key, value.map_or(Json::Null, |v| text(v)))
    }
    pub fn encode(&self) -> String {
        let mut members = vec![("v".to_owned(), Json::Int(VERSION))];
        members.extend(self.0.iter().map(|(k, v)| ((*k).to_owned(), v.clone())));
        encode(&Json::Object(members))
    }
}

/// The members of a stored record, read back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fields(Vec<(String, Json)>);

/// Reads a stored payload. Anything but an object of the current version is refused.
pub fn read(payload: &str) -> Result<Fields, String> {
    let Json::Object(members) = json::parse(payload.as_bytes()).map_err(|e| format!("{e:?}"))?
    else {
        return Err("payload is not an object".into());
    };
    let fields = Fields(members);
    match fields.int("v") {
        Some(VERSION) => Ok(fields),
        _ => Err("payload has no known version".into()),
    }
}

impl Fields {
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Json::Text(s)) => Some(s),
            _ => None,
        }
    }
    pub fn int(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(Json::Int(n)) => Some(*n),
            _ => None,
        }
    }
    pub fn count(&self, key: &str) -> Option<u64> {
        self.int(key).and_then(|n| u64::try_from(n).ok())
    }
    /// Every text member at the top level, in key order.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|(_, v)| match v {
            Json::Text(s) => Some(s.as_str()),
            _ => None,
        })
    }
    /// A member written as an optional integer: `Null` is `Some(None)`.
    pub fn optional_int(&self, key: &str) -> Option<Option<i64>> {
        match self.get(key) {
            Some(Json::Null) => Some(None),
            Some(Json::Int(n)) => Some(Some(*n)),
            _ => None,
        }
    }
}

/// Reads one member of a payload, or "" when the payload or the member is missing. For
/// sentences and labels only; a decision reads through [`read`] and refuses what it lacks.
pub fn member(payload: &str, key: &str) -> String {
    read(payload)
        .ok()
        .and_then(|f| f.text(key).map(str::to_owned))
        .unwrap_or_default()
}

/// A SQL `LIKE` pattern that matches a stored record whose text member `key` is exactly
/// `identity`. Sound because records are canonical and an identity is 32 lowercase hex
/// characters: a quote inside any text member is escaped, so the pattern cannot match
/// inside another member's value.
pub fn identity_pattern(key: &str, identity: &str) -> String {
    format!("%\"{key}\":\"{identity}\"%")
}

pub fn provider_word(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::ClaudeCode => "ClaudeCode",
        ProviderKind::Codex => "Codex",
    }
}

fn provider_from(word: &str) -> Result<ProviderKind, String> {
    match word {
        "ClaudeCode" => Ok(ProviderKind::ClaudeCode),
        "Codex" => Ok(ProviderKind::Codex),
        _ => Err(format!("unknown provider {word:?}")),
    }
}

pub fn terminal_word(status: TerminalStatus) -> &'static str {
    match status {
        TerminalStatus::Success => "Success",
        TerminalStatus::Error => "Error",
        TerminalStatus::Interrupted => "Interrupted",
        TerminalStatus::Unknown => "Unknown",
    }
}

/// The status of a stored `ProviderTerminal`; anything else, or anything unreadable, is
/// `Unknown`, which can never produce success.
pub fn terminal_from(word: &str) -> TerminalStatus {
    match word {
        "Success" => TerminalStatus::Success,
        "Error" => TerminalStatus::Error,
        "Interrupted" => TerminalStatus::Interrupted,
        _ => TerminalStatus::Unknown,
    }
}

/// A provider event as stored in `provider_events.payload`. The variant is the `event`
/// member; it repeats the row's `kind` so that the payload stands on its own.
pub fn provider_event(payload: &EventPayload) -> Record {
    let event = |name: &'static str| Record::new().text("event", name);
    match payload {
        EventPayload::ProcessStarted { process } => {
            event("ProcessStarted").text("process", process.clone())
        }
        EventPayload::SessionIdentified(s) => event("SessionIdentified")
            .text("provider", provider_word(s.provider))
            .text("session", s.session.clone())
            .text("turn", s.turn.clone()),
        EventPayload::TurnStarted { turn } => event("TurnStarted").text("turn", turn.clone()),
        EventPayload::TextDelta(t) => event("TextDelta").text("text", t.clone()),
        EventPayload::ToolStarted { id } => event("ToolStarted").text("id", id.clone()),
        EventPayload::ToolFinished { id } => event("ToolFinished").text("id", id.clone()),
        EventPayload::QuestionRequested { id, revision } => event("QuestionRequested")
            .text("id", id.clone())
            .count("revision", revision.0),
        EventPayload::PermissionRequested {
            id,
            revision,
            deadline_millis,
        } => event("PermissionRequested")
            .text("id", id.clone())
            .count("revision", revision.0)
            .int("deadline_millis", *deadline_millis),
        EventPayload::UsageObserved { units } => event("UsageObserved").count("units", *units),
        EventPayload::ProviderTerminal {
            turn,
            status,
            raw_status,
        } => event("ProviderTerminal")
            .text("turn", turn.clone())
            .text("status", terminal_word(*status))
            .text("raw_status", raw_status.clone()),
        EventPayload::ProviderError(t) => event("ProviderError").text("text", t.clone()),
        EventPayload::InterruptAcknowledged => event("InterruptAcknowledged"),
        EventPayload::ProcessExited { code } => event("ProcessExited")
            .with("code", code.map_or(Json::Null, |c| Json::Int(i64::from(c)))),
        EventPayload::StreamTruncated => event("StreamTruncated"),
        EventPayload::ProtocolViolation(t) => event("ProtocolViolation").text("text", t.clone()),
        EventPayload::Informational(t) => event("Informational").text("text", t.clone()),
    }
}

/// Reads a stored provider event back; the inverse of [`provider_event`].
pub fn provider_event_from(payload: &str) -> Result<EventPayload, String> {
    let f = read(payload)?;
    let t = |key: &str| {
        f.text(key)
            .map(str::to_owned)
            .ok_or_else(|| format!("missing {key}"))
    };
    let n = |key: &str| f.count(key).ok_or_else(|| format!("missing {key}"));
    Ok(match f.text("event").unwrap_or("") {
        "ProcessStarted" => EventPayload::ProcessStarted {
            process: t("process")?,
        },
        "SessionIdentified" => EventPayload::SessionIdentified(SessionRef {
            provider: provider_from(&t("provider")?)?,
            session: t("session")?,
            turn: t("turn")?,
        }),
        "TurnStarted" => EventPayload::TurnStarted { turn: t("turn")? },
        "TextDelta" => EventPayload::TextDelta(t("text")?),
        "ToolStarted" => EventPayload::ToolStarted { id: t("id")? },
        "ToolFinished" => EventPayload::ToolFinished { id: t("id")? },
        "QuestionRequested" => EventPayload::QuestionRequested {
            id: t("id")?,
            revision: Revision(n("revision")?),
        },
        "PermissionRequested" => EventPayload::PermissionRequested {
            id: t("id")?,
            revision: Revision(n("revision")?),
            deadline_millis: f.int("deadline_millis").ok_or("missing deadline_millis")?,
        },
        "UsageObserved" => EventPayload::UsageObserved { units: n("units")? },
        "ProviderTerminal" => EventPayload::ProviderTerminal {
            turn: t("turn")?,
            status: terminal_from(&t("status")?),
            raw_status: t("raw_status")?,
        },
        "ProviderError" => EventPayload::ProviderError(t("text")?),
        "InterruptAcknowledged" => EventPayload::InterruptAcknowledged,
        "ProcessExited" => EventPayload::ProcessExited {
            code: match f.optional_int("code").ok_or("missing code")? {
                None => None,
                Some(c) => Some(i32::try_from(c).map_err(|_| "code out of range")?),
            },
        },
        "StreamTruncated" => EventPayload::StreamTruncated,
        "ProtocolViolation" => EventPayload::ProtocolViolation(t("text")?),
        "Informational" => EventPayload::Informational(t("text")?),
        other => return Err(format!("unknown provider event {other:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_encode_canonically_and_read_back() {
        let a = Record::new().text("b", "x").int("a", 7).encode();
        let b = Record::new().int("a", 7).text("b", "x").encode();
        assert_eq!(a, b);
        assert_eq!(a, r#"{"a":7,"b":"x","v":1}"#);
        let f = read(&a).unwrap();
        assert_eq!((f.int("a"), f.text("b")), (Some(7), Some("x")));
        assert_eq!(member(&a, "b"), "x");
        assert_eq!(member("a|b", "b"), "");
    }

    #[test]
    fn unversioned_or_debug_text_is_refused() {
        assert!(read(r#"{"a":1}"#).is_err());
        assert!(read(r#"{"v":2}"#).is_err());
        assert!(read("SessionIdentified(SessionRef { provider: Codex })").is_err());
        assert!(read("[1]").is_err());
    }

    #[test]
    fn every_provider_event_round_trips() {
        let events = [
            EventPayload::ProcessStarted {
                process: "p".into(),
            },
            EventPayload::SessionIdentified(SessionRef {
                provider: ProviderKind::Codex,
                session: "s\"1".into(),
                turn: "t".into(),
            }),
            EventPayload::TurnStarted { turn: "t".into() },
            EventPayload::TextDelta("hello | world".into()),
            EventPayload::ToolStarted { id: "a".into() },
            EventPayload::ToolFinished { id: "a".into() },
            EventPayload::QuestionRequested {
                id: "q".into(),
                revision: Revision(3),
            },
            EventPayload::PermissionRequested {
                id: "q".into(),
                revision: Revision(4),
                deadline_millis: -5,
            },
            EventPayload::UsageObserved { units: 9 },
            EventPayload::ProviderTerminal {
                turn: "t".into(),
                status: TerminalStatus::Interrupted,
                raw_status: "x".into(),
            },
            EventPayload::ProviderError("e".into()),
            EventPayload::InterruptAcknowledged,
            EventPayload::ProcessExited { code: None },
            EventPayload::ProcessExited { code: Some(-1) },
            EventPayload::StreamTruncated,
            EventPayload::ProtocolViolation("v".into()),
            EventPayload::Informational("i".into()),
        ];
        for e in events {
            let stored = provider_event(&e).encode();
            assert_eq!(provider_event_from(&stored), Ok(e), "{stored}");
        }
        assert!(provider_event_from(r#"{"event":"Later","v":1}"#).is_err());
        assert!(provider_event_from(r#"{"event":"TextDelta","v":1}"#).is_err());
    }

    #[test]
    fn a_terminal_status_outside_the_four_is_unknown() {
        assert_eq!(terminal_from("Successful"), TerminalStatus::Unknown);
        for s in [
            TerminalStatus::Success,
            TerminalStatus::Error,
            TerminalStatus::Interrupted,
            TerminalStatus::Unknown,
        ] {
            assert_eq!(terminal_from(terminal_word(s)), s);
        }
    }
}
