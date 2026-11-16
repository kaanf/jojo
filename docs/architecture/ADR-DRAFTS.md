# Jojo — ADR drafts for D01–D10

Status of **every ADR below: DRAFT — awaiting explicit human approval**. Latest user constraints are recorded as confirmed requirements, not as retroactively accepted ADRs. This document refines [Stage 1 open decisions](OPEN_DECISIONS.md); [Phase 0 review](../planning/PHASE_0_REVIEW.md) explains superseded proposals. Acceptance criteria are requirements for future tests, not results already obtained.

## D01 — Core and process architecture

**Confirmed:** native SwiftUI, Rust modular monolith, separate local helper, single-user/local-first. Those choices are settled by the user.

**Proposed detail:** one helper owns one SQLite database, scheduler and child process registry; multiple concurrent provider invocations are async worker tasks/process trees. One writer serializes state transactions, not agent execution. Keep logical modules for domain, storage, scheduler, providers/processes, workspace, evidence and IPC in a small Cargo workspace. No distributed broker or microservices.

**Alternatives/trade-offs:** embedded Rust/UniFFI shares GUI failure/lifetime and violates the separate-helper constraint; cloud workers add unrelated infrastructure; a per-run daemon improves reattachment but adds lifecycle complexity not required by stop-on-close. Separate helper adds IPC/versioning tests but prevents GUI ownership of provider state.

**Acceptance:** no GUI direct database writes; OS singleton lock plus engine epoch prevent competing helpers; fake Claude and Codex jobs in independent repos can overlap at configured capacity two; one slow job never blocks cancellation/state transactions. Dependency: D03/D09. Open item: U01 foundation scope, not the core language/process choice.

## D02 — UI/helper transport and close ownership

**Confirmed:** no background requirement; active-work close requires confirmation, Cancel continues work, Confirm safely stops and preserves state, reopening offers validated continuation.

**Proposed detail:** one private inherited stdio control channel, versioned bounded JSONL envelopes, helper stderr for diagnostics, logs fetched by cursor in bounded chunks. No provider inherits the control descriptors. UI disappearance/EOF starts emergency stop, not unattended background work. Follow the close-ticket protocol in [SHUTDOWN_RECOVERY.md](SHUTDOWN_RECOVERY.md). Helper exits after durable shutdown; GUI terminates after helper acknowledgement and observed helper exit.

**Alternatives/trade-offs:** Unix socket permits a replacement UI to reconnect to a live helper; defer until that requirement exists. XPC/LaunchAgent adds signing/shim/lifetime work; defer. TCP/WebSocket adds listener security. Stdio cannot reconnect a new process to old pipe handles; reopening uses durable recovery after ensuring old helper absence.

**Acceptance:** no-active quit has no confirmation; no launch races the close ticket; cancel-close sends no stop; confirm-close stops all workers; missing acknowledgement blocks a clean close and shows error; protocol mismatch blocks mutation. U02 defines window-close mapping; future sockets must retain epochs/idempotency/snapshot cursors rather than reinterpret old pipe requests.

## D03 — SQLite authority, immutable records and local artifacts

**Confirmed:** SQLite authoritative storage; preserve execution state/logs/evidence/worktrees.

**Proposed detail:** mutable revisioned projections, append-only artifact versions/decisions/events/results; one writer queue with short transactions. File blobs are private and content-addressed, logs append until sealed. State transition + event + outbox/claims commit together. Use foreign keys, required unique constraints, explicit durability policy, a maintained WAL-capable SQLite release and consistent backup/restore. Working Markdown is an edit/import surface, not approval authority.

**Alternatives/trade-offs:** file-only state loses atomic admission across steps/resources; Postgres service is unnecessary; full event sourcing adds replay/migration burden. SQLite transactions cannot atomically include a process launch or filesystem write: journal gaps and reconcile.

**Acceptance:** concurrent duplicate admission creates one attempt; rollback leaves no partial claims/events; final result cannot be updated; active-DB backup restores manifests; no committed durable log cursor exceeds flushed bytes; corrupted/missing evidence cannot satisfy a gate. U03 covers storage root/retention defaults. Logical model: [CORE_STATE_MODEL.md](CORE_STATE_MODEL.md).

## D04 — Official CLI provider boundary

**Confirmed:** both Claude Code and Codex, user-installed official binaries, native subscription login, no credential extraction/proxying, no API-key fallback or automatic paid extra usage.

**Proposed detail:** provider-neutral execution contracts; separate process/session per Jojo attempt; capability/version checks; bounded structured output; explicit session ID for continuation; CLI owns auth. First foundation uses fakes for both provider identities. Real adapters enter a separately approved later milestone, Codex first as a proposed order only.

**Alternatives/trade-offs:** embedded SDK/internal agent engine increases runtime and auth coupling; custom OAuth/proxy violates constraints; pretending CLI capabilities are identical hides permission and interrupt differences. External CLIs require release-specific conformance fixtures and supported-version checks.

