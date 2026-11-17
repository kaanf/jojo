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
