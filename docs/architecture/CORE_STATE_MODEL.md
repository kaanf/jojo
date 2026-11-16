# Jojo — Core state and persistence model

Status: proposed implementation contract; confirmed constraints are identified in [ADR drafts](ADR-DRAFTS.md). No production schema or code is implemented. Related contracts: [scheduler](PARALLEL_SCHEDULER.md), [providers](PROVIDER_CONTRACTS.md), [shutdown/recovery](SHUTDOWN_RECOVERY.md).

## Identities and separate state dimensions

A feature is product intent. A plan revision contains executable step revisions. An attempt is one invocation to perform one step revision. A provider session is optional provider-owned conversation history, not an attempt ID, execution authority or process checkpoint. One logical agent role can have many sequential attempts; many roles/providers can run concurrently. There is no singleton `current_agent` or `current_task` field.

Stable IDs are opaque locally generated identifiers. Repository identity includes canonical Git common-directory identity (path plus verified filesystem identity) so linked-worktree/path aliases do not bypass locks. Two clones of the same remote are different local resources; future remote-ref writes also need a separate remote-resource key. Every attempt has its own worktree binding, output directory, process identity, event stream and eventual result.

Use four independent dimensions:

| Dimension | Authority and semantics |
|---|---|
| Human intent | Feature/step `RUN`, `HOLD`, `CANCEL`; default HOLD until approved and explicitly started. Session shutdown adds an admission gate without rewriting intent as permanent cancellation. |
| Artifact/approval applicability | Exact revision/hash and upstream decision bindings; valid, missing, rejected, revoked or stale. Files present is not approval. |
| Attempt lifecycle/outcome | Mutable lifecycle until sealed; immutable result afterward. Process death is separate from a successful provider result. |
| Step/feature eligibility | Derived from current plan, applicable approvals, selected validation/evidence, dependencies and recovery holds. Historical success may be inapplicable now. |

Feature UI stages are projections: `DISCOVERY`, `PRD_REVIEW`, `ARCHITECTURE_REVIEW`, `PLAN_REVIEW`, `READY`, `EXECUTING`, `VERIFYING`, `BLOCKED`, `NEEDS_REVIEW`, `COMPLETE`, `CANCELLED`. Do not encode every Cartesian combination as one giant mutable enum. A feature completion receipt records the accepted revision/evidence bundle; changing requirements creates a new assessment, not a rewrite of old completion history.

## Revisions, content and approval authority

Each mutable aggregate has a monotonically increasing integer `revision`; CAS uses `(id, expected_revision)`. Timestamps are audit data, never concurrency tokens. Immutable revisions carry `schema_version`, canonical content digest, rendered review digest, parent revision ID, creation event and provenance. Reject ambiguous serialization such as duplicate JSON keys; hash canonical typed data and preserve exactly what the UI displayed.

PRD includes goal/non-goals, actors/use cases (preconditions/main/alternate/error/postconditions), acceptance IDs, NFRs, assumptions and unresolved blocking questions. A minimal complete fixture can be human-authored; no inference is needed to exercise approval. ADR bundle binds the PRD revision. Plan binds PRD and ADR approval decision IDs, graph/step revisions, source inputs, permission envelope, validation plan and required output/evidence types.

Approval/rejection/revocation is an append-only human decision, with subject revision/digest, reviewed bundle digest, upstream approval IDs, client command ID and event sequence. Current validity is computed, not a mutable `approved=true` on an editable artifact. An explicit rejection/revocation takes effect immediately; a prior approved revision remains historical but cannot authorize current work.

All three approval gates must be applicable before **development** admission. Pre-approval discovery/authoring work, when added later, is a separately authorized step kind with a read-only artifact-production envelope; it cannot run development commands by pretending to be a planner. Phase 1 uses human-authored fixtures and fake jobs.

An approval command must match current revisions and the displayed review bundle. Model text, Markdown checkboxes, hooks and provider tool responses cannot invoke this command. Material edits invalidate dependent readiness transactionally and request stop for affected live attempts. Their output is preserved but cannot satisfy the new plan automatically. The conservative invalidation default is proposed in D06/U04.

## Attempt lifecycle

