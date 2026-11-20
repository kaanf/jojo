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

## Approved plan query (design wave 1)

One query was added for protocol minor 1; the minor version stays 1 and Hello lists the name `ApprovedPlan` last among the queries. It is a read: no command, no table and no schema change.

Request: `ApprovedPlan` with exactly one field, `after`: `null` for the first approved plan, or the `next` of the previous answer. A payload with another field set is answered `InvalidPayload: unexpected or missing fields`; a 1.0 frame naming it is answered `UnknownType`.

Answer: `null` when no execution plan after `after` holds a current approval, otherwise one plan, whole:

| Field | Value |
|---|---|
| `plan`, `revision`, `digest` | The plan's identity, the approved revision and the digest of its canonical body |
| `decision` | The event of the approval that is current |
| `prd`, `adr` | The documents the plan was stored under |
| `validation`, `validation_digest` | The digest of the validation plan the plan binds, and the identity of the one document under the same PRD and ADR whose latest body has that digest; `null` when there is none or more than one |
| `steps` | Per step: `step`, `revision`, `provider`, `repository`, `priority`, `repository_mode`, `environment`, and `sources` (`repository`, `commit`, `snapshot`). `provider`, `repository`, `priority` and `repository_mode` are `null` for a step the plan binds no resources to |
| `edges` | Per edge of the execution graph: `edge`, `prerequisite`, `dependent`, `predicate` (`ArtifactApproved` or `Evidence`), and what the predicate names: `artifact` with `artifact_revision`, or `evidence` with `evidence_kind` |
| `approvals` | The approvals the plan binds: `artifact`, `revision`, `decision` |
| `next` | The cursor for the following approved plan, or `null` when this is the last |

A plan is "approved" exactly when a Start would find its approval current. A newer revision that is not approved makes the approval stale, so such a plan is not returned at all: the answer never holds an approved and a proposed revision side by side.

Not returned, because the core does not store it: a name, description, acceptance criteria or file scope of a step; a lifecycle state of a step; whether an edge's predicate is met; the result of the plan's validation. Stored but not returned: the edges of the artifact graph, the permission digest, the required outputs, the additional resources of a step, and the attempts of a step (`scheduler_attempts`).

The whole plan is one answer. An answer above the frame limit is refused with `PayloadTooLarge: use bounded queries`, as for every request; the steps are not paged.

## Protocol 1.2: Feature Chat (M2)

Protocol minor 2 adds three commands and three queries for the lead agent of a feature. The helper's `MINOR` is 2; Hello for a frame of minor ≥ 2 lists `ChatTurn AnswerQuestion CancelTurn` after the 1.1 commands and `FeatureChat ChatText ChatIndex` after the 1.1 queries. A 1.0 or 1.1 frame naming any of them is answered `UnknownType`; everything a 1.1 client sends is answered exactly as before. The macOS client sends these six requests at minor 2 and every other request at minor 1 (`Request.minor`). Every payload is checked for its exact field set: a frame with any other field is answered `InvalidPayload: unexpected or missing fields`.

A turn is one fresh, read-only provider run in a registered repository: Claude Code with `--tools Read,Glob,Grep --permission-mode manual`, Codex with `--sandbox read-only`. It is not an attempt: it has no worktree, plan, candidate or approval. One run at a time: a turn is refused while an attempt or another turn is active, and a Start is refused while a turn runs. A reply never approves anything.

### Commands

