//! Bounded, strict JSON reader for manually imported typed content.
//!
//! Stricter than RFC 8259 on purpose: input that could be read in two ways is refused, not
//! resolved. Duplicate object keys (compared after unescaping), non-integer numbers, NUL,
//! unpaired surrogates, a byte-order mark and trailing content are all errors, and size
//! and nesting are bounded before anything is allocated for them.

/// Largest accepted document, in bytes.
pub const MAX_INPUT_BYTES: usize = 256 * 1024;
/// Deepest accepted nesting of arrays/objects. The top-level value is depth 1.
pub const MAX_DEPTH: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    List(Vec<Json>),
    /// Members in input order; keys are unique.
    Object(Vec<(String, Json)>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JsonError {
    TooLarge {
        bytes: usize,
        limit: usize,
    },
    TooDeep {
        path: String,
        limit: usize,
    },
    /// `path` names the object that contains `key` more than once.
    DuplicateKey {
        path: String,
        key: String,
    },
    NotUtf8,
    /// Fractions and exponents have no canonical typed form here; only `i64` integers.
    UnsupportedNumber {
        offset: usize,
    },
    Syntax {
        offset: usize,
        message: &'static str,
    },
}

/// Parses exactly one JSON value spanning the whole input.
pub fn parse(input: &[u8]) -> Result<Json, JsonError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(JsonError::TooLarge {
            bytes: input.len(),
            limit: MAX_INPUT_BYTES,
        });
    }
    let text = std::str::from_utf8(input).map_err(|_| JsonError::NotUtf8)?;
    validate_numbers(text)?;
    let mut error = None;
    let mut decoder = serde_json::Deserializer::from_str(text);
    use serde::de::DeserializeSeed;
    let result = Seed {
        depth: 1,
        path: String::new(),
        error: &mut error,
    }
    .deserialize(&mut decoder)
    .and_then(|value| {
        decoder.end()?;
        Ok(value)
    });
    result.map_err(|failure| {
        error.unwrap_or_else(|| JsonError::Syntax {
            offset: text
                .split_inclusive('\n')
                .take(failure.line().saturating_sub(1))
                .map(str::len)
                .sum::<usize>()
                + failure.column().saturating_sub(1),
            message: "invalid JSON",
        })
    })
}

/// `parent.key`, or `key` at the document root.
pub fn member_path(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        key.to_string()
    } else {
        format!("{parent}.{key}")
    }
}

/// `parent[index]`.
pub fn element_path(parent: &str, index: usize) -> String {
    format!("{parent}[{index}]")
}

/// Appends `text` as one JSON string with a single, deterministic escaping.
pub fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

// Serde handles JSON grammar and Unicode. This lexical number policy preserves the
// integer-only contract, including -0, without allowing floating-point rounding.
fn validate_numbers(text: &str) -> Result<(), JsonError> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'"' {
            at += 1;
            while at < bytes.len() && bytes[at] != b'"' {
                if bytes[at] == b'\\' {
                    at += 1;
                }
                at += 1;
            }
            at += 1;
        } else if matches!(bytes[at], b'-' | b'0'..=b'9') {
            let start = at;
            if bytes[at] == b'-' {
                at += 1;
            }
            let digits = at;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            if at == digits || (bytes[digits] == b'0' && at - digits > 1) {
                return Err(JsonError::Syntax {
                    offset: digits,
                    message: "invalid integer",
                });
            }
            if matches!(bytes.get(at), Some(b'.' | b'e' | b'E'))
                || text[start..at].parse::<i64>().is_err()
            {
                return Err(JsonError::UnsupportedNumber { offset: start });
            }
        } else {
            at += 1;
        }
    }
    Ok(())
}

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
struct Seed<'a> {
    depth: usize,
    path: String,
    error: &'a mut Option<JsonError>,
}
impl Seed<'_> {
    fn fail<E: de::Error>(&mut self, error: JsonError) -> E {
        *self.error = Some(error);
        E::custom("strict JSON policy")
    }
    fn container<E: de::Error>(&mut self) -> Result<(), E> {
        if self.depth > MAX_DEPTH {
            return Err(self.fail(JsonError::TooDeep {
                path: self.path.clone(),
                limit: MAX_DEPTH,
            }));
        }
        Ok(())
    }
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Json;
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Json;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("strict JSON")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Json, E> {
        Ok(Json::Int(value))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Json, E> {
        i64::try_from(value).map(Json::Int).map_err(E::custom)
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Json, E> {
        // The lexical policy has already excluded every float spelling; serde
        // represents the valid integer spelling -0 as negative floating zero.
        if value == 0.0 && value.is_sign_negative() {
            Ok(Json::Int(0))
        } else {
            Err(E::custom("non-integer"))
        }
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Json, E> {
        self.visit_string(value.to_owned())
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Json, E> {
        if value.contains('\0') {
            return Err(E::custom("NUL is not accepted in text"));
        }
        Ok(Json::Text(value))
    }
    fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> Result<Json, A::Error> {
        self.container()?;
        let mut items = Vec::new();
        while let Some(value) = seq.next_element_seed(Seed {
            depth: self.depth + 1,
            path: element_path(&self.path, items.len()),
            error: self.error,
        })? {
            items.push(value);
        }
        Ok(Json::List(items))
    }
    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<Json, A::Error> {
        self.container()?;
        let mut members = Vec::new();
        let mut keys = std::collections::BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if key.contains('\0') {
                return Err(de::Error::custom("NUL is not accepted in text"));
            }
            if !keys.insert(key.clone()) {
                return Err(self.fail(JsonError::DuplicateKey {
                    path: self.path.clone(),
                    key,
                }));
            }
            let value = map.next_value_seed(Seed {
                depth: self.depth + 1,
                path: member_path(&self.path, &key),
                error: self.error,
            })?;
            members.push((key, value));
        }
        Ok(Json::Object(members))
    }
}
