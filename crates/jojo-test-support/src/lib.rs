//! Deterministic test fixtures only. Never linked into the production helper.
//! Synthetic provider adapters have no executable, auth or process launcher.

pub mod dependency;

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use jojo_domain::identity::{IdGenerationError, IdGenerator, OpaqueId};
use jojo_domain::time::{Clock, ClockReading};

pub struct FakeClock(Mutex<ClockReading>);

impl FakeClock {
    pub fn new(reading: ClockReading) -> Self {
        Self(Mutex::new(reading))
    }

    /// Explicitly replace wall/monotonic readings; tests need no sleep or system clock.
    pub fn set(&self, reading: ClockReading) {
        *self.0.lock().expect("test clock lock poisoned") = reading;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> ClockReading {
        *self.0.lock().expect("test clock lock poisoned")
    }
}

/// Test-only, caller-scoped namespace and sequence. Not a production/restart-safe ID source.
pub struct SequenceIds {
    namespace: u64,
    next: AtomicU64,
}

impl SequenceIds {
    pub fn new(namespace: u64, first: u64) -> Self {
        Self {
            namespace,
            next: AtomicU64::new(first),
        }
    }
}

impl IdGenerator for SequenceIds {
    fn next_id(&self) -> Result<OpaqueId, IdGenerationError> {
        self.next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map(|n| OpaqueId((u128::from(self.namespace) << 64) | u128::from(n)))
            .map_err(|_| IdGenerationError::Exhausted)
    }
}

/// A structurally complete, human-authored PRD in the typed Jojo schema (version 1).
/// Written for these tests; no upstream template text was adapted.
pub const COMPLETE_PRD_JSON: &str = r#"{
  "schema_version": 1,
  "title": "Reading list",
  "goal": "Let one person keep a private list of articles to read later.",
  "non_goals": ["Sharing lists with other people."],
  "actors": [{"id": "ACT-READER", "name": "Reader"}],
  "use_cases": [
    {
      "id": "UC-1",
      "title": "Save an article",
      "actor_ids": ["ACT-READER"],
      "preconditions": ["The reader has the application open."],
      "main_flow": [
        "The reader enters an article address.",
        "The system stores the article at the end of the list.",
        "The system shows the updated list."
      ],
      "alternate_flows": [
        {
          "id": "UC-1-A1",
          "title": "Article already saved",
          "steps": ["The system shows the existing entry and stores nothing."]
        }
      ],
      "error_flows": [
        {
          "id": "UC-1-E1",
          "title": "Address is not valid",
          "steps": ["The system rejects the address and keeps the list unchanged."]
        }
      ],
      "postconditions": ["The article appears exactly once in the list."],
      "acceptance_ids": ["AC-1", "AC-2"]
    }
  ],
  "acceptance_criteria": [
    {"id": "AC-1", "statement": "A saved article is listed after restarting the application."},
    {"id": "AC-2", "statement": "Saving the same address twice leaves one entry."}
  ],
  "nfrs": [{"id": "NFR-1", "statement": "All data stays on the local machine."}],
  "assumptions": [{"id": "ASM-1", "statement": "The list holds fewer than ten thousand entries."}],
  "blockers": []
}"#;

pub mod provider;
