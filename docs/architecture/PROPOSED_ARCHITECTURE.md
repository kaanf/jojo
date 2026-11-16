# Proposed architecture

Status: **proposed, awaiting approval**. This document defines a local single-user system, not SaaS. Component actions C01–C37 and exact source/commit evidence are in [COMPONENT_EXTRACTION_MATRIX.md](../research/COMPONENT_EXTRACTION_MATRIX.md). Nothing has been implemented or remotely provisioned.

## Architecture recommendation

Use a **Rust modular monolith in a separate local helper process**, a native SwiftUI client, SQLite plus a local artifact store, and user-installed official execution/test CLIs. “Pure Rust core” means our domain/orchestration logic is Rust; it does not mean replacing the agent CLIs, Git, Xcode or mobile test drivers. Keep one authority for feature state and approvals.

Why: the hard problem is enforcing approved feature intent across artifact, code, deployment and evidence revisions. Optio's reconcile boundary is useful, but its cloud runtime, auth relay and dependency definitions are not our product. Backstage is larger than the bootstrap operations needed. A focused Rust implementation avoids maintaining translations of their service ecosystems. See source [`packages/shared/src/reconcile/types.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/types.ts), [`apps/api/src/services/reconcile-executor.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/reconcile-executor.ts), [`plugins/scaffolder-backend/src/scaffolder/tasks/NunjucksWorkflowRunner.ts`](https://github.com/backstage/backstage/blob/57111dcbf112e263688c03fd481315d7804a89e0/plugins/scaffolder-backend/src/scaffolder/tasks/NunjucksWorkflowRunner.ts).

Conceptual process layout (no new infrastructure service):

```text
SwiftUI app (views, user questions, approvals, artifact/evidence browser)
  ↕ versioned local command/query/event protocol
Rust factory helper (single state owner, durable scheduler and process supervision)
  ├─ SQLite: domain state, revisions, decisions, run attempts, observations, outbox
  ├─ local artifacts: PRDs/ADRs, design exports, logs, reports, screenshots
  ├─ worktree manager → installed Git / gh
  ├─ provider adapters → official claude / codex processes
  ├─ validator adapters → project tests / xcodebuild / Maestro / OSV-Scanner
  └─ deployment observer → approved provider-specific CLI or read-only receipt
```

The drawing is a process overview, not an approved deployment topology. Child CLIs are separate processes because they already own execution/auth; domain modules remain one application, one database and one scheduler.

## Module boundaries and source decisions

These are proposed logical Rust modules; crate names and libraries remain implementation choices.

