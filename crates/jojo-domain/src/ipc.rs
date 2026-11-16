//! P1-13 bounded JSONL transport vocabulary. No I/O or mutation authority.
use crate::identity::OpaqueId;
use crate::json::{self, Json};

pub const MAJOR: i64 = 1;
/// 1.1 (P2-02) adds repository registration, bundle import, Start and recovery
/// resolution. A 1.0 client is unaffected: the additions answer only to minor >= 1.
/// 1.2 (M2) adds Feature Chat: lead agent turns, answers to their questions and the chat
/// queries. A 1.1 client is unaffected: the additions answer only to minor >= 2.
pub const MINOR: i64 = 2;
pub const MAX_FRAME: usize = 64 * 1024;
pub const MAX_PAGE: u32 = 8;

pub fn object(fields: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
    Json::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
pub fn text(s: impl Into<String>) -> Json {
    Json::Text(s.into())
}
pub fn number(n: u64) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}
pub fn get<'a>(value: &'a Json, key: &str) -> Result<&'a Json, String> {
    match value {
        Json::Object(fields) => fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .ok_or_else(|| format!("missing {key}")),
        _ => Err("expected object".into()),
    }
}
pub fn string(value: &Json) -> Result<&str, String> {
    if let Json::Text(s) = value {
        Ok(s)
    } else {
        Err("expected text".into())
    }
}
pub fn uint(value: &Json) -> Result<u64, String> {
    if let Json::Int(n) = value {
        u64::try_from(*n).map_err(|_| "expected nonnegative integer".into())
    } else {
        Err("expected integer".into())
    }
}
pub fn id(value: &Json) -> Result<OpaqueId, String> {
    let s = string(value)?;
    if s.len() != 32
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("expected 32 lowercase hex identity".into());
    }
    u128::from_str_radix(s, 16)
        .map(OpaqueId)
        .map_err(|_| "invalid identity".into())
}
pub fn identity(id: OpaqueId) -> Json {
    text(format!("{:032x}", id.0))
}
pub fn encode(value: &Json) -> String {
    fn write(v: &Json, out: &mut String) {
        match v {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => out.push_str(&n.to_string()),
            Json::Text(s) => json::write_string(out, s),
            Json::List(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(v, out);
                }
                out.push(']');
            }
            Json::Object(fields) => {
                out.push('{');
                let mut ordered: Vec<_> = fields.iter().collect();
                ordered.sort_by(|a, b| a.0.cmp(&b.0));
                for (i, (k, v)) in ordered.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    json::write_string(out, k);
                    out.push(':');
                    write(v, out);
                }
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out
}

#[derive(Clone, Debug)]
pub struct Envelope {
    pub major: u64,
    pub minor: u64,
    pub session: OpaqueId,
    pub epoch: u64,
    pub request: OpaqueId,
    pub revision: Option<u64>,
    pub kind: String,
    pub payload: Json,
}
impl Envelope {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_FRAME {
            return Err("frame too large".into());
        }
        let v = json::parse(bytes).map_err(|e| format!("invalid JSON: {e:?}"))?;
        if !matches!(&v, Json::Object(f) if f.len()==8) {
            return Err("envelope requires exactly eight fields".into());
        }
        let revision = match get(&v, "revision")? {
            Json::Null => None,
            n => Some(uint(n)?),
        };
        let kind = string(get(&v, "type")?)?.to_owned();
        if kind.len() > 64 {
            return Err("type too long".into());
        }
        if !matches!(get(&v, "payload")?, Json::Object(_)) {
            return Err("payload must be an object".into());
        }
        Ok(Self {
            major: uint(get(&v, "major")?)?,
            minor: uint(get(&v, "minor")?)?,
            session: id(get(&v, "session")?)?,
            epoch: uint(get(&v, "epoch")?)?,
            request: id(get(&v, "id")?)?,
            revision,
            kind,
            payload: get(&v, "payload")?.clone(),
        })
    }
    pub fn json(&self) -> Json {
        object([
            ("major", number(self.major)),
            ("minor", number(self.minor)),
            ("session", identity(self.session)),
            ("epoch", number(self.epoch)),
            ("id", identity(self.request)),
            ("revision", self.revision.map_or(Json::Null, number)),
            ("type", text(&self.kind)),
            ("payload", self.payload.clone()),
        ])
    }
}

