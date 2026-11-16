# P1-13 stdio implementation

Implements the Phase 1 brief P1-13 and SHUTDOWN_RECOVERY sections 1–2; these remain normative. No P1-14 UI or AppKit termination flow is implemented.

Start `jojo-helper --serve` with explicit roots or the accepted private-root defaults. The existing OS ownership lock rejects a replacement with exit 3. Startup leaves admission closed, inspects recovery without adopting processes, and waits for Hello. `--engine-probe` remains the earlier ownership-only test harness, which never launches workers.

Frames are UTF-8 JSONL, at most 65,536 bytes excluding LF, with exactly these fields:

```json
{"major":1,"minor":0,"session":"00000000000000000000000000000001","epoch":0,"id":"00000000000000000000000000000002","revision":null,"type":"Hello","payload":{}}
```

IDs are 32 lowercase hexadecimal digits. Client command IDs use the low half of the ID space; helper internal IDs use the high half and the durable engine epoch plus a checked counter. All numeric fields are nonnegative signed-64-bit-compatible integers. JSON duplicate keys, floats, excessive nesting, NUL, malformed Unicode, unknown envelope fields and oversized/incomplete frames are refused by the strict domain parser. A fatal framing error initiates emergency shutdown; a ProtocolError response is best effort because the channel itself may be unusable.

Hello binds the pipe to one client session and returns the actual engine epoch, negotiated compatibility, bounds and supported operation names. Subsequent requests require that session and epoch. Major/minor mismatch allows diagnostic queries but disallows mutations. Unknown types return Error. Stdout carries envelopes only; stderr and attempt spools are separate.

| Type | Payload / meaning |
| --- | --- |
| PrepareClose | `{}`; durable command receipt, then query CloseStatus for ticket/count |
| CancelClose / ConfirmClose | `ticket`; existing P1-12 validation and all-target stop rules |
| CancelAttempt | `attempt`; commits the existing explicit task-cancel operation |
| HumanDecision | subject, hash, content_hash, displayed_hash, context_hash, upstream event list, prior_decision or null, action Approve/Reject/Revoke; envelope revision required. Uses the unchanged P1-04 exact-review authority |
| Receipt | `command`; original durable command receipt, including across epochs |
| ShutdownReceipt | `ticket`; original durable shutdown receipt, including across epochs |
| CloseStatus | `{}`; state, ticket, active count, newest receipt |
| ReviewBundle | `subject`; exact review fields (revision is returned in payload) |
| Recovery | `offset`, `limit` (1–8); startup inspection digest and bounded subject summaries; no resolution or automatic admission |
| Snapshot | `section` aggregates/claims/effects, `after` key, `cursor` null for first page or expected committed cursor, `limit` 1–8 |
| Events | `after` global sequence, `limit` 1–8; ordered committed events with aggregate identity/revision |
| EventDetail | `seq`, `offset`; up to 2,048 Unicode characters of event payload |
| Log | `attempt`, `sequence`, `offset`; up to 2,048 Unicode characters from a durable normalized provider log record; never an arbitrary file path or unflushed spool tail |

Snapshot pages include session state and `as_of_event_seq` from the same read transaction. A commit between pages returns SnapshotChanged; discard the partial snapshot and reload. Start event replay after the committed snapshot cursor. Projection applies each sequence once; duplicates/older events are ignored, gaps or epoch changes invalidate the projection until snapshot reload. Detail projections explicitly mark `detail_complete=false`; clients must not treat a partial detail as a full review. Normalized log cursors count Unicode characters. Raw stdout/stderr spools remain independent durable evidence.

Command Response has `acknowledged=true`, `operation_completed=false`, command identity, original commit epoch/event cursor, response and replay flag. It proves the transaction, not process termination. Lost responses must be reconciled by receipt lookup or the same command identity/payload, never a newly generated approval/launch identity. Human decisions bind the exact complete review DTO. Unknown commands and stale approvals fail closed.

