# Implementation roadmap — after explicit approval only

Stage 1 is complete as a static source audit. This roadmap is a proposal, not authorization to start implementation. Full evidence and classifications are in the [audit](../research/REPOSITORY_AUDIT.md), [matrix](../research/COMPONENT_EXTRACTION_MATRIX.md) and [architecture](../architecture/PROPOSED_ARCHITECTURE.md). Decisions to approve are listed in [OPEN_DECISIONS.md](../architecture/OPEN_DECISIONS.md).

## Shortest path to a working prototype

Deliver one vertical slice: **idea → clarified feature PRD/use cases → user-approved architecture/plan → one isolated coding attempt → independent deterministic validation → evidence-backed completion**, controlled through a small native SwiftUI client. Start in an existing sample repository, with one official CLI and no remote provisioning. Add the second CLI before claiming provider parity; add one backend/mobile dependency scenario before claiming deployment-aware orchestration.

This avoids building a portal, generalized plugin platform, persistent-agent fleet or a port of an entire TypeScript/Python application. It does not bypass the PRD/architecture gates to get code faster. A CLI development harness can precede the GUI to validate core invariants; it is not the final application.

## Phase 0 — accept decisions and validate boundaries

**Entry:** explicit implementation approval and a sample repository/feature. Resolve D01–D09; keep GitHub read-only by default.

Work:

- Record accepted ADRs for helper process, IPC/lifecycle, persistence, artifact format, permission/trust model, auth boundary and conservative recovery.
- Verify actual official CLI versions, documented options/schema and supported subscription status without inference; have the user sign in with official native flows if needed. Confirm extra-usage policy separately; no API-key fallback.
- Select supported tool releases and maintained Rust/SQLite dependencies; check binary origin, host architecture and licenses. Audit selected dependency notices; Stage 1 HEAD snapshots are not release pins.
- Choose a small testable sample feature and expected deterministic test commands. No edits to `agentic-factory`, no remote repositories and no infrastructure provisioning.

**Exit evidence:** approved ADRs, capability/version inventory, sample acceptance plan and a concrete bounded execution permission policy. If auth mode or required privileges cannot be established under existing constraints, stop that provider integration without switching to paid APIs.

Source basis: small reconcile contract [`packages/shared/src/reconcile/types.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/types.ts), current Codex events [`codex-rs/exec/src/exec_events.rs`](https://github.com/openai/codex/blob/7924f95421a326bb66426a3ce232b72550948470/codex-rs/exec/src/exec_events.rs). These are starting contracts, not permission to copy a whole engine.

## Phase 1 — durable feature and approval kernel

**Components:** C19–C23/C36 REWRITE plus adapted REUSE content; C03 small graph PORT.

Build domain types and migrations for features, use cases, questions/answers, artifact revisions, ADRs, approvals, steps/typed edges and immutable attempts. Use SQLite single-writer transactions, integer revisions, event journal and outbox. Render editable Markdown from the typed feature schema; preserve source notices for adapted templates.

Implement discovery as persisted state: one question at a time, explicit assumptions/blockers, acceptance IDs, PRD completeness checks. Architecture recommendation produces alternatives and an ADR; user approval binds the exact PRD/ADR/plan revision. A changed prerequisite invalidates readiness. Use one artifact DAG and one execution DAG with explicit node sets and cycle explanations.

Meaningful verification:

- Missing acceptance paths, unresolved blocking questions, placeholders or absent approvals prevent scheduling.
- Editing an approved PRD or ADR invalidates downstream readiness/evidence.
- Concurrent stale approval/launch commands fail revision checks rather than approving a newer artifact accidentally.
- Cycle/missing-node/self-edge/isolated-node cases are handled deterministically.
- A crash between transition and effect dispatch preserves the outbox record for observation/recovery.

**Exit:** one complete approved feature package can reach ready; no unapproved feature can execute. No remote side effects required. Source content: [`templates/spec-template.md`](https://github.com/github/spec-kit/blob/2ee2b1e8005905db05f5717300a469b3068b908f/templates/spec-template.md), [`templates/commands/clarify.md`](https://github.com/github/spec-kit/blob/2ee2b1e8005905db05f5717300a469b3068b908f/templates/commands/clarify.md); graph evidence [`src/core/artifact-graph/graph.ts`](https://github.com/Fission-AI/OpenSpec/blob/ea635dd255f8686dcf48626bbcbe0e3fd59b6067/src/core/artifact-graph/graph.ts). Replace [`src/core/artifact-graph/state.ts`](https://github.com/Fission-AI/OpenSpec/blob/ea635dd255f8686dcf48626bbcbe0e3fd59b6067/src/core/artifact-graph/state.ts) existence-as-completion semantics.

## Phase 2 — one provider, isolated work and safe recovery

**Components:** C01 PORT, C02/C05/C06/C13 REWRITE, Git/Codex EXTERNAL TOOL. Recommended first provider: Codex, because its current structured source contract is auditable. Claude follows in Phase 4; switching the order is acceptable if user preference/access dictates.

Implement a scoped process launcher and current official JSONL adapter, argv/stdin framing, bounded logs, process-group cancellation, tool/version capture and conservative unknown outcomes. Create engine-owned worktrees/branches based on a recorded source SHA; do not reset or clean user checkouts. Enforce repository/provider resource leases and initially one coding attempt.

Have the agent implement only the approved feature plan. Treat its claimed PR/test status as untrusted observations. Record candidate SHA/diff separately from plan/PRD hashes. On helper restart, inspect incomplete attempts before any new writer starts; preserve dirty/unknown work.

Meaningful verification with fake local processes first:

- Split/malformed/oversized JSONL, stderr floods, output backpressure, exit zero without terminal result, `turn.failed`, auth errors and missing executable.
- Cancellation reaches descendants; PID reuse never kills an unrelated process.
- GUI disconnect differs from helper crash; interrupted attempt blocks duplicate execution.
- Crash after spawn but before identity commit, after output append but before offset update, and after effect but before acknowledgement.
- Dirty worktrees survive interruption/retry; paths with spaces/metacharacters and leading-dash input are safe.

Then one explicitly authorized real subscription-backed feature attempt, within user limits. **Exit:** deterministic unit tests for the new kernel/adapters plus a preserved, inspectable attempt/evidence record; no unsupported automatic restart or auth relay. Upstream recovery cases: [`packages/container-runtime/src/fake-protocol.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/container-runtime/src/fake-protocol.test.ts), [`packages/container-runtime/src/process.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/container-runtime/src/process.test.ts); avoid Linux-specific supervisor scripts from [`packages/container-runtime/src/run-protocol.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/container-runtime/src/run-protocol.ts).