```text
RESERVED → STARTING → RUNNING ↔ AWAITING_INPUT
    │          │          │          │
    └──────────┴──────────┴──────────┴→ STOP_REQUESTED
                    │                       │
                    └──────────→ SETTLING ←──┘
                                     │
                                     ├→ FINISHED + immutable result
                                     └→ RECOVERY_REQUIRED

Any unsealed phase → RECOVERY_REQUIRED on unclean restart/ambiguous outcome.
RECOVERY_REQUIRED → SETTLING only through validated observations/resolution.
FINISHED has no outgoing lifecycle transition. Retry/continue creates a new attempt.
```

| Phase | Meaning / permitted transition conditions |
|---|---|
| RESERVED | Atomic durable claim exists; launch not yet issued. Release as NOT_STARTED only if dispatcher proves it never crossed issue boundary. |
| STARTING | Launch intent marked issued/unknown before spawn; no external call inside DB transaction. Failure known to occur before child creation can settle FAILED with `launch_failure`; uncertain child creation requires recovery. |
| RUNNING | Verified child identity registered; no success implied. Provider tools may spawn descendants. |
| AWAITING_INPUT | Provider awaits a supported question/tool approval. Process/resources remain occupied. No blanket permission grant. Answer returns to RUNNING; shutdown can interrupt. |
| STOP_REQUESTED | Durable stop reason, request sequence and escalation policy; process may still be alive. Cancel/backoff does not remove this state. |
| SETTLING | Process absence verified; drain/seal logs, inspect worktree and known effects, classify result. Slow/pending effects and evidence gaps remain explicit. |
| RECOVERY_REQUIRED | Process identity, termination, log durability, worktree or effect outcome uncertain. Preserve claims/quarantine as applicable; never auto-requeue. |
| FINISHED | Exactly one immutable result sealed. Worktree/evidence retained. Resource release still requires safety checks; unresolved shared effects may keep a recovery hold. |

`STOP_REQUESTED` can be recorded while a launch is in flight. Dispatcher rechecks it before spawning; if child already started, registry immediately routes it through stop. Repeated commands are idempotent and append no duplicate outcome. No `cancelled=true` shortcut releases resources.

## Immutable result and race rules

A result records attempt/step/plan IDs, frozen input/approval/resource-policy digests, source vector, candidate output hashes, provider version/session reference, process exit/signal/stop evidence, log/evidence manifest IDs, stop reason/escalation and known uncertainties. A unique result per attempt is enforced; no update/delete API. Evidence/results are immutable by application policy and hashes, not protected against a malicious unrestricted same-user process.

| Outcome | Required facts |
|---|---|
| SUCCEEDED | Applicable frozen inputs; explicit supported terminal success; acceptable process exit; required process tree gone; step-level output well-formed and durable. This is an invocation outcome, not independent feature verification. |
| FAILED | Known launch/execution/provider failure with process absence confirmed; diagnostics/evidence coverage recorded. |
| CANCELLED | Explicit user task-cancel intent wins ordering, process absence confirmed, preserved partial outputs. |
| INTERRUPTED | Shutdown/revocation/crash interruption, process absence confirmed; output and lost evidence/unknown effects explicitly recorded. Does not assert the code is unusable or unfinished in every respect. |
| NOT_STARTED | Durable reservation revoked and proven never issued; no provider process/effect occurred. |

Unknown termination is not a terminal outcome: keep RECOVERY_REQUIRED. Once process absence is proven, an INTERRUPTED result may record unresolved external-effect uncertainty; a separate recovery hold still blocks actions that could duplicate that effect. A later observation creates a recovery resolution or validation/acceptance assessment; it never changes this result to SUCCEEDED.

Without an intentional stop or crash, a process that exits without the required terminal record is FAILED with `protocol_incomplete` once absence and durable stream coverage are established. If missing records may instead reflect lost/unflushed evidence, retain RECOVERY_REQUIRED until the loss is classified; never infer provider success from an empty stream.