Input control and query queues each hold 16 bounded frames. The output queue holds 16. Control is checked first; bulk queue saturation returns Busy when possible. Saturated control/output or broken input/output initiates emergency shutdown instead of accumulating unbounded data. There is no provider-output push feed; workers spool independently. No sockets or reconnection to a lost live pipe are offered.

Service owns a registry of concurrent workers. Each calls the existing P1-09 runner and P1-10 settlement. EOF commits P1-12 emergency stop intents. If that commit fails, an in-memory emergency flag reaches each retained runner; only that runner signals its own lifetime handle. This fallback cannot manufacture durable absence/evidence or a clean receipt. Drop also requests emergency handling and joins workers. A stalled worker keeps engine ownership; after 45 seconds the helper reports failure and continues controls, rather than detaching the worker. External OS force termination still requires P1-11 recovery.

After all worker joins, P1-12 verifies and commits READY/FAILED/EMERGENCY. Only READY produces ShutdownReady, with ticket/shutdown IDs, receipt event sequence, stop/seal booleans and outcome details. Exit 0 for a user close requires its frame to fit, write and flush successfully within a two-second acknowledgement bound. A missing/failed flush is exit 4 and remains unknown to the client. EOF emergency can exit 0 after verified local handling, but emits no ShutdownReady and leaves the session unclean. A persisted receipt can exist even when transmission fails; query it after ownership is released. ClientClose requires matching ready proof plus observed successful helper exit; process exit alone never means graceful close.

The runtime exposes typed launch registration for already admitted frozen LaunchSpecs. This milestone does not add arbitrary executable/argv IPC, a scheduler dispatch loop, live provider adapters, socket takeover, GUI views or macOS quit behavior.

## Protocol 1.1 (P2-02)

Minor-version additions only; the frame format, bounds, queues and every 1.0 type are unchanged. A frame with `minor: 0` gets exactly the 1.0 behaviour: the additions answer `UnknownType`, Hello lists only the 1.0 names, and the response carries `minor: 0`. A frame with a minor above 1 is diagnostics only.

The helper is configured when it is started, never over the protocol:

```
jojo-helper --serve [--data-root P] [--worktree-root P]
            [--disposable-root P
             [--codex-executable P --codex-version TEXT --codex-sha256 HEX
              --codex-home P --codex-auth native-subscription]]
```

Hello (minor ≥ 1) adds `slice: true|false`. Without `--disposable-root` the slice commands answer `SliceNotConfigured`. The Codex options are all or nothing; `--codex-auth` accepts only `native-subscription`, which is the launcher's record of the human's attestation.

| Type | Payload (exact field set) / meaning |
| --- | --- |
| RegisterRepository | `path`. The path and the repository's Git common directory must resolve strictly inside the disposable root (a private `0700` directory at least three levels deep). Response text is the repository identity. Refusal: `RepositoryOutsideDisposableRoot`. |
| ImportBundle | `feature`, `prd_id`, `adr_id`, `plan_id`, `validation_id`, `prd` (PRD JSON text), `adr` (text), `step`, `repository`, `base` (commit), `provider` (`Codex` or `ClaudeCode`), `validation` (canonical validation-plan text). Creates drafts only and never approves. Because the core requires an approved PRD before an ADR and both before a plan, the same bundle is sent again after each human decision; each call advances as far as approvals allow. Response text: `stage=PRD|ADR|PLAN|COMPLETE awaiting=<subject ids>`. A bundle that differs from what was imported is refused. |
| Start | `prd`, `adr`, `plan`, `validation`, `step`. One human Start for one approved plan step. The command identity is the attempt identity. Response adds `attempt` and `dispatch` (`launched` or `not repeated`). |
| RecoveryInspect (query) | `{}`. Runs a fresh read-only inspection; returns `digest`, `subjects`, `other_quarantined`. The existing `Recovery` query then pages that inspection. |
| RecoveryResolve | `digest`, `attempt`, `choice` (`ValidateOutput`, `ContinueWithHistory`, `ContinueFreshContext`, `Hold`). Maps to the P1-11 `resolve_recovery`; when the core answers `SETTLE` the helper runs the existing P1-10 settlement. The helper re-inspects afterwards. |
| RecoveryStart | `digest`. Maps to the P1-11 `start_after_recovery`: records the reviewed start and opens admission. It launches nothing. |