## Phase 3 — independent validation and minimal SwiftUI prototype

**Components:** C17/C33/C35/C37 REWRITE; existing project test runner EXTERNAL TOOL.

Implement the minimum native views: feature list and blocked reason, clarification conversation, PRD/ADR diff and approval, run progress/cancel, logs, validation results and evidence detail. SwiftUI sends typed commands and reads engine projections. Prototype transport is stdio with documented lifecycle; use a private Unix socket before promising reconnection to a continuing helper. No UI direct DB writes.

Run deterministic acceptance/unit checks from a clean candidate snapshot with a separate validator role/context as needed. Collect expected test inventory, logs/reports/exit statuses and source/tool identities; freeze acceptance plan and baseline revisions. Store immutable evidence manifests and redacted views. Add native unit/UI coverage for the approval and reconnect path; avoid broad visual test infrastructure for the control app.

**Exit/demo:** enter one idea, answer questions, approve the complete PRD/use cases, approve one architecture/ADR and plan, execute one feature, view independent test evidence, and mark complete only if required criteria pass. Reject a missing/empty/stale report. Restart the UI/helper according to D02/D09 and show preserved state honestly. This is the first working prototype, not yet a cross-repository/deployment product.

## Phase 4 — Claude parity and current rich client contracts

**Components:** C08 selected PORT; C09/C12 EXTERNAL TOOL; C13 current protocol REWRITE.

Add the second official CLI through its native authentication. Port only useful Claude final-result/error semantics from [`packages/agent-adapters/src/claude-code.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/agent-adapters/src/claude-code.ts) with fixture provenance; reject credential relay [`apps/cli/src/local/claude-credentials.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/cli/src/local/claude-credentials.ts) and bypass defaults. Where required, add supported Codex app-server stdio commands for multi-turn questions and native permission responses using release-matched schema.

**Exit:** same feature/attempt/evidence state machine works through both CLIs; auth/permission/rate-limit failures pause without paid fallback, copied tokens or silent retries. Real-provider smoke tests are explicit and bounded; routine automated tests use recorded/synthetic protocol fixtures without inference.

## Phase 5 — Figma traceability and reviewable bootstrap

**Components:** C24 optional PORT, C25 EXTERNAL TOOL conditional, C26/C27 REWRITE.

Start from user-exported design data or existing entitled official MCP. Bound node/image sizes; link frame/node/capture identities to use cases and acceptance IDs. Show missing variants/error states and require mapping confirmation. Only port normalization pieces if raw context size actually justifies them; do not reimplement all Figma layout/CSS machinery.

Add workspace/repository bootstrap **planning**: repo topology, local directories, chosen templates, CI/test commands and generated diff. Only after a concrete plan and explicit remote-action approval add gh create/push/PR operations. Journal intent/result and reconcile partial/ambiguous effects; no rollback by deleting remote repos. Test using a fake SCM adapter or local repos first.