| Module | Owns | Classification and source rationale |
|---|---|---|
| Domain/artifacts | Feature IDs, use cases, acceptance, artifact revisions, decisions, invariants | REWRITE with REUSE of Spec Kit content [`templates/spec-template.md`](https://github.com/github/spec-kit/blob/2ee2b1e8005905db05f5717300a469b3068b908f/templates/spec-template.md), [`templates/commands/clarify.md`](https://github.com/github/spec-kit/blob/2ee2b1e8005905db05f5717300a469b3068b908f/templates/commands/clarify.md). |
| Discovery | Persisted question/answer sessions, assumption resolution, PRD assembly | REWRITE, because prompt guidance is not native durable state. |
| Architecture | Option comparisons, recommendations, ADRs, approvals and invalidation | REWRITE, learning from real Spec Kit [`src/specify_cli/workflows/step/gate/__init__.py`](https://github.com/github/spec-kit/blob/2ee2b1e8005905db05f5717300a469b3068b908f/src/specify_cli/workflows/step/gate/__init__.py). |
| Graph/scheduler | Artifact graph, executable DAG, typed prerequisite predicates, resources | PORT bounded graph/readiness from [`src/core/artifact-graph/graph.ts`](https://github.com/Fission-AI/OpenSpec/blob/ea635dd255f8686dcf48626bbcbe0e3fd59b6067/src/core/artifact-graph/graph.ts) and [`src/core/artifact-graph/schema.ts`](https://github.com/Fission-AI/OpenSpec/blob/ea635dd255f8686dcf48626bbcbe0e3fd59b6067/src/core/artifact-graph/schema.ts); REWRITE product semantics. |
| Reconciler | Observe/decide/apply with explicit unknown/error, deadlines and intents | PORT C01 pattern; inject clock/jitter, one decision evaluator. |
| Storage/effects | SQLite transactions, migrations, event journal, leases, outbox | REWRITE C02/C36; replace upstream timestamp CAS with integer revisions. |
| Workspace/SCM | Repo bindings, owned worktrees, diff, bootstrap plan/journal, PR/CI facts | REWRITE C06/C14/C27; Git/gh EXTERNAL TOOL. |
| Providers/processes | CLI capability/auth status, launch/cancel, input/output normalization | Claude small parser PORT C08; Codex adapter REWRITE C13; CLIs EXTERNAL TOOL. |
| Design context | Export import, normalized nodes, revisions and feature mapping | Optional PORT C24; mapping REWRITE C26; live access conditional C25. |
| Verification/deployment | Approved validation plans, independent checks, environment predicates | REWRITE C33/C34 using EXTERNAL TOOL C29/C30/C32. |
| Evidence | Blob lifecycle, manifests, redaction, integrity and retention | REWRITE C35. |
| IPC/query projections | Versioned commands, events, snapshots and cursor resume | REWRITE C37; UI reads projections, never writes SQLite directly. |

Suggested dependencies to evaluate later: Tokio for async processes/IO, Serde/JSON for contracts, a maintained SQLite wrapper (rusqlite is a plausible default), tracing and a hashing library. These libraries were not source-audited here and are not pinned by this audit. Start with a single workspace and limited crates rather than a plugin framework. No embedded LLM SDK, vector database, Redis, Postgres server, Kubernetes or workflow platform is needed.

## SwiftUI-to-Rust integration options

| Option | Strengths | Cost / limitations | Recommendation |
|---|---|---|---|
| SwiftUI → Rust helper via stdio JSON messages | Tiny surface, explicit process boundary, no listening port, easy fake helper and logs | App owns launch/reattach handle; reconnect to the same running helper is awkward; messages need framing/backpressure | First vertical slice, bounded runs and explicit shutdown semantics. |
| Private Unix socket to Rust helper | Reconnect independent of GUI lifetime, no TCP service, same protocol can serve future headless client | Ownership/peer validation, stale socket and singleton handling, daemon lifecycle | Preferred durable local architecture once GUI reconnect is needed. |
| XPC service/LaunchAgent with Swift/Obj-C shim to Rust | Native macOS lifecycle and peer/signature integration | Additional shim/FFI/signing/packaging complexity; service lifetime must match execution policy | Evaluate for packaged app, defer prototype. |
| UniFFI Rust library called from Swift | Typed bindings, low serialization overhead, Rust shared domain logic possible | In-process FFI is **not IPC**; Rust abort/native crash affects GUI; callback/thread/lifetime/build complexity; no UI-independent scheduler | Viable alternative if process independence is rejected. |
| C ABI or swift-bridge | Small C facade or generated interop | Manual ownership/errors or more generator coupling; same-process crash/lifetime limits | Only for narrow needs/shims, not the default orchestration boundary. |
| Loopback HTTP/WebSocket | Familiar tooling and live streaming | Extra auth/origin/CSRF/listener surface and server dependency | Omit unless a browser client becomes a requirement. |

[Apple XPC](https://developer.apple.com/documentation/xpc.md) documents helper lifecycle/privilege boundaries; [UniFFI documentation](https://mozilla.github.io/uniffi-rs/latest/) documents Swift bindings and async support. These are documentation-level alternatives, not audited reusable modules.

Define transport-independent Command/Query/Event envelopes: protocol major/minor, request ID/idempotency key, expected aggregate revision, typed payload and bounded size. Responses include new revision or conflict. Events have a monotonically increasing sequence and aggregate/attempt identity. Reconnect reads a snapshot plus events from its committed cursor; a gap triggers snapshot reload. Reject unknown mutating commands. Separate bulk log bytes/artifact reads from control messages so an output flood cannot prevent cancellation or an approval response.

Only user-origin IPC commands may approve scope, PRDs, ADRs, deployment/publishing operations or changed baselines. Agent processes do not inherit the control socket or approval capability. Same-user unrestricted shell remains an explicit trust limitation (security document); token hiding alone is not an adversarial sandbox.

## Local persistence

Recommend one SQLite database on the local Mac filesystem and content-addressed blobs under a private application directory. Keep portable, user-visible Markdown artifacts in the workspace, with immutable approved copies/hashes in the evidence store. SQLite is authoritative for lifecycle and approvals; editable workspace files are drafts/imports and invalidate prior approval when content changes.

| Choice | Fit and tradeoff |
|---|---|
| SQLite + blobs | Transactional local state, inspectable relational joins, easy single-user backup; serialize writes, version migrations, manage WAL/checkpoints and blob consistency. Recommended. |
| JSON/Markdown only | Great portability, poor atomic multi-record scheduling/approval updates and concurrency; use for exports/drafts, not queue authority. |
| Embedded key-value database | Useful primitives, but adds custom indexes/query/migration logic for feature/repo/evidence joins without a clear benefit. |
| PostgreSQL | Good DB, unnecessary service/credential/backup burden for one Mac. No evidence of a local need. |

Use a single writer actor/queue and short SQLite transactions; workers send completed observations. With WAL, reader/writer coexistence does not remove the single-writer constraint. Pin a maintained build containing relevant WAL fixes; see [WAL documentation](https://www.sqlite.org/wal.html). Enable foreign keys, explicit busy policy, suitable durability settings and integrity/backup checks. Make backups through [SQLite's backup API](https://www.sqlite.org/backup.html) with a manifest of immutable blobs; test restore. Do not place active DB/WAL on network/cloud-synced storage.

Proposed records (design only): projects, repositories, features, use_cases, acceptance_criteria, artifact_revisions, clarification_sessions/questions/answers, architecture_options, adrs, approvals, executable_steps/typed_edges, resource_leases, run_attempts, run_events, observations, effect_outbox, deployments, validation_runs, evidence_manifests/blob_refs. Omit tenants, organizations, subscriptions, billing and admin roles.

Use foreign keys plus aggregate integer revisions. One transaction records the state transition, event and effect intent. External calls happen outside it; their completion returns through a version-checked observation. A slow CLI never holds a database write transaction.

## Product discovery and feature readiness

One feature moves through: idea → discovery → PRD draft → PRD approved → architecture proposed → architecture approved → execution plan approved → ready → developing → verifying → integration blocked/ready → complete. Rejected/revised artifacts return to review. Process states (launching/running/exited/interrupted) belong to attempts, not this lifecycle.

A complete feature package must include goal/non-goals, actors, scope, assumptions, data and integrations, NFRs, use-case preconditions/main/alternate/error flows/postconditions, acceptance IDs with measurable expected behavior, Figma references or an explicit no-design justification, architecture impact, dependency declarations, validation plan and unresolved issues. “Complete” means the schema and human review agree that **blocking** questions are resolved; it does not promise omniscient requirements.

Adapt Spec Kit's ambiguity taxonomy and one-question-at-a-time interaction, but retain durable question IDs, answer provenance and open/deferred/blocking distinctions. Do not adopt a hard five-question cap as evidence of completeness. Model-generated guesses are labeled assumptions until accepted.

Architecture generation presents 2–3 realistic options when there is a material choice, evaluates tradeoffs against accepted constraints, recommends one with supporting sources and writes ADR(s). Approval binds feature PRD hash, ADR hashes, dependency contracts and execution-plan revision. A relevant change invalidates affected approvals and downstream evidence. Cosmetic-only exceptions, if desired, need explicit defined rules; prototype should use conservative hash invalidation.

Figma analysis imports a bounded file/frame snapshot, preserves file key/node IDs/revision/capture time, and maps frames/variants/interactions to use cases and acceptance IDs. An imported design is input evidence, never authority for requirements. Missing error/loading/empty states become clarification questions. Generated mapping requires user confirmation. Exact file revisions may be unavailable through an MCP response; record capture identity/hash and that limitation rather than inventing a version.

## Two graphs, typed edges and cross-repository scheduling

Keep an **artifact readiness DAG** (PRD/ADR/test plan approvals) distinct from an **execution DAG** (repo-scoped implementation, build, deploy, integration and verification steps). Use stable IDs and one convention: `step requires prerequisite`. Validate all node IDs, self-edges, duplicate edges and cycles transactionally. Deterministic order uses prerequisite readiness, priority and stable declaration/creation order; stalled nodes expose which predicate blocks them.

A typed edge includes prerequisite step, required output kind, expected revision/digest, environment/contract selector and freshness policy. Suggested predicates: artifact approved; code verified at commit; PR merged; build artifact produced; backend deployed healthy; contract compatible; mobile build matches; independent validation passed. A boolean “upstream task completed” is too weak.

The scheduler acquires all required local resources atomically in a stable order: repository mutation lease, worktree/attempt identity, provider slot, simulator/device slot, build cache/workspace where necessary. Start sequentially with one coding run and one device job. Expand only after cancellation and recovery tests. Cross-repo scheduling does not require a distributed scheduler: one local coordinator can own a DAG spanning many repository paths/URLs.

### Deployment-aware example

1. Backend feature PRD/ADR/contract approved.
2. Backend implementation produces candidate source SHA; independent tests verify it; PR is merged when authorized.
3. Build/deploy step records source SHA → immutable artifact digest → deployment ID/environment.
4. Observer confirms that exact artifact is serving, migrations are compatible, health probes pass and expected API contract/schema is available.
5. Deployment receipt becomes eligible with observation timestamp/expiry and revocation status.
6. Mobile implementation/unit tests may run earlier if the approved plan permits; the dependent **integration** step waits for the deployment receipt and uses its exact endpoint/contract/build configuration.
7. Integration runner records mobile build SHA/digest, device/OS, backend receipt and test evidence. Recheck deployment before starting and invalidate/re-run affected results if backend identity changes during the run.

A merged PR, successful deploy command, healthy unrelated environment or mock server cannot satisfy step 4. For a first prototype, a user-approved manual deployment receipt is acceptable only if labeled manual and independently checked where possible; it does not count as automated deployment verification. Expired/unknown/rolled-back deployments block dependent jobs. Environment URL changes alone cannot silently retarget tests.

## Provider and process boundaries

Use official `claude -p` structured stream for bounded authoring/execution, and `codex exec --json` initially. Use documented Codex app-server stdio when interactive approvals/questions/multi-turn control are needed; perform initialize/initialized and request correlation, and use the matching release schema. Source [`codex-rs/exec/src/exec_events.rs`](https://github.com/openai/codex/blob/7924f95421a326bb66426a3ce232b72550948470/codex-rs/exec/src/exec_events.rs) is authority over Optio's outdated parser. Do not link [`codex-rs/app-server-client/Cargo.toml`](https://github.com/openai/codex/blob/7924f95421a326bb66426a3ce232b72550948470/codex-rs/app-server-client/Cargo.toml) by default: it brings server/core/login workspace dependencies.

Official binaries remain unmodified, installed by the user; login and refresh stay CLI-owned. Full auth guidance/discrepancies are centralized in [LICENSING_AND_SECURITY.md](../research/LICENSING_AND_SECURITY.md#official-cli-authentication-boundaries). No API-key fallback, credential relay or billing estimate inference. Probe `--version`/`--help`/supported status without inference; pin supported capabilities. Subscription exhaustion becomes waiting/needs-user state, not account rotation.

Adapter contract: discover capabilities, prepare scoped launch, start/resume, send supported input, emit normalized lifecycle/tool/question/approval/result events, interrupt/cancel, report exit and artifact references. Rust owns approval authority, worktree leases and evidence. CLI output is an observation; success requires terminal event plus acceptable exit plus required independent checks. Unknown events can be retained diagnostically but must not manufacture success. Bound line size, total output, parse time and child stderr; disk spooling/backpressure must preserve responsive control.

Use argv with explicit cwd/environment; pass prompt bytes through stdin or documented files, not shell interpolation. Preserve user CLI auth locations without reading them; omit unrelated secrets/provider overrides in the launch environment. Scoped CLI permissions are required. Host-side process execution has user privileges; worktree isolation is not a security sandbox.

### Crash recovery protocol

For the first bounded prototype, the helper owns and supervises runs; GUI loss must not imply feature failure. A helper crash may interrupt a run. We require safe conservative recovery before transparent resumption.

1. Commit attempt ID, expected revisions, worktree/base SHA, launch intent and resource leases before spawning.
2. Spawn a process group; record executable/version/start identity and log paths. Minimize the launch-before-PID-record gap; until resolved it is `launch_outcome_unknown`, not safe to start again.
3. Drain stdout/stderr into bounded durable files; track consumed offsets/partial NDJSON records and normalized event sequence. Never commit an event offset beyond durably stored bytes.
4. On normal exit, capture status, terminal record, final diff/head and evidence; atomically mark attempt outcome and enqueue validation.
5. On restart, obtain singleton ownership, inspect unfinished attempts/logs/worktrees, match process start identity and verify provider session status where supported. PID alone is unsafe because of reuse. Do not assume Linux `/proc` exists on macOS.
6. Reattach only through a designed supervisor/protocol that actually owns the process/output. Otherwise quarantine as interrupted/unknown and ask for inspect/retry; preserve partial work. Never claim that a saved session ID proves uninterrupted execution.
7. Reconcile PR/deployment side effects by stable identity before retry. Exactly-once remote actions are not guaranteed; ambiguous outcomes block.

Persist monotonic deadlines/boot context appropriately and re-observe after Mac sleep/wake; wall-clock jumps must not steal active leases. Expiry alone is not permission to launch a duplicate writer. A later small Rust per-run supervisor can survive engine restart, but this is an optional second process **role**, not a new service ecosystem. Approve it only if transparent continuation justifies more lifecycle/IPC work. LaunchAgent/XPC packaging is separate from domain modularity.

## Independent verification and evidence

Generate and approve a validation plan with the feature. Run tests in a clean candidate snapshot with a separate validator context/worktree; do not let the developer amend acceptance criteria, tests or visual baselines unnoticed. A fresh agent review may complement deterministic checks, not replace them. A validator sharing unrestricted host credentials is not a hostile-code sandbox.

Use project unit/contract tests, native `xcodebuild`/XCTest for macOS/iOS where appropriate, local Maestro for supported mobile flows, and OSV-Scanner for dependency evidence. Local Maestro screenshot assertions are already implemented in [`maestro-orchestra/src/main/java/maestro/orchestra/Orchestra.kt`](https://github.com/mobile-dev-inc/Maestro/blob/0ccf5a9c5b1c48d4cb6211d9060d537e04745107/maestro-orchestra/src/main/java/maestro/orchestra/Orchestra.kt); do not add reg-suit solely to get pixel comparison. Device/OS/font/locale/time/appearance and approved reference hashes must be stable. No AI-powered Maestro/cloud path in the keyless prototype.

Evidence records all source/build/plan/environment identities, command/exit/signal, expected/executed/skipped test inventory, raw reports/logs/screenshots, hashes, result and missing coverage. Missing report, zero unexpected tests, stale source, invalid baseline or unavailable scanner DB yields unknown/blocked, not pass. OSV findings are limited by package extraction, ignores and database freshness; retain that coverage context.

Write blobs atomically before DB references; retain manifests append-only with supersession. Hashes detect replacement but do not prove correctness or malicious same-user tamper resistance. Keep storage private, redact views, and allow explicit export/retention policy. Never store provider bearer tokens.

## Extraction and scope control

Reuse Markdown content where it saves real work. Port small graph/reducer/parser semantics with tests and attribution. Rewrite storage/process/workspace/bootstrap/native UI around this product. Keep tool engines external. Discard hosted admin/tenancy/cluster/billing and unattended persistent agents from the prototype.

The shortest validated path is one approved feature in an existing repository, one provider at a time, isolated worktree, independent deterministic tests and a source-bound evidence bundle, followed by the SwiftUI control surface and a two-repo deployment-gate scenario. Roadmap and approval points are separate documents. No code generation, repository creation or deployment is authorized by this proposal alone.
