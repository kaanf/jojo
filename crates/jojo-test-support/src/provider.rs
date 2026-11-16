//! Original synthetic fixtures, not a claim of official CLI protocol compatibility.
//! Each prepared session owns its script/cursor. No shared mutable adapter state or I/O.
use jojo_domain::provider::*;
use jojo_domain::time::ClockReading;
use std::collections::VecDeque;

#[derive(Clone)]
pub struct FakeAdapter {
    fingerprint: ProviderFingerprint,
    capabilities: Capabilities,
    auth: AuthMode,
    readiness: Readiness,
}
pub struct FakeClaude(pub FakeAdapter);
pub struct FakeCodex(pub FakeAdapter);
impl FakeAdapter {
    pub fn new(
        fingerprint: ProviderFingerprint,
        capabilities: Capabilities,
        auth: AuthMode,
        readiness: Readiness,
    ) -> Self {
        Self {
            fingerprint,
            capabilities,
            auth,
            readiness,
        }
    }
}
macro_rules! adapter {
    ($name:ident, $kind:expr) => {
        impl ProviderAdapter for $name {
            type Prepared = FakeSession;
            fn capabilities(&self) -> &Capabilities {
                &self.0.capabilities
            }
            fn prepare(&self, spec: LaunchSpec) -> Result<FakeSession, ProviderError> {
                if spec.fingerprint.provider != $kind {
                    return Err(ProviderError::FingerprintMismatch);
                }
                self.0.prepare(spec)
            }
        }
    };
}
adapter!(FakeClaude, ProviderKind::ClaudeCode);
adapter!(FakeCodex, ProviderKind::Codex);
impl ProviderAdapter for FakeAdapter {
    type Prepared = FakeSession;
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn prepare(&self, spec: LaunchSpec) -> Result<FakeSession, ProviderError> {
        if spec.fingerprint != self.fingerprint || self.fingerprint.schema != PROVIDER_SCHEMA {
            return Err(ProviderError::FingerprintMismatch);
        }
        if self.auth != AuthMode::NativeSubscription
            || self.capabilities.auth_status != Support::Supported
        {
            return Err(ProviderError::AuthenticationDenied);
        }
        if self.readiness != Readiness::Ready {
            return Err(ProviderError::NotReady);
        }
        spec.validate()?;
        if self.capabilities.evidence.trim().is_empty()
            || self.capabilities.structured_events != Support::Supported
            || self.capabilities.explicit_sessions != Support::Supported
            || self.capabilities.workspace_policy != Support::Supported
        {
            return Err(ProviderError::NotReady);
        }
        if let Some(c) = &spec.continuation
            && (self.capabilities.exact_continuation != Support::Supported
                || (c.previous_cwd != spec.cwd
                    && self.capabilities.continuation_cwd_change != Support::Supported))
        {
            return Err(ProviderError::UnsupportedContinuation);
        }
        let session = spec.continuation.as_ref().map(|c| c.exact.clone());
        Ok(FakeSession {
            spec,
            capabilities: self.capabilities.clone(),
            session,
            script: VecDeque::new(),
            sequence: 0,
            cancelled: false,
            terminal: false,
            pending: None,
            failed: false,
        })
    }
}
#[derive(Clone, Debug)]
pub struct FixtureRecord {
    pub raw: RawReference,
    pub observed: ClockReading,
    pub payload: EventPayload,
}
pub enum FixtureAction {
    Record(FixtureRecord),
    Wait(String),
    Fail(String),
}
#[derive(Debug, Eq, PartialEq)]
pub enum Poll {
    Event(NormalizedEvent),
    Waiting(String),
    Exhausted,
}
pub struct FakeSession {
    spec: LaunchSpec,
    capabilities: Capabilities,
    session: Option<SessionRef>,
    script: VecDeque<FixtureAction>,
    sequence: u64,
    cancelled: bool,
    terminal: bool,
    pending: Option<EventPayload>,
    failed: bool,
}
impl FakeSession {
    pub fn spec(&self) -> &LaunchSpec {
        &self.spec
    }
    pub fn session(&self) -> Option<&SessionRef> {
        self.session.as_ref()
    }
    pub fn enqueue(&mut self, action: FixtureAction) {
        self.script.push_back(action);
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    pub fn release(&mut self, barrier: &str) -> Result<(), ProviderError> {
        if !matches!(self.script.front(), Some(FixtureAction::Wait(id)) if id == barrier) {
            return Err(ProviderError::Correlation);
        }
        self.script.pop_front();
        Ok(())
    }
    /// Only captured synthetic records enter normalization. P1-09 owns real byte spooling.
    pub fn poll(&mut self) -> Result<Poll, ProviderError> {
        if self.failed {
            return Err(ProviderError::Protocol);
        }
        let result = self.poll_record();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn poll_record(&mut self) -> Result<Poll, ProviderError> {
        if self.cancelled {
            return Err(ProviderError::Cancelled);
        }
        if let Some(FixtureAction::Wait(id)) = self.script.front() {
            return Ok(Poll::Waiting(id.clone()));
        }
        let Some(action) = self.script.pop_front() else {
            return Ok(Poll::Exhausted);
        };
        let record = match action {
            FixtureAction::Record(r) => r,
            FixtureAction::Fail(_) => return Err(ProviderError::Protocol),
            FixtureAction::Wait(_) => unreachable!(),
        };
        if record.raw.blob.is_empty()
            || record.raw.end <= record.raw.start
            || record.raw.end > self.spec.max_output_bytes
            || record.raw.end - record.raw.start > self.spec.max_frame_bytes
        {
            return Err(ProviderError::Limit);
        }
        match &record.payload {
            // These are OS observations; adapter messages cannot prove process identity/death.
            EventPayload::ProcessStarted { .. } | EventPayload::ProcessExited { .. } => {
                return Err(ProviderError::Protocol);
            }
            EventPayload::SessionIdentified(s) => {
                if s.provider != self.spec.fingerprint.provider
                    || s.session.is_empty()
                    || s.turn.is_empty()
                    || self
                        .session
                        .as_ref()
                        .is_some_and(|old| old.session != s.session)
                {
                    return Err(ProviderError::Correlation);
                }
                self.session = Some(s.clone());
            }
            EventPayload::TurnStarted { turn } => {
                let s = self.session.as_mut().ok_or(ProviderError::Correlation)?;
                if turn.is_empty() {
                    return Err(ProviderError::Correlation);
                }
                s.turn = turn.clone();
            }
            EventPayload::ProviderTerminal { turn, .. } => {
                if self.session.as_ref().is_none_or(|s| &s.turn != turn) || self.terminal {
                    return Err(ProviderError::Correlation);
                }
                self.terminal = true;
            }
            EventPayload::QuestionRequested { id, revision }
            | EventPayload::PermissionRequested { id, revision, .. } => {
                if self.session.is_none()
                    || id.is_empty()
                    || revision.0 == 0
                    || self.pending.is_some()
                    || self.terminal
                {
                    return Err(ProviderError::Correlation);
                }
                self.pending = Some(record.payload.clone());
            }
            _ => {}
        }
        self.sequence = self.sequence.checked_add(1).ok_or(ProviderError::Limit)?;
        Ok(Poll::Event(NormalizedEvent {
            attempt: self.spec.attempt,
            process_generation: self.spec.dispatch_generation,
            adapter_version: self.spec.fingerprint.parser.clone(),
            schema: PROVIDER_SCHEMA,
            sequence: self.sequence,
            raw: record.raw,
            observed: record.observed,
            payload: record.payload,
        }))
    }
    fn correlate(&self, target: &InputTarget) -> Result<(), ProviderError> {
        if self.failed
            || self.cancelled
            || self.terminal
            || target.attempt != self.spec.attempt
            || target.generation != self.spec.dispatch_generation
            || self.session.as_ref() != Some(&target.session)
        {
            return Err(ProviderError::Correlation);
        }
        let matches = match &self.pending {
            Some(EventPayload::QuestionRequested { id, revision })
            | Some(EventPayload::PermissionRequested { id, revision, .. }) => {
                id == &target.request && revision == &target.expected_revision
            }
            _ => false,
        };
        if !matches {
            return Err(ProviderError::Correlation);
        }
        Ok(())
    }
    pub fn supply_input(
        &mut self,
        target: &InputTarget,
        bytes: &[u8],
    ) -> Result<(), ProviderError> {
        if self.capabilities.incremental_input != Support::Supported
            || self.capabilities.question_ids != Support::Supported
        {
            return Err(ProviderError::UnsupportedInput);
        }
        self.correlate(target)?;
        if !matches!(self.pending, Some(EventPayload::QuestionRequested { .. })) {
            return Err(ProviderError::Correlation);
        }
        if bytes.is_empty() || bytes.len() > MAX_PROMPT_BYTES {
            return Err(ProviderError::Limit);
        }
        self.pending = None;
        Ok(())
    }
    pub fn answer_permission(
        &mut self,
        answer: &ToolPermissionAnswer,
    ) -> Result<(), ProviderError> {
        if self.capabilities.permission_response != Support::Supported {
            return Err(ProviderError::UnsupportedPermission);
        }
        self.correlate(&answer.target)?;
        match self.pending {
            Some(EventPayload::PermissionRequested {
                deadline_millis, ..
            }) if answer.observed_millis <= deadline_millis => {}
            _ => return Err(ProviderError::Correlation),
        }
        self.pending = None;
        Ok(())
    }
    /// A request acknowledgement only; no process exit, result or claim mutation.
    pub fn request_interrupt(
        &self,
        attempt: jojo_domain::identity::AttemptId,
        generation: u64,
    ) -> Result<(), ProviderError> {
        if self.capabilities.turn_interrupt != Support::Supported {
            return Err(ProviderError::UnsupportedInterrupt);
        }
        if self.failed
            || self.cancelled
            || attempt != self.spec.attempt
            || generation != self.spec.dispatch_generation
        {
            return Err(ProviderError::Correlation);
        }
        Ok(())
    }
}