**Exit:** design revision changes invalidate affected mappings; bootstrap is repeatable and non-destructive; partial remote success is detected without duplicate creation. Source basis: [`src/extractors/node-walker.ts`](https://github.com/GLips/Figma-Context-MCP/blob/c083d65c7e002923e7cb98f4e3bdafb105e90f6d/src/extractors/node-walker.ts), [`plugins/scaffolder-backend/src/scaffolder/actions/builtin/fetch/templateActionHandler.ts`](https://github.com/backstage/backstage/blob/57111dcbf112e263688c03fd481315d7804a89e0/plugins/scaffolder-backend/src/scaffolder/actions/builtin/fetch/templateActionHandler.ts), [`plugins/scaffolder-backend-module-github/src/actions/githubRepoCreate.ts`](https://github.com/backstage/backstage/blob/57111dcbf112e263688c03fd481315d7804a89e0/plugins/scaffolder-backend-module-github/src/actions/githubRepoCreate.ts). Do not install Backstage.

## Phase 6 — two-repository deployment-gated mobile integration

**Components:** C04/C14/C34 REWRITE; Git/gh, project backend tools and Maestro EXTERNAL TOOL. Requires D12–D17.

Create a small backend/mobile feature plan across two registered repos. Implement typed edges and local resource locks. Record backend source SHA/artifact digest/contract, observe its deployment to an approved non-production environment, verify migrations/health/contract and issue an expiring receipt. Use explicit manual receipts only if approved and labeled.

Mobile unit work may proceed against an approved contract, but integration jobs are blocked until that exact backend receipt is valid. Bind mobile build/config/device/OS and backend identity in evidence. Reobserve before dispatch and around completion; changing or revoking the backend receipt invalidates dependent verification. Use local non-AI Maestro flows and stable screenshot baselines; native XCTest/XCUITest covers the factory's own UI.

Meaningful gate scenarios:

- Backend PR opens/merges but no deployment exists: mobile integration remains blocked.
- Wrong commit, healthy wrong environment, stale receipt, failed migration, incompatible schema or unknown CI: blocked with reason.
- Matching deployment becomes healthy: integration queues exactly once with required resources.
- Backend rolls back or changes during integration: invalidate/stop or re-run under explicit policy; old pass cannot complete the feature.
- Simulator unavailable/timeout/flaky retry: record each attempt and coverage; no synthetic pass.

**Exit:** one reproducible backend → deployed healthy compatible artifact → mobile integration sequence, with source/environment evidence and demonstrated invalidation. This is the first point to claim deployment-aware cross-repository orchestration. Existing Optio task edges alone do not meet this bar: [`apps/api/src/services/dependency-service.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/dependency-service.ts).

## Phase 7 — dependency checks, packaging and recovery hardening

**Components:** OSV-Scanner EXTERNAL TOOL C32; evidence/storage/process improvements REWRITE. Implement only what prototype findings justify; scanner integration may move earlier if the sample's dependency policy requires it.

Add OSV JSON/SARIF evidence with explicit network/offline database freshness, extracted package inventory, ignores and failure distinctions. Keep Maestro screenshot assertions before introducing reg-suit. Add backup/restore, disk-pressure handling, artifact retention, Mac sleep/wake and upgrade/migration tests. Evaluate per-run supervisor, Unix socket/XPC/LaunchAgent and signing/notarization based on actual background lifetime requirements.

**Exit:** restore reproduces accepted feature state and blob integrity; helper crash, disk-full and partial writes preserve unknown outcomes safely; packaged app verifies official binary discovery/capabilities. Resolve mixed-origin/transitive notices before bundling any external tool. OSV source boundary: [`pkg/osvscanner/osvscanner.go`](https://github.com/google/osv-scanner/blob/80fe50567a5d6e0ec408353e2ceacca1561b91be/pkg/osvscanner/osvscanner.go), [`internal/output/machinejson.go`](https://github.com/google/osv-scanner/blob/80fe50567a5d6e0ec408353e2ceacca1561b91be/internal/output/machinejson.go); root license alone is insufficient clearance.

## Deferred work

Persistent logical agents, advanced multi-agent concurrency, always-on supervision, broad template/plugin marketplaces, automatic merging, cloud report publishing and new deployment providers are deferred until a demonstrated need. Discard tenancy, billing, enterprise administration and cluster infrastructure from this personal product. Full OpenSpec delta interchange and Spec Kit workflow compatibility are optional future decisions, not hidden prototype requirements.

## Effort and evidence discipline

The ordering is a critical path, not a calendar commitment. CLI protocol/login friction, macOS process lifecycle, actual app testability, Figma entitlement and deployment identity are the main uncertainty drivers. Estimate phases only after Phase 0 selects the sample/tool versions and permission policy. Do not call a TypeScript-to-Rust translation a shortcut without counting dependencies, semantic corrections, test migration and ongoing protocol maintenance.

Stage 1 checks are source/test inspection and reference/repository integrity verification only. No upstream test suite or real agent/device/deploy integration was executed. Every later phase must report separately what was tested, on which source/tool/environment revisions, and what remains unknown.