**Acceptance:** auth unknown blocks; key/provider-routing environment is not silently inherited; no credential-file/Keychain reads; unsupported input/interrupt returns explicit capability error; terminal output and process exit are separate; simultaneous attempts cannot mix session IDs/output. U06 gates live integration, U07 provider order. See [PROVIDER_CONTRACTS.md](PROVIDER_CONTRACTS.md) and Stage 1 [authentication qualifications](../research/LICENSING_AND_SECURITY.md#official-cli-authentication-boundaries).

## D05 — Canonical artifacts and dependency schema

**Confirmed:** complete feature PRD/use cases before development; explicit dependencies; PR-open alone never satisfies a dependency.

**Proposed detail:** one typed Jojo schema with stable IDs and immutable revisions, rendered as Markdown; two distinct graphs (artifact prerequisites and executable step dependencies) sharing graph validation. Edge direction is `dependent requires prerequisite`. Plan revision contains node/edge revisions, validation requirements, permission/resource envelope and source bindings. Adapt selected Spec Kit/OpenSpec content with provenance, without full format compatibility.

**Alternatives/trade-offs:** full Spec Kit/OpenSpec interoperability adds parsers, round-trip and status compatibility; Markdown existence/checklists do not prove approval. A generic arbitrary predicate DSL is unnecessary: use a closed, versioned typed predicate set with explainable tri-state results.

**Acceptance:** placeholders/unresolved blocking questions prevent PRD approval; duplicate/missing/self/cyclic graph references rejected transactionally; isolated nodes supported; unknown/deleted/stale facts block; opened PR cannot produce any eligible edge. Proposed schema details require U01 approval; graph contract: [CORE_STATE_MODEL.md](CORE_STATE_MODEL.md).

## D06 — Human approvals and invalidation

**Confirmed:** human approval of PRD, ADR and execution plan. Model output cannot grant approval.

**Proposed detail:** explicit review bundle binds artifact revision/hash, upstream approval decision IDs and plan graph digest. User command includes expected revisions and displayed bundle digest. Immutable approval/rejection/revocation decisions; current validity derived from current revisions and revocations. Any bound-content change invalidates that decision's applicability and dependent readiness; no cosmetic exception initially. A material edit during execution requests stop and marks output ineligible pending re-review.

**Alternatives/trade-offs:** approved filenames/checkboxes are mutable and weak; semantic-only change detection reduces review churn but is difficult to prove; automatically accepting generated ADRs violates the user requirement. Conservative hashes can over-invalidate; accept that for the foundation.

**Acceptance:** stale UI approval fails CAS; changing PRD invalidates downstream ADR/plan applicability; cancellation/revocation outranks retry backoff; historical decisions/results remain; agents have no approval command channel. U04 decides conservative invalidation policy. Architecture drafts here are not automatically application-feature approvals.

## D07 — Parallel resource and worktree safety

**Confirmed:** future multiple concurrent Claude/Codex processes, configurable limits, independent worktree ownership, appropriate per-repo locking, independent repo concurrency, separate attempts/immutable results. Initial concurrency may be one.

**Proposed detail:** transactional durable reservations and resource claims; global/provider/repository capacities plus exclusive worktree/device/output claims and shared/exclusive repository activity gate. Default global one, each provider one, each repo one. Test capacity greater than one immediately. Deterministic round-robin fairness across eligible repo queues with bounded priority aging. Trusted local repositories and scoped CLI permissions; no bypass default. Locks coordinate Jojo, not arbitrary same-user programs.

**Alternatives/trade-offs:** global exclusive run lock is fundamentally single-agent; per-repo exclusive lock for an entire invocation prevents same-repo scaling; worktree-only locks ignore common Git metadata/caches; distributed leases do not make detached processes safe. Conservative same-repo policy is configurable without schema changes; admin operations may wait for running repo activity to end.

**Acceptance:** all-resource atomic acquisition, no hold-and-wait; unrelated repositories overlap at cap two; distinct pre-provisioned worktrees can overlap when per-repo cap permits; common-dir aliases share safety gates; expired/unknown process claims remain quarantined; lowering cap does not kill jobs or oversubscribe new work. U05 covers trust model and detailed defaults. See [PARALLEL_SCHEDULER.md](PARALLEL_SCHEDULER.md).

## D08 — First implementation milestone

**Confirmed:** documentation only now; no inference, remote creation/deployment, or touching `agentic-factory`.

**Proposed detail:** Phase 1 is an offline architecture foundation: durable domain/approvals, deterministic scheduler for both provider identities, fake cooperative/hostile-behavior test processes, local disposable Git fixtures, evidence and a minimal native lifecycle/review UI. It refines the Stage 1 kernel phase by bringing concurrency/shutdown safety tests forward; it is not a live software-generation demo. Later provider inference and real workspace writes need their own scope approval.

**Alternatives/trade-offs:** kernel-only cannot demonstrate correct UI shutdown; first real coding demo obscures lifecycle bugs with model nondeterminism; full product design/Figma/deploy integration is too broad. Small SwiftUI shell is needed for quit/review correctness, not visual polish.

**Acceptance:** [Phase 1 backlog](../planning/PHASE_1_IMPLEMENTATION_BRIEF.md) passes offline, at capacity one and simulated capacities greater than one; quit/reopen preserves interrupted jobs; no built-in action can create a remote repo or deploy. U01 is the implementation-scope decision. First task after approval is P1-01, not an agent invocation.

## D09 — Stop, interruption and validated continuation

**Confirmed:** stop safely on confirmed close; preserve work; validate actual process/repository state on resume; no blind replay or exact-memory guarantee; expose stop failure.

**Proposed detail:** durable stop intent; capability-specific graceful interruption followed by process-tree termination and verified exit. Proposed budget: graceful interrupt up to 10 seconds, TERM up to 20 additional seconds, optional approved KILL then 5-second verification; timers are policy inputs, not hardcoded assumptions. Show graceful timeout/forced termination even when eventual stop succeeds. Unconfirmed stop leaves recovery required and claims quarantined; do not acknowledge clean shutdown. Reopen paused; continuation is a new attempt linked to history and optionally an exact supported provider session.

**Alternatives/trade-offs:** immediate SIGKILL loses more evidence and may strand effects; detached always-on supervisor contradicts current lifetime need; blind session resume can repeat operations or target changed files. No signal sequence guarantees termination of all hostile descendants. External completed effects require observation even if the local process is gone.

**Acceptance:** stop request never means stopped; success racing cancel/shutdown has deterministic ordering; session unavailable falls back only to user-approved fresh context; dirty work retained; PID reuse/escaped child/missing log/disk failure visible; no new writer until recovery resolution proves safe. U05 includes escalation/budgets; details in [SHUTDOWN_RECOVERY.md](SHUTDOWN_RECOVERY.md).

## D10 — Remote actions and effect policy

**Confirmed:** no remote repository creation or deployment in this work. Stage 1 also excludes destructive edits to existing projects.

**Proposed detail:** Phase 1 has no remote mutation adapter or live SCM polling; local/fake observations only. Treat future remote operations as journaled effects with `not_issued`, `issued_unknown`, `confirmed`, `rejected` states and explicit permission scopes. No generic outbox replay for issued-unknown launch or remote mutation. Auto-merge remains off; no deletion-based compensation.

**Alternatives/trade-offs:** auto-push/PR/deploy inside agent prompts bypasses review boundaries; read-only observation can be useful later but introduces network/auth dependence into foundation tests; at-least-once retries alone can duplicate side effects.

**Acceptance:** remote-action commands rejected before effects in Phase 1; fake “operation completed, acknowledgement lost” blocks replay; PR-open metadata alone never unlocks work. Future GitHub writes/deployment approvals remain U08, outside Phase 1.

## Human approval register

| ID | Decision needed | Recommendation | When blocking |
|---|---|---|---|
| U01 | Approve Phase 1 scope and schema/ADR implementation direction | Offline foundation and minimal lifecycle/review UI, P1-01–P1-17; no inference/live provider execution | Before implementation; acceptance of an ADR draft must be explicit |
| U02 | What does closing the last Jojo window mean? | Last primary window invokes quit protocol; auxiliary windows close normally; Cmd-Q always invokes protocol | Before GUI lifecycle implementation |
| U03 | Storage/retention/backup defaults | Private Application Support root for DB/blobs, separate owned worktree root; no automatic deletion of interrupted/unknown work; fail/pause on quota | Before persistent storage policy is implemented; path naming can then be routine |
| U04 | Approval invalidation and affected active runs | Conservative bound-hash invalidation; stop affected runs, preserve results, require re-review | Before approval/scheduler implementation |
| U05 | Trusted-host resource defaults, fairness and stop escalation | Cooperative trusted repos; caps one initially; round-robin/aging; approved interrupt→TERM→KILL budget with visible escalation and unconfirmed-stop blocking | Before scheduling/process execution is implemented; numeric budgets tunable later |
| U06 | Supported real CLI versions/permission/auth capabilities and billing-mode assurance | Recheck official guidance and local supported non-inference status; block uncertain mode; use fakes now | Before any later live-provider milestone, not Phase 1 |
| U07 | Which real provider and sample repository first? | Codex first proposed; disposable sample; Claude parity before provider-complete claim | Before later live-provider milestone |
| U08 | Live Figma/SCM/deployment access, distribution and strong isolation | Separate scoped approvals and license/provenance checks; no implicit authorization | Later stages only |

Accepting confirmed constraints does not select the proposed timeouts, paths, fairness algorithm, schema compatibility or provider order. No repeated approval is needed for already-confirmed Rust/SwiftUI/SQLite/native-auth/stop-on-close constraints. User approval of this implementation brief must explicitly authorize implementation; this documentation assignment does not.