Linearization is the writer transaction order: if valid success is already finalized when stop commits, keep success. If stop commits before finalization, a later terminal success is evidence, but outcome is CANCELLED for task-cancel or INTERRUPTED for shutdown/revocation; preserve the candidate for inspection/independent validation. Simultaneous finalizers cannot create two results. Stop request itself is acknowledged separately from confirmed process exit.

Retry, repair and continuation create new attempt IDs and optional `predecessor_attempt_id`, `continuation_kind` and exact provider session ID. A step revision can have at most one unsealed attempt, while different steps run concurrently. Parallel speculative alternatives must be distinct approved step IDs/worktrees; do not bypass this uniqueness rule.

## Step projection and dependencies

Projection precedence: cancellation-in-progress or cancelled intent; recovery hold; stale/missing approval; unresolved prerequisite; capacity wait; live attempt phase; pending validation/review; satisfied accepted assessment. Render the specific blocker(s), not only a generic status. HOLD is displayed as paused even if underlying dependencies are valid.

Edge fields: immutable `edge_id`, dependent step revision, prerequisite step/output or artifact revision, `predicate_kind`, exact source/output/contract selectors, freshness policy and invalidation behavior. All required edges are AND-combined initially; do not introduce arbitrary scripts or a boolean expression language. Explicit alternative dependency paths require separately modeled approved steps later.

| Predicate | Evidence required |
|---|---|
| ARTIFACT_APPROVED | Current exact artifact/bundle and applicable human decision |
| OUTPUT_AVAILABLE | Selected immutable output with digest/source vector matching the plan; provider prose alone cannot select it |
| CODE_VERIFIED | Independent validation receipt for exact commit/dirty-snapshot, approved test plan and expected test inventory |
| PR_MERGED | Authoritative merge/source observation at the expected revision; never inferred from an agent URL |
| BUILD_AVAILABLE | Source→artifact digest binding and build evidence |
| DEPLOYED_HEALTHY | Exact artifact/source/environment plus current health/migration receipt |
| CONTRACT_COMPATIBLE | Versioned approved contract compatibility evidence for both sides |
| VALIDATION_PASSED | Required independent checks, source/environment/plan identities and complete evidence |

Evaluator returns `SATISFIED`, `UNSATISFIED` or `UNKNOWN`, with evidence IDs, observation versions, reason and expiry. Unknown kind/fact/failed read/stale evidence blocks. There is deliberately **no PR_OPENED satisfaction predicate**. A PR URL is informational metadata. Even OUTPUT_AVAILABLE needs a verified output identity and selection policy, not just an opened PR or agent claim.

Candidate source vectors can name several repositories; DAG order alone does not materialize code. A dependent same-repo step must be based on the selected predecessor output commit/snapshot (or an explicitly verified integration commit), not the old branch tip. A cross-repo test must bind backend artifact/contract and mobile build identities. No worker automatically cherry-picks, rebases or “uses latest” to make an edge pass.

Invalidate applicability when source, approval, plan or deployment receipt changes; preserve all prior results. Before admission and immediately before launch, evaluate current facts. After launch, relevant invalidation requests stop and excludes output from acceptance until re-reviewed. Phase 1 evaluates future deployment/SCM predicates against fakes only.

## Logical SQLite records and constraints

This is a logical data design; no DDL migration is being shipped.