/// Fragmented input is retained up to one frame; oversize/truncated frames are fatal.
#[derive(Default)]
pub struct Decoder {
    pending: Vec<u8>,
}
impl Decoder {
    pub fn push(&mut self, byte: u8) -> Result<Option<Envelope>, String> {
        if byte == b'\n' {
            let bytes = std::mem::take(&mut self.pending);
            return Envelope::parse(&bytes).map(Some);
        }
        if self.pending.len() == MAX_FRAME {
            return Err("frame too large".into());
        }
        self.pending.push(byte);
        Ok(None)
    }
    pub fn finish(&self) -> Result<(), String> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err("truncated frame".into())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Projection {
    Apply,
    Duplicate,
    Reload,
}
/// UI-independent projection rule. A gap invalidates the projection until a new snapshot.
#[derive(Default)]
pub struct Cursor {
    pub epoch: u64,
    pub sequence: u64,
    pub valid: bool,
}
impl Cursor {
    pub fn snapshot(&mut self, epoch: u64, sequence: u64) {
        self.epoch = epoch;
        self.sequence = sequence;
        self.valid = true;
    }
    pub fn event(&mut self, epoch: u64, seq: u64) -> Projection {
        if !self.valid || epoch != self.epoch || seq > self.sequence.saturating_add(1) {
            self.valid = false;
            return Projection::Reload;
        }
        if seq <= self.sequence {
            return Projection::Duplicate;
        }
        self.sequence = seq;
        Projection::Apply
    }
}

/// Receiving EOF or an exit status alone never proves a graceful close. The native
/// lifecycle shell consumes this transport result in P1-14; no OS quit behavior here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Closure {
    Unknown,
    ReadyAwaitingExit,
    Clean,
    Failed,
}
pub struct ClientClose {
    session: OpaqueId,
    epoch: u64,
    ticket: OpaqueId,
    ready: bool,
    failed: bool,
    exit: Option<i32>,
}
impl ClientClose {
    pub fn new(session: OpaqueId, epoch: u64, ticket: OpaqueId) -> Self {
        Self {
            session,
            epoch,
            ticket,
            ready: false,
            failed: false,
            exit: None,
        }
    }
    pub fn response(&mut self, e: &Envelope) -> Result<(), String> {
        if e.major != MAJOR as u64 || e.session != self.session || e.epoch != self.epoch {
            return Err("response identity/version mismatch".into());
        }
        match e.kind.as_str() {
            "ShutdownFailure" => self.failed = true,
            "ShutdownReady" => {
                let p = &e.payload;
                if id(get(p, "ticket")?)? != self.ticket
                    || id(get(p, "shutdown")?)?.0 == 0
                    || uint(get(p, "event_seq")?)? == 0
                    || string(get(p, "outcome")?)? != "Ready"
                    || get(p, "process_stop_confirmed")? != &Json::Bool(true)
                    || get(p, "persistence_sealed")? != &Json::Bool(true)
                    || !matches!(get(p,"failures")?,Json::List(v) if v.is_empty())
                {
                    return Err("invalid shutdown proof".into());
                }
                self.ready = true;
            }
            _ => {}
        }
        Ok(())
    }
    pub fn exited(&mut self, status: i32) {
        self.exit = Some(status);
    }
    pub fn state(&self) -> Closure {
        if self.failed || self.exit.is_some_and(|s| s != 0) {
            Closure::Failed
        } else if self.ready && self.exit == Some(0) {
            Closure::Clean
        } else if self.ready {
            Closure::ReadyAwaitingExit
        } else {
            Closure::Unknown
        }
    }
}