Any payload with a missing or extra field is refused with `InvalidPayload`. No command carries an executable, an argument, an environment value or a working directory.

**Start.** Refused before anything is committed when: the slice or the adapter is not configured; a run is active (`CapacityOne`); admission is closed (`AdmissionClosed`); a fresh inspection has any recovery subject (`RecoveryHold`); any of the four subjects lacks a current approval, the plan subject is not an execution plan, or the step is not in it (`StartRefused: …`); the plan's provider has no production adapter (`NoProductionAdapter`); the configured executable, version, digest, home or sign-in attestation is rejected (`AdapterRejected: …`); or the registered repository no longer resolves inside the disposable root. Acceptance is one writer transaction that re-reads admission, every approval, the validation-plan digest, capacity (`approval_attempts` not finished) and whether the step was ever reserved (`DuplicateStart`), queues the step and takes the worktree ticket. The helper then creates and seals the owned worktree, reserves the scheduler envelope and binds the worktree in one admission scan, builds the `LaunchSpec` from durable state, asks `CodexAdapter::prepare_launch` for the OS request, and hands both to the existing worker registry (issue journal → supervised launch → settlement). If a step after acceptance fails, the response is `StartFailed: …` and an existing reservation is retired through the pre-issue path.

**Prompt.** The prompt is composed by the helper from the approved PRD rendering, the approved ADR text and the approved validation plan's write scope and baseline. No prompt text arrives over the protocol.

**Replay.** A slice command whose identity already has a receipt, in this or an earlier epoch, is answered from that receipt alone: no check is re-run and nothing is dispatched again. The same identity with another payload is a conflict. A lost response is reconciled with `Receipt`.

**Recovery.** `RecoveryResolve` and `RecoveryStart` must name the digest of the helper's current inspection (`StaleInspection` otherwise), and the core additionally refuses an inspection whose durable facts have changed. Which choices exist for a subject is decided by the core from process and repository evidence; a choice it does not offer is refused, so an unknown or live process can only be held. An attempt that did not succeed stays a recovery subject after it is settled (the core keeps it until a reviewed continuation exists, which this protocol does not offer), and while any subject exists Start is refused.


## Validation and candidate review (P2-03)

Additive to protocol 1.1 (minor ≥ 1); the minor version is unchanged, 1.0 frames still get `UnknownType`, and Hello lists the new names.

| Type | Payload (exact field set) / meaning |
| --- | --- |
| Validate | `attempt`, `candidate` (the sealed candidate digest on screen). Accepts one host validation run; the command identity is the run identity. Response adds `validation` (`started` or `not repeated`). |
| CandidateDecision | `attempt`, `candidate`, `receipt` (the reviewed receipt event for Accept, `null` for Reject), `action` (`Accept` or `Reject`). |
| Candidate (query) | `attempt`. Sealed result (`outcome`, `reason`, `provider_terminal`, `candidate`, `log_records`), `validation` (`state`, `detail`, `receipt`, `candidate`, up to 16 `reasons`, `reasons_total`) and `decision` (`NONE`, `ACCEPTED`, `REJECTED`). |
| CandidateChanges (query) | `attempt`, `candidate`, `offset`, `limit` (1–8). Repository-derived changed paths with `kind` and `scope` (`ALLOWED`, `OUT_OF_SCOPE`, `BASELINE`, `UNKNOWN`), the overall `scope` verdict and `total`. |
| CandidateDiff (query) | `attempt`, `candidate`, `offset`. A 4,096-character page of the bounded (1 MiB) diff of the frozen base against the sealed candidate; `complete` is false when the diff was cut. |
| ValidationReport (query) | `attempt`, `receipt`, `offset`. A page of the validator report retained as evidence for that receipt. |

Agent output uses the existing `Log` query; `log_records` is its highest sequence.

**Validation state.** `NOT_RUN`: never requested. `UNKNOWN`: requested but no verdict exists (running, interrupted, or it could not be performed; `detail` says why). `PASS` and `FAIL`: the latest run recorded a receipt. Only `PASS` is a success. `provider_terminal` is what the provider said about itself and is never a validation state.