| Record | Key fields / invariants |
|---|---|
| engine_sessions | session/epoch, OS singleton identity, admission gate, clean/unclean shutdown marker, schema/protocol version; history append-only |
| features / repositories | stable IDs, current revision/control intent; verified canonical paths/common-dir identity; remote metadata not local identity |
| artifact_versions | immutable ID/type/content/rendered digests, parent/source references, provenance; content not overwritten |
| clarification_questions / answers | stable question ID, feature/artifact context, blocking/deferred state; answers versioned with human/model provenance |
| decisions | immutable subject/bundle hash, approve/reject/revoke, superseded decision reference; no agent authority |
| plan_versions / step_versions / dependency_edges | immutable content/graph revision; FKs enforce known endpoints; graph validation rejects cycle/duplicate/self-edge; current-plan pointer revisioned |
| step_controls | RUN/HOLD/CANCEL, revision and command origin; controls cannot revive a sealed attempt |
| attempts | unique ID/ordinal per step revision, phase/revision, engine epoch/claim token, input vector, worktree binding, predecessor; unique unsealed attempt per step revision |
| process_records | attempt/process generation, PID/start identity/boot ID/PGID, executable/cwd evidence, children/exit observations; PID alone never identity |
| resources / resource_claims | canonical key, capacity or shared/exclusive mode, holder/epoch/generation, ACTIVE/QUARANTINED; exclusivity and counters checked in writer transaction |
| worktrees | verified repo/path/branch identity, owned marker, lifecycle, active owner; at most one writer, no shared mutable working directory |
| effects / outbox | unique operation key, payload digest, target/attempt, issue state and observation; issued-unknown not replayable automatically |
| attempt_results / validation_receipts | unique immutable result per attempt; independent validation references exact candidate/test plan; selection/acceptance is separate append-only assessment |
| observations / recovery_resolutions | observer identity, subject/revision, fact value, capture/expiry, resolution evidence; unknown and absence distinct |
| log_segments / evidence_manifests / blob_refs | stream generation/byte range/durable offset, hashes/coverage/truncation; manifest cannot assert nonexistent blob durability |
| commands / events | unique command ID plus request digest and stored response; ordered event sequence, aggregate revision/causation; bounded UI cursor reads |
| shutdown_operations / recovery_holds | close ticket/operation ID, phase, target set, stop acknowledgements/failures; holds survive restart independent of queue contents |

Projection updates may be compacted; facts/results remain inspectable under approved retention policy. This is not a requirement to reconstruct the entire database from an event log. Foreign keys cannot enforce every cross-record invariant: test writer transactions explicitly. Use checked counters/queries, not mutable UI counts, to admit work.

## Transaction and filesystem boundaries

Mutating command transaction: deduplicate ID/payload → check aggregate/config revisions and authority → validate invariants → update projections/revisions → append facts/events → add effects/claims → commit → respond. Reused ID with different payload is a conflict. Slow provider/Git/file I/O happens outside the write transaction and returns as tagged observations.

Outbox state: `NOT_ISSUED → ISSUED_UNKNOWN → CONFIRMED | REJECTED`; only a proven NOT_ISSUED action can be automatically dispatched after crash. Set ISSUED_UNKNOWN before crossing spawn/remote boundary. This deliberately leaves a crash-before-spawn ambiguity rather than assuming absent PID means nothing happened. Single helper ownership and per-attempt dispatch token stop duplicate concurrent consumers; epoch fencing protects DB writes, not an old child’s OS side effects.

Write complete blobs to owned temporary paths, flush, atomically rename and persist directory entry as supported, then commit manifest references. Orphan blobs are safe to sweep later; missing referenced blobs are not silently reconstructed as evidence. Append stdout/stderr bytes before committing durable consumed offsets; replay handles split UTF-8/JSONL and deduplicates `(attempt, stream_generation, offset)` normalization. Partial/unflushed tails are recorded as incomplete after crash. Proposed FULL synchronous policy/backup version selection belongs to D03, not a claim of immunity to hardware failure. SQLite permits one writer at a time ([transaction documentation](https://sqlite.org/lang_transaction.html)).

If disk/DB writes fail, close admission immediately and attempt best-effort process stop even if stop intent cannot persist. Expose unclean shutdown/evidence loss; do not send clean acknowledgement or free unknown writers. On restart, recovery precedes scheduler activation.

## Recovery and acceptance invariants

Reopening starts with admission closed. Reconcile old process identities, outstanding launch/effect intents, repo/worktree state, logs and approvals. An old result is not changed by a new engine epoch. Known-dead interrupted work can be continued only through a reviewed continuation plan; unknown-live work retains quarantine. Historical provider sessions are optional context, not proof of termination or completed operations.

Foundation tests must prove: no stale approval launch; no double admission; no result mutation; no claim release on timeout alone; no unknown predicate pass; correct input materialization; stop/success ordering; no old-epoch canonical writes; rollback and partial-log durability; interruption creates a new attempt on continuation. Exact tasks and deterministic oracles are in [Phase 1 brief](../planning/PHASE_1_IMPLEMENTATION_BRIEF.md).