| Request | Fields | What it does |
|---|---|---|
| `ChatTurn` | `feature` (identity), `kind` (`Discover` or `Plan`), `repository` (a registered repository), `provider` (`Codex` or `ClaudeCode`), `message` (text) | Accepts one turn under the command identity, which is the turn's identity, composes its prompt from stored content, records the HEAD commit of the repository, and runs it. The first turn of a feature must be `Discover` and its `message` is the objective; a later `message` is a note and may be empty. A `Plan` turn needs the feature's requirements document approved. The receipt adds `turn` and `dispatch` (`launched`, or `not repeated` for a replayed command). Refusals: `AdmissionClosed`, `CapacityOne: …`, `SliceNotConfigured`, `NoProductionAdapter: …`, `AdapterRejected: …`, `TurnRefused: …` (repository not registered, another repository, no objective, no approved requirements, message too long), `PromptTooLarge: …`; `TurnFailed: …` when the accepted turn could not be launched (it is then sealed as failed) |
| `AnswerQuestion` | `question` (identity), `revision` (the question's revision as shown), `action` (`Answer`, `Defer` or `Block`), `answer` (text exactly when `action` is `Answer`, otherwise `null`) | Appends a human answer, or sets the question aside (`Defer`) or makes it blocking again (`Block`), by compare-and-swap at `revision`. Only questions a turn asked are reachable. A question about the whole feature is part of what the requirements approval binds, so answering it makes that approval stale; a question about one plan step changes no approval. Refusals: `UnknownQuestion: …`, `InvalidAnswer: …`, `UnknownAction`, a revision conflict |
| `CancelTurn` | `turn` | Records the human's stop of a running turn; the runner stops the process tree within one poll and the turn ends `Cancelled`. Read from the control queue. Refusals: `UnknownTurn`, `TurnFinished` |

### Queries

`FeatureChat` with `feature` and `after` (an ordinal; `0` from the start) returns one page:

| Field | Value |
|---|---|
| `feature` | The feature |
| `turns` | The turns after `after`, oldest first, as many as fit in 40 KiB of rows (at least one). Per turn: `turn`, `ordinal`, `kind`, `provider`, `base` (the commit the turn read), `message` (at most 1024 characters) and `message_complete`, `state` (`Running`, `Stopping`, `Replied`, `Failed`, `Cancelled`, `Interrupted`), `detail` (why it failed, at most 1 KiB), `summary` (the lead's words to the human), `prd` (`document`, `revision` of the version the turn created, or `null`), `plan` (`adr`, `plan`, `validation`, `drafted`, `steps` with `step`, `key`, `title`; or `null`), `questions` |
| `questions` (per turn) | `question`, `revision`, `prompt`, `blocking`, `answer` (newest, at most 1024 characters, or `null`) with `answer_complete`, `answered_by` (`HUMAN`, `MODEL` or `null`), `step` (the key of the one step it is about, or `null` for the whole feature) |
| `next` | The ordinal to ask after for the next page, or `null` |
| `running` | `turn` and `stop_requested` of the turn without a result, or `null` |
| `prd` | The feature's requirements document: `document`, `revision`, `author`, `ready` (complete and no unanswered blocking question), `blockers` (their count), `approved` (a current approval), `length` (characters); or `null` |

`ChatText` with `subject`, `part` and `offset` returns at most 8192 characters of one long text from a character offset: `subject`, `offset`, `next`, `total`, `complete`, `text`; or `null` when the subject has no such text. `part` is `Message`, `Reply` or `Prompt` (subject: a turn), `Answer` (subject: a question, its newest answer) or `Prd` (subject: a requirements document, the rendered newest version). Another `part` is answered `UnknownPart`.

`ChatIndex` with no field returns `repositories` (per registered repository: `repository`, `path` at most 160 characters, `path_complete`), `repositories_complete`, `features` (per feature with a turn, newest first: `feature`, `repository`, `objective` at most 160 characters, `objective_complete`, `turns`, `running`) and `features_complete`. Each list holds at most 32 rows.

### What a reply becomes

The lead's last message must be exactly one JSON object of the turn's schema (`jojo-lead-discovery-v1` or `jojo-lead-plan-v1`, `crates/jojo-domain/src/planner.rs`); one surrounding code fence is tolerated and nothing else. A discovery reply becomes clarification questions and a new version of the feature's one requirements document, all authored by the model. A plan reply becomes an ADR draft at once and, when the human has approved that ADR, a validation plan and an execution plan: one node per step, one edge per dependency with an `Evidence` predicate of kind `ValidationPassed` on the prerequisite step, a provider per step, every step from the base commit the turn read. A plan reply that asks about the whole feature drafts nothing. A message that is not such an object fails the turn and stores nothing. The `HumanDecision` receipt carries an extra field `lead_plan` (`PlanNotDrafted: …`) only when an approved ADR's draft could not become a plan.

### Other changes

A blocking question about one plan step that no human answered refuses that step's `Start` with `StepQuestionOpen: …`. When a helper closes or loses its client, a running turn is stopped and sealed `Failed` with the detail `the helper is closing`; a turn an earlier helper left without a result is sealed `Interrupted` when the next helper starts. Neither is a recovery subject.

## Protocol 1.3: project records and the plan runner (M3)

Protocol minor 3 adds three commands and two queries. The helper's `MINOR` is 3; Hello for a frame of minor ≥ 3 lists `SaveProject RunPlan ControlPlan` after the 1.2 commands and `Projects PlanRun` after the 1.2 queries. A frame of minor ≤ 2 naming any of them is answered `UnknownType`. The macOS client sends these five at minor 3 and only while Projects or Execution Plan is shown (`Request.minor`). Every payload is checked for its exact field set. `ControlPlan` is read from the control queue.

Changed in 1.3 for every client: capacity is no longer one. `Start` and `ChatTurn` are refused with `CapacityFull: <n> attempts and <m> lead turn use all <c> units; … waits` when the unfinished attempts (held ones included) and the running lead turn fill the global capacity, which a project record sets (D4: two in all by default for a new record, not split by provider, no limit per repository; without any project record the Phase 1 configuration of one stays). A second turn is refused `TurnRefused: another lead turn is running`; a second host validation `ValidationBusy: another host validation is running`. The old code `CapacityOne` is no longer sent.

### Commands

| Request | Fields | What it does |
|---|---|---|
| `SaveProject` | `project` (identity), `revision` (the revision edited; `0` for a new project), `name`, `repositories` (1 to 4 registered repository identities), `instructions` (at most 2048 bytes), `test_command`, `ci_command` (argument vectors, an absolute program first, or empty), `kinds` (at most 8 of `{kind, provider}`; a kind is lower case letters, digits and `-`), `default_provider` (`Codex` or `ClaudeCode`), `capacity` (1 to 8) | Saves the whole record as the next revision (revisions are immutable) and sets the scheduler's capacity: `global`, `provider:claude`, `provider:codex` and the per-repository default all become `capacity`. Response `project revision=<n>`. Refusals: `InvalidProject: …` (a field out of bounds, an unregistered repository, a repository of another project, more than eight projects), `StaleProject: …`, `UnknownProvider` |
| `RunPlan` | `plan` | Starts the approved revision of a plan a lead drafted; approving a plan does not run it. Response `plan run revision=<n>`. Refusals: `RunRefused: not a plan a lead drafted`, `RunRefused: missing or stale approval: <document>`, `RunRefused: this plan revision was already started; resume it instead` |
| `ControlPlan` | `plan`, `action` (`Pause`, `Resume` or `Cancel`) | Pause: no new step starts, running attempts continue. Resume. Cancel (final): queued steps are cancelled and their unsealed attempts are asked to stop. Response `plan run <STATE>` or `plan run already <STATE>`. Refusals: `UnknownPlanRun: …`, `PlanRunCancelled: …`, `UnknownAction` |

### Queries

`Projects` (no field) returns `projects` (per project, newest revision: `project`, `revision` and the fields of `SaveProject` other than `project` and `revision`), `capacity` (the scheduler's global capacity now) and `configuration` (its revision).

`PlanRun` with `plan` returns the newest run of that plan, or `null` when it was never started: `plan`, `revision`, `state` (`RUNNING`, `PAUSED`, `CANCELLED`, `HALTED` when every step left needs the human, `DONE` when every step is integrated), `control` (the newest control), `repository`, `base`, `branch` (`jojo/plan/<plan identity>`), `head` (the newest integration commit, or the base), and `steps` in plan order: `step`, `key`, `title`, `kind` (or `null`), `provider`, `depends_on` (keys), `state` (`Waiting`, `Blocked`, `Ready`, `Running`, `Validating`, `Review`, `Accepted`, `Integrated`, `Conflict`, `Failed`, `Refused`, `Cancelled`), `detail` (at most 512 characters), `attempt`, `candidate`, `receipt` (the validation receipt), `commit` (the integration commit), each `null` when absent. One plan has at most six steps, so one answer holds it.

### The runner

The helper's serve loop runs one runner pass every 250 ms. A pass does nothing unless a run is `RUNNING`, admission is open and the last inspection holds no recovery subject. In order: each accepted step is integrated, one at a time in the order the human accepted them; each settled step that succeeded gets its host validation (one at a time); each ready step is started through the `Start` path under a runtime command identity (`PlanStepStart`), so every check of a Start applies. A step is ready when every prerequisite is integrated, no blocking question about it is open, it was never started and its write scope does not overlap a step that is running or not yet integrated. A refusal that means "not now" (`CapacityFull`, `AdmissionClosed`, `RecoveryHold`) ends the pass; any other refusal is recorded once (`plan_step_refusals`) and the step is `Refused`. Nothing is retried. A helper that starts finds every run `PAUSED` (reason "the helper started again").

### Integration

An accepted step's sealed candidate is applied onto the plan branch in the registered repository, in a new detached worktree under `<data root>/integration/`: `git apply` of the packet (no three-way merge) onto the branch's head, `git add --all`, `write-tree`, `commit-tree` as `Jojo <jojo@localhost>` with the step's key and title, then `update-ref refs/heads/jojo/plan/<plan>` from the expected head (or from nothing for the first). A hunk or new file that does not apply, or a branch that someone else moved, records a `CONFLICT` and moves nothing. An integration records the `ValidationPassed` observation the step's dependents wait for. A step of a running plan starts from the plan branch's head (E8 in `docs/roadmap/DESIGN_ROADMAP_DIFFERENCES.md`). The developer's checkout is never touched and nothing is pushed.

## Protocol 1.4: the executables query (D3)

Protocol minor 4 adds one read-only query. The helper's `MINOR` is 4; Hello for a frame of minor ≥ 4 lists `Executables` after `PlanRun`. A frame of minor ≤ 3 naming it is answered `UnknownType`. The macOS client sends it at minor 4 and only while Settings & Providers is shown (`Request.minor`); it asks again after a second while a provider is still `checking`. The payload has no field; anything else is `InvalidPayload: unexpected or missing fields`.

`Executables` returns `providers`, Claude Code first, then Codex. Each entry:

| Field | Meaning |
|---|---|
| `provider` | `ClaudeCode` or `Codex` |
| `source` | `discovered` (looked up on the login shell's PATH), `path` (the launcher's `--…-executable` alone), `pinned` (path, version and digest given at launch), `none` (not configured) |
| `state` | `ready`, `checking` (being looked up, probed or hashed on a background thread), `refused`, `unconfigured` |
| `detail` | Why it is refused; empty otherwise |
| `entry` | The PATH entry it was found under (a link, or the npm launcher of Codex); `null` unless `discovered` |
| `path`, `version`, `sha256`, `profiled` | The native binary as the helper knows it now, its `--version` text, its digest and whether that version is the one the stream profiles were written against; `null` while unknown |
| `last_run` | The executable of the provider's last attempt or lead turn: `path`, `version`, `sha256`, `profiled`, `event` (the event that first recorded it); `null` before any run |
| `change` | The latest change between two runs: `event` (`ExecutableChanged`), `previous` and `current` (each like `last_run`); `null` when it never changed |

Asking may start a background check in the helper; it never waits for one and never runs a provider.

Changed in 1.4 for every client:

- `Start`, `ChatTurn` and a runner pass can be answered `ExecutableChecking: the <provider> executable … try again shortly` while the helper learns an executable's version and digest off its control thread. Nothing is accepted. The runner leaves that step ready and starts the other provider's steps in the same pass.
- A discovered provider that cannot be run is refused with `ExecutableRefused: <provider>: <reason>` (not on the login shell's PATH, a script launcher, no usable `--version`).
- A version other than the profiled one is no longer refused, except for a pinned executable, which still answers `AdapterRejected: Version` (or `AdapterRejected: Digest`).
- Schema 14 records, in the transaction that issues the run, the executable of every launched attempt (`attempt_executables`) and every accepted lead turn (`turn_executables`), with the events `ExecutableRecorded` (a new provider, file, release and digest) and `ExecutableChanged` (`<provider>|<previous identity>|<identity>`).

How the helper finds an executable is in `crates/jojo-storage/src/discovery.rs` and in `docs/roadmap/DESIGN_ROADMAP_DIFFERENCES.md`, E9.

## Protocol 1.5: repairs and reviews (M4)

Protocol minor 5 adds one read-only query and two fields of a project record. The helper's `MINOR` is 5; Hello for a frame of minor ≥ 5 lists `Repairs` after `Executables`. A frame of minor ≤ 4 naming it is answered `UnknownType`. The macOS client sends it at minor 5 and only while Agent Runs is shown, again with each new snapshot (`Request.minor`). The payload has no field; anything else is `InvalidPayload: unexpected or missing fields`.

`SaveProject` carries two more fields at every minor, and a payload without them is `InvalidPayload: unexpected or missing fields`: `repair_limit` (0 to 2: automatic repairs of a failed plan step; 0 is off) and `review` (boolean: a step whose host validation passed is reviewed by the other provider). `Projects` answers both for every record. A record saved before 1.5 reads as `repair_limit` 2 and `review` true.

`Repairs` returns `attempts` (at most 64, newest run first, each step's attempts newest first) and `complete` (false when more exist). Each entry:

| Field | Meaning |
|---|---|
| `attempt`, `plan`, `step`, `key`, `provider` | The attempt, its plan run, the step, its key and the provider that did it |
| `state` | The step's state when this is the step's newest attempt (see `PlanRun`); `null` for an older attempt |
| `repair` | `null` for a step's first attempt; else `ordinal`, `limit` and `failure` (one line: why the attempt it repairs failed) |
| `hold` | Why the step waits for the human because of this attempt; `null` otherwise |
| `review` | `null` until a review of this attempt's candidate started; else `review`, `provider` (the other one), `state` (`RUNNING`, `APPROVED`, `REJECTED`, `FAILED`, `CANCELLED`, `INTERRUPTED`), `notes` (2048 characters at most), `objection` (`null` or one sentence), `detail` |

Changed in 1.5 for every client:

- `PlanRun` answers `repair_limit` and `review_enabled` (the project's newest settings) for the run, and `repairs`, `repair` (`null` or `ordinal` and `limit`) and `review` (as above) for each step. A step's `state` may also be `Repairing` (a repair is decided or running), `Reviewing` (validation passed; the review is queued or running) and `NeedsHuman` (held; `detail` says why). `Failed` now means the runner has not yet decided between a repair and a hold; it is decided on the next pass.
- `Start` of a step that already ran is refused with `DuplicateStart` as before, except for the repair the core recorded for the step's newest, sealed attempt; only the runner starts that, under the identity it recorded.
- A running review takes one unit of capacity: `CapacityFull: <n> attempts and <m> turns use all <c> units; <what> waits`, where turns now count lead turns and reviews.
- A cancelled run asks its running review to stop. A review left by a helper that ended is closed as `INTERRUPTED` when the next helper starts; its step waits for the human.
- Schema 15 records repairs, holds, reviews, their results and stops, and the executable of each review (`review_executables`; `Executables` counts a review as a run).

The policy is `crates/jojo-domain/src/repair.rs`; the review prompt and verdict are `crates/jojo-domain/src/review.rs`; the decisions are in `docs/roadmap/DESIGN_ROADMAP_DIFFERENCES.md`, D2 and E10.

## Protocol 1.6: user-facing data

Protocol minor 6 adds two read-only queries and, for a frame of minor ≥ 6, three fields of existing answers. Nothing is stored for them: the helper derives each value from what the core already records (schema 15 unchanged). The helper's `MINOR` is 6; Hello for a frame of minor ≥ 6 lists `Attempts Documents` after `Repairs`. A frame of minor ≤ 5 naming either is answered `UnknownType`. Each payload has exactly the field `before`; anything else is `InvalidPayload: unexpected or missing fields`, and a `before` that is neither `null` nor a non-negative integer is refused as such. The derivation is `crates/jojo-storage/src/overview.rs`.

The macOS client sends `Documents` at minor 6 after every snapshot (the Approval Inbox's sidebar counter follows the journal on every screen), `Attempts` at minor 6 only while Agent Runs is shown, `ChatIndex` at minor 6, and `Events` at minor 6 only for Activity & Audit (the poll that notices new events stays at minor 1).

### Attempts

`Attempts {before}` returns `attempts`, at most 16, newest reservation first, and `next` (`null`, or the `before` of the following page). `before` is `null` for the newest page. Each entry:

| Field | Meaning |
|---|---|
| `order` | The attempt's place in reservation order; the cursor |
| `attempt`, `step` | The attempt and its plan step |
| `key`, `title` | The step's key and title from its lead plan (title at most 160 characters); `null` for a step of an imported plan |
| `plan`, `feature`, `feature_name` | The execution plan the attempt was admitted under, its feature and the feature's name (below); `null` when unknown |
| `provider` | `ClaudeCode` or `Codex` once a process was issued; `null` before |
| `state` | The attempt's lifecycle state, from its newest record: `Reserved`, `Preparing` (a worktree exists), `Launching` (a process was issued), `Running` (the process root was identified), `Stopping` (a stop was requested), `Settling` (the process tree is proven gone and the result is being sealed); `Unsupervised` (unsealed and reserved by an earlier helper: nobody supervises it now; the Recovery Center decides it), `Held` (held for recovery); after its result `Succeeded`, `Failed`, `Cancelled`, `Interrupted` |
| `started_at_unix_ms`, `ended_at_unix_ms` | When its first process was issued and when its result was sealed; `null` until then |

### Documents

`Documents {before}` returns `documents`, at most 16, newest revision first, and `next` (`null`, or the `before` of the following page: the `order` of the last row). A document is a requirements document (PRD), an ADR, a validation plan or an execution plan; its newest revision is described. Every answer also carries, whatever the page, `candidates` (at most 16, newest first) and `pending` (the number of documents whose state is `Pending` plus the number of all waiting candidates). Each document:

| Field | Meaning |
|---|---|
| `document`, `kind`, `revision` | The subject; `Prd`, `Adr`, `ValidationPlan` or `ExecutionPlan`; its newest revision |
| `title` | A requirements document's own title (at most 160 characters); `null` for the others |
| `feature`, `feature_name` | Its feature (an ADR's or a plan's is its requirements document's) and the feature's name |
| `author` | `Human` or `Model` for a requirements document; `null` for the others |
| `approval` | `Pending` (waits for the human's decision), `Approved`, `Rejected`, `Revoked` (the latest decision applies to this revision), `Waiting` (an upstream document is not approved), `NotReady` (a requirements document with findings) |
| `stale` | `Pending` after a decision on an earlier revision |
| `created_at_unix_ms`, `decided_at_unix_ms` | When the revision was created; when the decision that applies to it was recorded (`null` without one) |
| `order` | The event that created the revision; the cursor |

A waiting candidate is a sealed attempt whose newest host validation passed and that the human neither accepted nor rejected: `attempt`, `key`, `title`, `feature`, `feature_name`, `receipt` (the passing receipt's event) and `validated_at_unix_ms`. It is accepted or rejected with `CandidateDecision`, as before.

### Changed for a frame of minor ≥ 6

- `Events` gives each event `at_unix_ms` (when it was recorded) and `actor`: `Human` for an event of a command a client sent (a command identity whose top bit is clear; the helper's own commands use reserved identities), `ClaudeCode` or `Codex` for a record of that provider's output (a provider stream record, and every event of the command that sealed a lead turn that replied or a review that gave a verdict), and `Core` for everything else, including events without a command (`EngineStarted`).
- `ChatIndex` gives each feature `name`: the title of its newest requirements document, else the first non-empty line of its first message, at most 160 characters.

A frame of minor 1 to 5 is answered exactly as before. `crates/jojo-storage/tests/r01_frames.rs` sends its newer-than-helper Hello at minor 7 and pins nine more answers at its end (217 in all).

## Protocol 1.7: the Dashboard's data

Protocol minor 7 adds one read-only query and, for a frame of minor ≥ 7, more fields of two existing answers. Nothing is stored for them: the helper derives each value from what the core already records (schema 15 unchanged). The helper's `MINOR` is 7; Hello for a frame of minor ≥ 7 lists `Features` after `Documents`. A frame of minor ≤ 6 naming it is answered `UnknownType`. Its payload has exactly the field `before`; anything else is `InvalidPayload: unexpected or missing fields`, and a `before` that is neither `null` nor a non-negative integer is refused as such. The derivation is `crates/jojo-storage/src/features.rs`.

The macOS client sends `Features` and `Attempts` at minor 7 and `Events` at minor 7 for Activity & Audit and the Dashboard (the poll stays at 1). The Dashboard, the screen the app opens on, reads `Features`, `Attempts`, `Executables` and `Events` while it is shown and with each new snapshot.

### Features

`Features {before}` returns `features`, at most 16, newest first, and `next` (`null`, or the `before` of the following page: the `number` of the last row). A feature is one that has a chat turn or a requirements document. Each entry:

| Field | Meaning |
|---|---|
| `feature`, `number` | The feature and its place in the order features first appeared, from 1 (the app shows it as "FT-n") |
| `name` | As in `ChatIndex` at 1.6 |
| `repository`, `repository_name`, `project`, `project_name` | The repository it works in (its chat's, else its newest plan run's), the last component of its path, and the project that repository belongs to at its newest revision; `null` when unknown |
| `stages` | `discovery`, `specification`, `planning`, `execution`, `verification`, each `{state, label}`: `state` is `Done`, `Current`, `Running`, `NeedsYou`, `Blocked`, `Failed`, `Stale`, `Unknown` or `Off` (not reached); `label` a few words (`"Q open"`, `"PRD rev 2"`, `"ADR rev 1"`, `"2 running"`, `"1 to accept"`) or `null` |
| `running` | Its live attempts |
| `blocked` | Its blockers, all of them |
| `awaiting` | Its documents in state `Pending` and its candidates that passed host validation and wait for the human |
| `done` | Its plan run is `DONE`; without a run, execution and verification are both `Done` |
| `blockers` | At most 8: `kind` (`Question` or `Step`), `question`, `step`, `key`, `title`, `reason` (the question's prompt or the step's detail) |

How each stage is derived:

- **Discovery:** `NeedsYou` while a blocking question about the feature has no human answer; `Running` while a discovery turn runs; `Done` once a requirements document exists; `Current` ("In discussion") when turns exist without either; else `Off`.
- **Specification:** the newest requirements document's approval: `Approved` is `Done`; `Pending` is `NeedsYou` ("PRD rev n"), or `Stale` after a decision on an earlier revision; `Rejected` and `Revoked` are `Failed`; `NotReady` is `Current` ("Drafting").
- **Planning:** `Running` while a plan turn runs; then the ADR and plans drafted on that requirements document: the first `Pending` one is `NeedsYou` ("ADR rev n", "Plan rev n"), a stale one `Stale`, a rejected or revoked one `Failed`, all approved `Done`.
- **Execution:** with a plan run, `DONE` is `Done`, else steps `Running`, `Repairing`, `Validating`, `Reviewing` give `Running` ("n running"), steps `Blocked`, `NeedsHuman`, `Conflict`, `Refused` give `Blocked` ("n blocked"), steps `Review` give `NeedsYou` ("n to review"), a paused run `NeedsYou`, a cancelled one `Failed`, a halted one `Blocked`. Without a run, from the newest attempt of each step of the feature's plans: live ones `Running`, a held one `Blocked`, a succeeded one `Done`, else `Failed`.
- **Verification:** over the newest attempt of each step that has a candidate: a failed host validation `Failed` ("n failed"), one without a verdict `Unknown`, a passed one the human has not decided `NeedsYou` ("n to accept"), one never run `NeedsYou` ("n not run"), all accepted `Done`.

### Changed for a frame of minor ≥ 7

- `Attempts` gives each attempt `ordinal` (its place among its step's attempts, from 1), `repository`, `repository_name`, `project`, `project_name`, `validation` (`null` without a candidate; else `state` `NotRun`, `Unknown`, `Passed` or `Failed`, `at_unix_ms` of the verdict, and `decision` `Accepted`, `Rejected` or `null`) and `activity` (`null` before any provider record; else `text`, a few words for the newest record's kind such as "Using a tool" or "Writing", and `at_unix_ms`).
- `Events` gives each event `summary`: one sentence with the names a person knows ("api · attempt 1 started on Claude Code", "Host validation of api · attempt 1 passed", "Slugify from the shell rev 1 approved" for a requirements document by its title, "Plan run of Slugify from the shell started"). An event type without a sentence of its own is named by its type ("Worktree ready"), with the attempt its payload names.

A frame of minor 1 to 6 is answered exactly as before. `crates/jojo-storage/tests/r01_frames.rs` sends its newer-than-helper Hello at minor 8 and pins five more answers at its end (222 in all).

## Protocol 1.8: dated chat turns and assumptions

Protocol minor 8 adds no request. For a frame of minor ≥ 8 the `FeatureChat` answer carries more; a frame of minor 1 to 7 is answered exactly as before. Nothing is stored for it (schema 15 unchanged); the derivation is `crates/jojo-storage/src/features.rs` (`turn_times`, `feature_assumptions`). The helper's `MINOR` is 8; Hello lists the same names as at 1.7. The macOS client sends `FeatureChat` at minor 8.

- Each turn gives `started_at_unix_ms` (when the core accepted it) and `ended_at_unix_ms` (when its result was recorded; `null` while it runs).
- The answer gives `assumptions`: at most 32, from the feature's newest requirements document over all its revisions in order. Each: `id` and `statement` (its newest wording, at most 160 characters), `state` `Current` (the newest revision has it; `revision` is the one it first appeared in) or `Dropped` (an earlier revision had it and the newest does not; `revision` is the one that dropped it). A revision whose content does not read as a requirements document is skipped. Whether an assumption was verified is not recorded.

`crates/jojo-storage/tests/r01_frames.rs` sends its newer-than-helper Hello at minor 9 and pins one more answer at its end (223 in all).

## Protocol 1.9: the details of the rebuilt screens

Protocol minor 9 adds four read-only queries for Specifications, the Approval Inbox, Execution Plan, Agent Runs, Source Control and Verification & Evidence. Nothing is stored for them: the helper derives each value from what the core already records (schema 15 unchanged). The helper's `MINOR` is 9; Hello for a frame of minor ≥ 9 lists `Document Decisions PlanDetail AttemptDetail` after `Features`. A frame of minor ≤ 8 naming any of them is answered `UnknownType`. Each payload has exactly its fields; anything else is `InvalidPayload: unexpected or missing fields`. The derivations are `crates/jojo-storage/src/overview.rs` (`Document`, `Decisions`) and `crates/jojo-storage/src/details.rs` (`PlanDetail`, `AttemptDetail`).

The macOS client sends each of them at minor 9 and only while the screen that shows it is the shown destination, again with each new snapshot. `ReviewBundle`, `Candidate` and the other requests the GUI scenarios drive are sent as before, so the scenarios see no 1.9 frame.

### Document

`Document {document, revision, offset}` (`revision`: `null` for the newest, or a revision number; `offset`: a character offset into the text) returns `null` when the core has no such document or revision, else:

| Field | Meaning |
|---|---|
| `document`, `kind`, `title`, `feature`, `feature_name` | As in `Documents` (1.6); `title` of the chosen revision |
| `revision`, `newest` | The revision described and the document's newest |
| `approval`, `stale` | The newest revision's approval state, as in `Documents` |
| `revisions` | At most 32, newest first: `revision`, `created_at_unix_ms`, `digest` (the content digest a decision binds), `author` (`Human`, `Model` or `null`), `decision` (the latest recorded on that revision: `Approved`, `Rejected`, `Revoked` or `null`), `decided_at_unix_ms`, `superseded` (a newer revision exists) |
| `history` | At most 32 decisions on the document, newest first: `revision`, `decision`, `at_unix_ms`, `digest`, `applies` (the decision that binds the current bundle) |
| `upstream` | The requirements document and ADR this revision was drafted on: `document`, `kind`, `revision`, `title`, `approval` |
| `downstream` | The ADRs and plans drafted on this document, newest revision of each: `document`, `kind`, `revision`, `approval` |
| `counts` | For a requirements document: how many `actors`, `use_cases`, `acceptance_criteria`, `nfrs`, `assumptions`, `blockers`, `non_goals` and whether it has a `goal` (0 or 1); else `null` |
| `findings` | For a requirements document: at most 32 `{path, problem}` (`Empty`, `Placeholder`, `UnknownReference`, `OpenBlocker`); else `null` |
| `criteria` | For a requirements document: at most 32 acceptance criteria `{id, statement, use_cases}` (the use cases that name it); else `null` |
| `offset`, `next`, `total`, `complete`, `text` | At most 8192 characters of the revision's rendered text, as `ChatText` pages it |

### Decisions

`Decisions {before}` returns `decisions`, at most 16 decisions on documents newest first, recorded before event `before` (`null` for the newest), and `next`. Each: `order` (the decision's event; the cursor), `document`, `kind`, `title` (of the decided revision of a requirements document), `feature`, `feature_name`, `revision`, `decision`, `applies` (it is the document's newest decision and binds its current bundle; an approval that no longer applies was invalidated by a newer revision or a later decision), `at_unix_ms`.

### PlanDetail

`PlanDetail {plan}` returns `null` for a plan the core does not hold, else the newest stored revision of that execution plan: `plan`, `revision`, `approval` (as in `Documents`), `feature`, `feature_name`, `run` (the newest run's state, as in `PlanRun`, or `null`), `checks` (`cycle`: a step depends on itself through others; `missing`: prerequisites the plan does not hold; `conflicts`: `{steps, paths}` for two steps whose write scopes overlap) and `steps` in plan order:

| Field | Meaning |
|---|---|
| `step`, `key`, `title`, `kind`, `provider` | As in `PlanRun`; `key` and `title` `null` for an imported plan |
| `depends_on`, `wave` | Prerequisite keys; 1 for a step without prerequisites, else one more than its latest prerequisite (`null` on a cycle) |
| `write_scope`, `baseline`, `tests`, `command` | What the approved validation plan declares for the step |
| `state`, `detail` | The run's step state and detail; without a run, from the step's newest attempt: `Waiting` (none), `Running`, `Validating`, `Review` (passed, not decided), `Accepted`, `Failed`, `Cancelled` |
| `eligible` | With a run, the step is `Ready`; without, it has no attempt and no blocker |
| `attempts` | Oldest first: `attempt`, `ordinal`, `outcome` (or `null` while live), `started_at_unix_ms`, `ended_at_unix_ms`, `validation` (`NotRun`, `Unknown`, `Passed`, `Failed` or `null` without a candidate), `accepted` |
| `blockers` | `{kind, reason}`: `Approval` (the revision is not approved), `Question` (an open blocking question about the step), `Dependency` (prerequisites not accepted or integrated), `Step` (the run's detail of a blocked, held, conflicting, refused or failed step) |

### AttemptDetail

`AttemptDetail {attempt}` returns `null` for an attempt the core does not hold, else `attempt`, `step`, `key`, `title`, `plan`, `feature`, `feature_name`, the 1.7 fields of `Attempts` (`ordinal`, `repository`, `repository_name`, `project`, `project_name`, `validation`, `activity`) and:

| Field | Meaning |
|---|---|
| `provider` | Of its newest launch, or `null` |
| `session` | The provider session its stream identified, or `null` |
| `process` | The root process `{pid, pgid}`, or `null` before one was identified |
| `heartbeat_at_unix_ms` | Its newest scheduler heartbeat |
| `worktree` | `{root, base, state, snapshot}` of its owned worktree, or `null` |
| `candidate_head` | The commit of its sealed candidate, or `null` |
| `continues`, `continued_by` | The attempt it continues after a recovery and the one that continues it |
| `claims` | At most 64 `{resource, mode, state}` it holds or held |
| `write_scope`, `baseline`, `tests`, `command` | What its step's approved validation plan declares |
| `events` | At most 64 events about it, oldest first: `seq`, `at_unix_ms`, `kind`, `summary` (the 1.7 sentence) |
| `validations` | At most 64 host validation receipts, oldest first: `ordinal` ("V-n"), `receipt`, `at_unix_ms`, `candidate`, `base`, `validator`, `passed`, `reasons`, `report` (the report's evidence digest), `approvals` (the approval bundle it was bound to) |
| `executable` | `{version, sha256, profiled}` of the executable it ran, or `null` |
| `evidence_records` | How many evidence records it has |

`crates/jojo-storage/tests/r01_frames.rs` sends its newer-than-helper Hello at minor 10 ("hello 1.10 => 1.9") and pins seven more answers at its end (230 in all).

## Protocol 1.10: push and draft pull request (M5)

Protocol minor 10 adds one query and four commands for the push of a finished plan branch and the draft pull request of a pushed one (schema 16: `external_actions`, `external_action_decisions`, `external_action_observations`, each action an `outbox` effect). The helper's `MINOR` is 10; Hello for a frame of minor ≥ 10 lists `RequestExternal AuthorizeExternal InspectExternal ResolveExternal` after `ControlPlan` and `ExternalActions` after `AttemptDetail`. A frame of minor ≤ 9 naming any of them is answered `UnknownType`. Each payload has exactly its fields. The storage side is `crates/jojo-storage/src/external.rs`; the one surface that reaches a remote is `crates/jojo-storage/src/remote.rs`; the helper side is `crates/jojo-helper/src/service/external.rs`.

The life of an action: `RequestExternal` records it (`NOT_ISSUED`) and performs nothing. `AuthorizeExternal` decides it on its exact payload digest; an authorization re-checks the run and issues the action in the same transaction (`ISSUED_UNKNOWN`), and only then does one worker thread run `git push` or `gh pr create --draft` once and record what it saw (`COMMITTED` → `CONFIRMED`, `REJECTED` → `REJECTED`, `UNKNOWN` kept). A replayed command answers from its receipt (`"dispatch": "not repeated"`); a restarted helper never runs an issued action again. `InspectExternal` reads the remote (`git ls-remote`, `gh pr list`) and records what it shows; it sends nothing. `ResolveExternal` lets the human mark an inspected action whose outcome stays unknown as not performed (`REJECTED` by `human:not-performed`); a new request may then follow, with the next sequence number.

| Request | Payload | Answer |
|---|---|---|
| `ExternalActions` (query) | `{plan}`: an identity or `null` for every plan | `runs` (at most 16, newest first): `plan`, `revision`, `feature`, `feature_name`, `repository`, `branch`, `base`, `head`, `state` (as in `PlanRun`), `push` and `pull_request` (the newest action's key or `null`), `can_push`, `push_refusal`, `can_open_pull_request`, `pull_request_refusal`. `actions` (at most 32, newest first): `action` (the operation key, `external:push:<plan>:<commit>:<n>` or `external:pr:…`), `kind` (`Push`, `PullRequest`), `plan`, `revision`, `sequence`, `feature`, `feature_name`, `repository`, `remote`, `url`, `branch`, `commit`, `base`, `title`, `body` (only while `Requested`, at most 4096 characters), `push`, `digest` (what an authorization binds), `state` (`Requested`, `Declined`, `InFlight`, `Unknown`, `Committed`, `Rejected`), `decision`, `requested_at_unix_ms`, `decided_at_unix_ms`, `outcome`, `detail`, `number`, `pull_request_url`, `observations` (`observer`, `status`, `detail`, `at_unix_ms`). `auth`: `git` and `gh`, each `{state, detail}` with `state` `Ready`, `Missing`, `Checking` or `NotConfigured`; read on a background thread at most once a minute |
| `RequestExternal` | `{plan, kind}`; `kind` `Push` or `PullRequest` | A receipt whose `response` is the operation key. Refused: `RemoteNotConfigured`, `ExternalRefused: …` (the run is not `DONE`, a request of the plan waits, the branch is pushed, the pull request is open, the branch is not pushed yet, origin is missing, an SSH URL or carries credentials), `GhMissing`, `GhChecking`, `RemoteUnreachable` (a pull request's base, the remote's `HEAD`, could not be read), `UnknownKind` |
| `AuthorizeExternal` | `{action, digest, decision}`; `decision` `Authorize` or `Decline` | A receipt (`Push authorized; issued once`, `PullRequest declined`) and, for an authorization, `dispatch`: `started`, or why it did not start (then recorded as not performed). Refused: `StaleExternal` (another digest, or the plan branch moved), `ExternalDecided`, `UnknownExternalAction`, `expected an external action` |
| `InspectExternal` | `{action}` | A receipt and `inspection: started`. Refused: `ExternalSettled`, `ExternalInFlight` |
| `ResolveExternal` | `{action}` | A receipt (`marked not performed`). Refused: `InspectFirst` (no inspection recorded an unknown outcome), `ExternalSettled`, `ExternalInFlight` |

A push goes to the URL of the repository's `origin` (HTTPS without credentials in it, or a local path), as `<commit>:refs/heads/jojo/plan/<plan>`, with `--porcelain --no-verify`, never forced. A pull request is `gh pr create --draft --repo <host/owner/name> --base <the remote's default branch> --head <plan branch> --title … --body-file -`; its title and body are built by the core from the approved requirements (title, goal, acceptance criteria), each step (provider, host validation receipt, commit) and the plan's provenance; no model writes them. Both programs get a cleared environment with only the user's HOME, `PATH` (`/usr/bin:/bin` and the directory of `gh`), `LC_ALL=C` and their no-prompt variables, so git's credential helper and gh's own sign-in are used as they are; no token is read, passed or stored.

At minor ≥ 10 the `Documents` answer also carries `external` (at most 16 requests that wait for a decision: `action`, `kind`, `plan`, `feature`, `feature_name`, `branch`, `commit`, `base`, `title`, `requested_at_unix_ms`) and counts them in `pending`. A frame of minor ≤ 9 gets the 1.9 answer unchanged.

The macOS client sends `Documents` at minor 10 after every snapshot (the sidebar counter), so the GUI scenarios admit it there (`OVERVIEW_MINOR`); it sends `ExternalActions` only while Source Control, the Approval Inbox or the Recovery Center is the shown screen, and the four commands only on the human's click. `r01_frames.rs` sends its newer-than-helper Hello at minor 11 ("hello 1.11 => 1.10") and pins seven more frames at its end (237); `p2_02_pipe` checks a later minor at 11.

## Protocol 1.11: Projects, Recovery Center and Activity & Audit (step 6)

Protocol minor 11 adds two read-only queries and two fields of every event; no schema change (schema 16), nothing is stored. The helper's `MINOR` is 11; Hello for a frame of minor ≥ 11 lists `RecoveryDetail ProjectDetail` after `ExternalActions`. A frame of minor ≤ 10 naming them gets `UnknownType`.

| Request | Payload | Answer |
|---|---|---|
| `RecoveryDetail` (query) | `{}` | The recovery inspection the helper holds (the one `RecoveryInspect` last made, or the one made at start): `digest` (the one `RecoveryResolve` binds), `inspected_at_unix_ms` (on the engine's clock), `counts` (`examined`, the subjects; `worktrees`, those whose launch was issued; `worktrees_intact`; `snapshot_mismatches`, a worktree that no longer matches its recorded candidate; `unknown_effects`, effects of the subjects plus external actions whose outcome is unknown; `stale_approvals`; `evidence_gaps`; `claims`, live claims of earlier sessions whose holder is not a subject). `subjects` (at most 16): `attempt`, `label` ("T4 · attempt 2"), `key`, `title`, the 1.7 attempt fields when the attempt has a step (`ordinal`, `repository`, `repository_name`, `project`, `project_name`, `validation`, `activity`), `feature`, `feature_name`, `stage` (`NotIssued`, `IssuedWithoutIdentity`, `Spawned`, `Concluded`, `Sealed`), `processes` (`label`, `finding` `Absent`/`Alive`/`Reused`/`Unknown`, `detail`), `worktree` (`state` `Intact`/`NotCreated`/`Changed`/`Problem`, `detail`), `snapshot` (`state` `Matches`/`Mismatch`/`None`, `digest`), `evidence_gaps`, `stale_approvals`, `unknown_effects`, `session` (`state` `Recorded`/`NotRecorded`/`Unsupported`, `detail`), `claims` (`resource`, `mode`, `state`), and the 1.1 assessment fields (`blockers`, `blockers_complete`, `process_absent`, `choices`). `claims` (at most 16): `resource`, `mode`, `holder_kind`, `holder`, `holder_label`. `settled` (at most 16, newest first): the resolutions of this session, `attempt`, `label`, `choice` (`VALIDATE_OUTPUT`, `CONTINUE_WITH_HISTORY`, `CONTINUE_FRESH_CONTEXT`, `HOLD`), `continued` (a continuation plan was recorded), `at_unix_ms`. Refused: `InvalidPayload` for any field |
| `ProjectDetail` (query) | `{project}` | `null` for an unknown project; else `project`, `revision`, `name`, `repositories` (per repository of the newest revision: `repository`, `name`, `path`, `branch` (`null` when HEAD is detached), `head`, `origin` (the configured URL without a user name or password; nothing is read from the remote), `origin_kind` (`Https`, `Ssh`, `File`, `Other`), `worktrees` (owned by the core in it), `active_worktrees` (of attempts without a sealed result), `healthy`, `issues`) and `missing` (`repository` or `null`, `text`: no test command, no CI command, no origin, an SSH origin). Git is read after the read snapshot is released, with the same local Git as worktrees (`rev-parse`, `symbolic-ref`, `config`) |

At minor ≥ 11 an event of `Events` also carries `category` (`Session`, `Attempt`, `Recovery`, `Validation`, `Decision`, `Approval`, `Git`, `Evidence`, `Chat`, `Document`, `Plan`, `Setup`, `Error`, `Core`; from its type, `features.rs` `event_category`) and `project` and `project_name` (the project of the first identity it records that leads to a registered repository: the subject of a decision or document version of the event, its aggregate, then each payload field; `null` when none does). A frame of minor ≤ 10 gets the 1.10 answer unchanged. No hash is stored, so no chain is reported.

The macOS client sends `Events` at minor 11 (Activity & Audit and the Dashboard, which reads events at launch, so the GUI scenarios admit `Events` at 11: `OVERVIEW_MINOR`), `RecoveryDetail` only while the Recovery Center is shown, `ProjectDetail` only while Projects is shown, `EventDetail` (minor 1) only when the human asks for the rest of a long payload. `r01_frames.rs` sends its newer-than-helper Hello at minor 12 ("hello 1.12 => 1.11", the two names) and pins six more frames at its end; `p2_02_pipe` checks a later minor at 12.

## Protocol 1.12: Onboarding and New Project (step 7)

Protocol minor 12 adds one command, one query and two fields; schema 17 (`brief_turns`, `brief_turn_results`, `brief_turn_stops`, `brief_executables`, `project_briefs`). The helper's `MINOR` is 12; Hello for a frame of minor ≥ 12 lists `BriefTurn` after `ResolveExternal` and `Workspace` after `ProjectDetail`. A frame of minor ≤ 11 naming them gets `UnknownType`.

| Request | Payload | Answer |
|---|---|---|
| `BriefTurn` (command) | `{repository, provider, message}` | The receipt with `turn` (the command identity) and `dispatch` (`launched`, `not repeated`). One read-only lead turn in the registered repository on `Codex` or `ClaudeCode`; `message` (at most 16 KiB) is what the human brings. Refused like a chat turn: `TurnRefused: another lead turn is running` (a chat or brief turn runs), `CapacityFull`, `AdmissionClosed`, `TurnRefused: the repository is not registered`, `UnknownProvider`, `InvalidPayload`. `CancelTurn` stops a brief turn too |
| `Workspace` (query) | `{}` | `root` (the disposable root, or `null` without a slice configuration), `problem` (why it cannot be used, or `null`), `repositories` (at most 64, by path: `path`, `name`, `stack` (words from the manifests found), `repository` (the identity when registered, else `null`)): the Git working directories one or two levels below the root; hidden directories are skipped |

At minor ≥ 12 each repository of `ProjectDetail` carries `brief`: `{brief, turn}`. `brief` is `null` or the newest revision of the repository's brief: `document`, `revision`, `approval` (as `Documents`), `summary`, `rows` (`key`, `text`, `sources`, `warning`), `test_command` (a list or `null`), `created_at_unix_ms`. `turn` is `null` or the newest brief turn: `turn`, `provider`, `state` (`Running`, `Stopping`, `Replied`, `Failed`, `Cancelled`, `Interrupted`), `detail`, `started_at_unix_ms`, `ended_at_unix_ms`. At minor ≥ 12 `Documents` lists briefs as kind `Brief` with the title "Project brief: <repository>" and counts a pending one in `pending`; `Document`, `Decisions` and `ReviewBundle` treat a brief like any document (`HumanDecision` approves it; it has no upstream). Events: `BriefTurnAccepted`, `BriefTurnStopRequested`, `BriefTurnFinished` (category `Setup`), `ProjectBriefCreated` (`Document`).

The macOS client sends `Documents` at 12 after every snapshot (so `OVERVIEW_MINOR` of `gui_p2_02.py` and `gui_p2_03.py` admits `Documents` at 12), `ProjectDetail` at 12 while Projects or New Project is shown, `Workspace` and `BriefTurn` only from New Project, and `RegisterRepository` (1.1) from New Project's folder choice and Start Reading. `r01_frames.rs` sends its newer-than-helper Hello at minor 13 and pins six more frames at its end (249); `p2_02_pipe` checks a later minor at 13.

## Protocol 1.13: a held plan step starts again on the human's word (M6)

Protocol minor 13 adds one command and two fields of `PlanDetail`; schema 18 (`plan_step_retries`, `plan_step_retry_refusals`). The helper's `MINOR` is 13; Hello for a frame of minor ≥ 13 lists `RetryPlanStep` after `BriefTurn`.

| Request | Payload | Answer |
|---|---|---|
| `RetryPlanStep` (command) | `{plan, step, attempt, action, note}` | The receipt; `response` is `repair recorded` or `restart recorded`. `attempt` is the step's newest attempt as shown (`null` when it never started); `action` is `Repair` (a new attempt told the failure and the note) or `Restart` (a fresh attempt from the plan branch as it is now, told only the note); `note` may be empty, at most 4096 characters. Refused: `UnknownAction`, `UnknownPlanRun`, `UnknownStep`, `StaleRetry` (the step has another newest attempt), `RetryRefused` (the step does not allow the action, or the run is cancelled), `NoteTooLong`. The runner starts the retry like a repair, under the identity recorded with it; if its start is refused, the step is `Refused` and may be restarted again; if the attempt fails, the step is held again, never repaired on its own |

At minor ≥ 13 each step of `PlanDetail` carries `retry`: `{actions, attempt}` (`actions`: what the human may do now, `Repair` (a held step whose failure is known) and `Restart` (a held, conflicting or refused step), none in a cancelled run or without a run; `attempt`: the step's newest attempt, to send back) and `retried` (`Repair`, `Restart` or `null`: the retry the newest attempt is, or will start as). The event `PlanStepRetried` (category `Decision`) reads "… will be repaired on your word" or "… will start again on your word".

The macOS client sends `PlanDetail` and `RetryPlanStep` at 13, the latter only from Execution Plan's inspector after a confirmation, and `Log` (1.1) every two seconds only while that inspector shows a running step.

## Protocol 1.14: follow-ups (choices, sessions, repairs, advisory reviews, counters)

Protocol minor 14 adds three commands and fields of four answers; schema 19 (`question_choices`, `lead_turn_continuations`, `lead_turn_sessions`, `lead_turn_repairs`, `advisory_reviews`, `advisory_review_results`, `advisory_review_stops`, `advisory_review_executables`). The helper's `MINOR` is 14; Hello for a frame of minor ≥ 14 lists `RepairTurn ReviewAttempt ReviewDocument` after `RetryPlanStep`.

| Request | Payload | Answer |
|---|---|---|
| `RepairTurn` (command) | `{turn, note}` | The receipt with `turn` (the new turn, the command's identity) and `dispatch`. The failed or interrupted turn must be the newest of its feature and not repaired before; the new turn has its kind, repository, provider and message, and its prompt carries "# Your previous attempt at this turn failed" with the core's reason and the note (at most 4096 bytes, may be empty) before its instructions. Refused: `UnknownTurn`, `RepairRefused: …`, and every refusal of `ChatTurn` |
| `ReviewAttempt` (command) | `{attempt, candidate}` | The receipt with `review` (the command's identity) and `dispatch`. The provider that did not do the attempt reviews its sealed candidate read-only, in the registered repository at its HEAD, with the step review prompt of M4. Only an attempt started by hand of a plan a lead drafted; a plan run reviews its own steps. Refused: `ReviewRefused: …` (not ended, another candidate, a plan run's step, not a lead plan, another review running), `AdmissionClosed`, `CapacityFull` |
| `ReviewDocument` (command) | `{document, revision}` | The receipt with `review` and `dispatch`. The provider that did not draft it reviews one revision of a requirements document, architecture decision or execution plan a lead drafted (`review::document_review_prompt`, the step review's verdict shape). Refused as `ReviewAttempt` |

An advisory verdict approves nothing, rejects nothing and holds nothing back; one review of either kind runs at a time and takes one unit of capacity. Events: `LeadTurnRepairAccepted`, `AdvisoryReviewAccepted`, `AdvisoryReviewFinished`, `AdvisoryReviewStopRequested`.

At minor ≥ 14:

- `Documents` carries `steps_waiting` (plan steps in review, held, conflicting or refused, in the newest run of every plan) and `open_questions` (`[{feature, count}]`: feature and step questions without a human's answer). Neither is part of `pending`.
- Each turn of `FeatureChat` carries `choices` (one list per entry of `questions`, in its order: none, or two to four options the lead named), `session` (`Fresh`, `Continued`, `Fallback`, `Continuing` while a continuation runs, or `null`), `repairs` and `repaired_by` (turn identities or `null`) and `repairable`.
- `AttemptDetail` and `Document` carry `advisory`: `{review, available, reviewer, reason}`; `review` is `null` or `{review, provider, state, notes, objection, detail, revision, requested_at_unix_ms, finished_at_unix_ms}`.

Session continuation (no frame of its own): a chat turn keeps its provider session (Claude Code without `--no-session-persistence`, Codex without `--ephemeral`). The next chat turn of the feature on the same provider, after a turn that replied and reported its session, is accepted with a continuation prompt (`planner::continued_prompt`) and runs first with `--resume <session>` (Claude Code) or `resume <thread> -` after the options (Codex). A continuation that ends on its own without a reply is followed in the same worker by the fresh run of the stored prompt (`Fallback`); a stop, an unconfirmed stop or lost evidence is settled as it is. Review and brief turns stay unsaved.

The macOS client sends `Documents`, `FeatureChat`, `Document` and `AttemptDetail` at 14, `RepairTurn` only from Feature Chat after a confirmation, `ReviewAttempt` from Verification & Evidence and `ReviewDocument` from Specifications and the Approval Inbox.

## Payload records (R-04 step 1, schema 20; protocol still 1.14)

No request or answer changes shape. The `payload` an event carries (`Events`, `EventDetail`) and the payload of a provider event (`Log`) is a versioned record since schema 20: a JSON object with its keys in order and `"v":1`, for example `{"attempt":"…","outcome":"SUCCEEDED","reason":"…","v":1}`. Before, it was text the core formatted in its own way (`a|b|c`, or a Rust `Debug` rendering). A client shows the text as it is and parses nothing from it; the sentence, category and project of an event stay the core's own fields.