**Validate.** Refused before anything runs when: the attempt has no sealed candidate or `candidate` is not it (`StaleCandidate`); the candidate was decided (`CandidateDecided`); a run or validation is active (`CapacityOne`, `ValidationActive`); the attempt's approvals or the validation plan's approval are stale or revoked; or the approved plan names no command for the step. No command, argument, path or environment value can be sent: the helper runs exactly the argument vector in the approved validation plan (`jojo-validation-plan-v2`), in a new private directory under the data root holding an independent clone at the candidate commit with the sealed packet applied, through the supervised process runner, under `sandbox-exec` with the network denied, with a cleared environment and a time and output budget. The command is not started when a changed path is out of scope or baseline. A run without a verdict records none.

**CandidateDecision.** Accept is refused unless, at that moment: the sealed packet still has its digest and the retained worktree still reproduces it (`CandidateChanged`); `receipt` is the current passing verdict for exactly that candidate; approvals are still fresh; the attempt succeeded; and the candidate is undecided. A decision is final. Accepting records the decision only: nothing is merged, pushed or published.

**Replay.** As for every 1.1 command: an identity that already has a receipt is answered from it and nothing is run again; a lost response is reconciled with `Receipt`.


## Typed messages and the structured assessment (Refactor R-01)

Every request type, the minor that introduced it, its queue (control or bulk) and the payload fields the helper reads are defined once, in `crates/jojo-domain/src/protocol.rs` (`SPECS`, `RequestKind` and one payload type per request). The helper dispatches on `RequestKind` and Hello's `commands` and `queries` lists are produced from the same table. The client's list is `macos/JojoApp/Sources/JojoCore/Protocol.swift`; `scripts/tests/test_protocol_fields.py` checks the two against each other. Requests and their answers are otherwise unchanged: `crates/jojo-storage/tests/r01_frames.rs` holds 168 frames whose answers were recorded before the change.

One addition to the wire, for frames of minor ≥ 1 only (the minor version is unchanged, and a 1.0 frame gets the row as before): each `Recovery` row carries the assessment as structured fields after the existing four.

| Field | Meaning |
| --- | --- |
| `blockers` | List of text: what blocks every choice except Hold. At most 8 entries of at most 256 characters each. |
| `blockers_complete` | `false` when the list or one of its entries was cut. |
| `process_absent` | `true` when every process that could belong to the attempt is proven gone. |
| `choices` | List of the choices the evidence offers, by the names `RecoveryResolve` accepts. |

The `assessment` text is still sent unchanged. It is a debug rendering for people; a client must read the structured fields and must not parse the text.


## Agent guard (P2-04.1)

Not a protocol change. No frame, type or field was added, and nothing about the guard can be requested over the protocol.

A serving helper starts a second instance of its own executable for every process group it launches (the agent of a `Start`, the command of a `Validate`):

```
jojo-helper --agent-guard <leader pid> <absolute path of ps>
```

The guard is started inside that process group, with a cleared environment, nothing on standard output or error, and a pipe on standard input whose only write end the helper holds. It first verifies from the OS that its own process group is `<leader pid>` and exits with status 3 otherwise, having signalled nothing. It then reads the pipe:

- the release byte followed by end-of-file (the helper has confirmed the group stopped): exit 0, no signal;
- end-of-file alone (the helper is gone, however it ended): TERM to the leader, up to 2 seconds, then KILL to the group, which ends the guard as well.

While the helper lives it remains the only supervisor: the guard takes no part in a normal stop, a guard that ends (a TERM sent to the group ends it too) is replaced, and a guard that cannot be kept is an inspection failure that stops the run. The guard is never recorded as a descendant of the agent. Started by hand outside a process group it was placed in, the mode does nothing.

Limits: the guard reaches the owned process group only; it is an ordinary process of the same user; and a helper that is frozen rather than dead keeps the pipe open. See [P2-04.1 verification](../verification/P2-04-1.md).
