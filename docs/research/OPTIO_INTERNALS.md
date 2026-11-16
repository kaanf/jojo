# Optio internals deep audit

Pinned repository: `jonwiggins/optio` at **f5f5b86da3cfa75e240637292a299dc5c6ffc5ba**, MIT. All paths below are from that revision and links embed it. Scope: relevant implementation and selected tests; no Optio runtime, cluster, agent or model call was started.

## Architectural shape

Optio is a hosted orchestration platform with a separate local execution path, not a drop-in local Rust core. API workers use BullMQ/Redis; data access uses Drizzle/Postgres; runtimes support pods/containers and processes; a local CLI daemon hosts terminal sessions; a Next/React UI observes REST/WebSocket state. Root [`package.json`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/package.json), [`apps/api/package.json`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/package.json), [`apps/cli/package.json`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/cli/package.json) and [`apps/web/package.json`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/web/package.json) establish these dependencies. The iOS application is additional client code, not proof of a macOS Rust bridge.

The useful architectural idea is **observe → decide → execute → observe again**. The strongest extraction boundary is a small decision library and the associated behavioral cases. Workers, credentials, pod pools and the dashboard should not be dragged into a local rewrite.

## Agent execution lifecycle

[`packages/shared/src/utils/state-machine.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/utils/state-machine.ts) defines:

- `PENDING` → `QUEUED` or `WAITING_ON_DEPS`.
- Waiting dependencies → queued, failed or cancelled.
- Queued → provisioning, cancelled or failed.
- Provisioning → running, failed or queued.
- Running → PR opened, completed, needs attention, failed or cancelled.
- Needs attention → running, queued or cancelled.
- PR opened → completed, needs attention, failed, cancelled or queued.
- Failed → queued or completed; cancelled → queued; completed has no outgoing transitions.

Failed/cancelled are retryable operational states, not terminal in the transition-graph sense. Keep run outcome separate from whether an operator can create a new attempt.

[`apps/api/src/workers/task-worker.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/task-worker.ts) claims work, checks current task state, branches between local and pod execution, resolves the adapter/environment, provisions/acquires a repository runtime, sets up worktrees, starts or reattaches a run, consumes logs, parses completion and records the result. It retains worktrees needed for continuation and uses recovery paths rather than treating a lost stream as success.

[`packages/agent-adapters/src/types.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/agent-adapters/src/types.ts) defines `validateSecrets`, `buildContainerConfig`, `parseResult` and `createResultParser`. The stream interface is useful. The container configuration and secrets vocabulary are wrong for our proposed user-installed CLI boundary; replace them with typed LaunchSpec/RunEvent contracts.

**Classification:** PORT lifecycle/reconciliation ideas; REWRITE the factory's feature/attempt lifecycle. Product document approval, deployment readiness and evidence sufficiency must remain separate from process/task state.

## Task scheduling and state ownership

[`apps/api/src/workers/reconcile-worker.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/reconcile-worker.ts) pops a `RunRef`, builds a snapshot, selects the repository/standalone/review/persistent reconciler and executes its single action. It retries stale CAS outcomes after roughly 0.5–1 second, general executor errors after roughly 5–7.5 seconds, and enqueues resync passes at a default five-minute interval, including a boot kick. These numbers are implementation defaults, not recommendations for a laptop.

[`packages/shared/src/reconcile/types.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/types.ts) distinguishes intent, desired spec, stored status, observed pod/PR/dependency/capacity facts and actions. [`apps/api/src/services/reconcile-snapshot.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/reconcile-snapshot.ts) materializes observations. [`packages/shared/src/reconcile/reconcile-repo.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-repo.ts) handles backoff, then control intent, then read errors, then per-state decisions. Despite the intent-precedence comment, **a future persisted backoff returns before interpreting a cancellation intent**. Our design should process cancel before cooldown.

Capacity exists globally and per repo; local tasks bypass pod capacity/off-peak controls. A laptop needs its own local CPU/memory/simulator/repository resource caps rather than inheriting cluster scheduling defaults. BullMQ job priority is not a feature dependency plan.

[`apps/api/src/services/reconcile-executor.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/reconcile-executor.ts) compares the snapshot's `updatedAt` with the stored timestamp before updating. This reduces stale writes, but a version check and a subsequent effect do not provide atomic exactly-once external execution. In `executeRepoTransition`, supporting fields are CAS-written before the delegated transition and fan-out. Reimplement this in SQLite as one revisioned state/event/outbox transaction; separately reconcile external operations.

**Classification:** PORT the decision boundary; REWRITE executor/storage/queue. Do not port Redis/Postgres just to preserve this architecture.

## Reconciliation algorithms and concrete divergences

[`packages/shared/src/reconcile/reconcile-repo.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-repo.ts) contains dependency gating, queued-capacity checks, lost-attachment detection, pod/stall checks, PR merging/closing, edge-triggered CI/review reactions and bounded auto-resume. Relevant tests are [`packages/shared/src/reconcile/reconcile-repo.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-repo.test.ts), [`packages/shared/src/reconcile/reconcile-edge-cases.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-edge-cases.test.ts) and [`apps/api/src/services/reconcile-executor.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/reconcile-executor.test.ts).

Findings from actual control paths:

| Finding | Evidence and implication | Required factory behavior |
|---|---|---|
| Dependency meaning differs by path | `areDependenciesMet` in [`apps/api/src/services/dependency-service.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/dependency-service.ts) accepts COMPLETED **or PR_OPENED**; `decideWaitingOnDeps` in the reconciler requires all COMPLETED. [`apps/api/src/services/dependency-service.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/dependency-service.test.ts) explicitly tests the former. An event-driven wake can release work sooner than a resync decision. | One evaluator for every typed edge. PR opened never implies deployed/healthy. |
| Restart interpretation disagrees with transition table | `interpretIntent` can return a queued transition for COMPLETED, and [`packages/shared/src/reconcile/reconcile-repo.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-repo.test.ts) expects it. [`packages/shared/src/utils/state-machine.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/utils/state-machine.ts) makes COMPLETED absorbing. The executor delegates to transition validation, so this is a static contract mismatch; no end-to-end reproduction was run. | Completed attempt immutable; restart creates a new attempt, explicitly and transactionally. |
| “Pure” decisions contain randomness | `jitteredBackoff`, capacity/off-peak requeue functions call `Math.random()` inside [`packages/shared/src/reconcile/reconcile-repo.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-repo.ts). | Inject jitter or calculate deadlines outside reducer; deterministic replay requires all inputs recorded. |
| Read failure can become no checks | [`apps/api/src/workers/pr-watcher-worker.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/pr-watcher-worker.ts) calls `getCIChecks(...).catch(() => [])`; `determineCheckStatus([])` is `none`. The reconciler's `checksOk` accepts `passing` or `none` for auto-merge decisions. External merge guards may still reject; this is a risky observation collapse, not a claim that every failed lookup merges a PR. | Unknown/error distinct from legitimately absent checks; required checks must be positively observed for exact SHA. |
| Process outcome is not feature completion | Parsers accept exit-zero streams without necessarily requiring complete product evidence. PR URL extraction uses regex matches in agent output. | Verify PR ownership/repo/head via SCM; require terminal event, exit status and separate validation evidence. |

Graph code [`packages/shared/src/utils/dag.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/utils/dag.ts) implements iterative cycle detection, Kahn ordering and reverse reachability. Edges mean `from depends on to`; isolated nodes are not expressible in an edge-only input. Use OpenSpec's explicit node set as a better starting point and include cycle/missing-node/self-edge/duplicate-edge tests. Do not silently reverse prerequisite direction between planning and execution.

## Claude Code integration and authentication boundary

[`packages/agent-adapters/src/claude-code.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/agent-adapters/src/claude-code.ts) supports API-key, `max-subscription`, Vertex and Bedrock modes. `max-subscription` describes a token fetched at execution time and injected into the worker environment. [`apps/api/src/workers/task-worker.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/task-worker.ts) builds `claude --print`, stream-json stdin/stdout, replay-user-messages, resume/model/turn flags and `--dangerously-skip-permissions` for its pod path.

The result parser tracks the **last** result event, treats explicit result errors as failures even if the process exits zero, supersedes recovered transient API errors with a successful final result, aggregates usage and extracts PR URLs. [`packages/agent-adapters/src/claude-code.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/agent-adapters/src/claude-code.test.ts) supplies edge cases. PORT a small version of these normalization rules, adding a required-terminal-event rule and bounded parsing.

[`apps/cli/src/local/claude-credentials.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/cli/src/local/claude-credentials.ts) reads the macOS `Claude Code-credentials` Keychain item or credential JSON, extracts an access token, and can run a small inference command to trigger renewal. [`apps/api/src/services/local-auth-refresh-service.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/local-auth-refresh-service.ts) coordinates server/daemon refresh. **DISCARD this subsystem.** We should neither read CLI credential stores nor send tokens to a factory proxy. Technical access is not provider authorization. Current official documentation and its scope/conflict are recorded once in [LICENSING_AND_SECURITY.md](LICENSING_AND_SECURITY.md#official-cli-authentication-boundaries).

The local command path is distinct: [`apps/cli/src/local/agent-command.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/cli/src/local/agent-command.ts) constructs interactive/headless/resume commands against local binaries and probes supported options. It defaults unknown/unset permission modes to bypass. REWRITE using explicit fail-closed capability configuration; preserve official CLI permission prompts or a scoped documented policy.

## Codex integration and protocol fidelity

[`packages/agent-adapters/src/codex.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/agent-adapters/src/codex.ts) defaults to API-key secret requirements unless `app-server` is selected; it also contains Bedrock config construction. The `app-server` mode exposes a URL environment value. The task worker constructs `codex exec --dangerously-bypass-approvals-and-sandbox ... --json`, optionally with an app-server argument. Do not assume those flags work across arbitrary official releases; confirm against installed `--help` and supported protocol.

Its result parser and [`apps/api/src/services/codex-event-parser.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/codex-event-parser.ts) primarily understand `message`, `function_call` and `function_call_output`, plus generic error/usage shapes. The result parser misses nested assistant-message items and does not model the dotted lifecycle explicitly. Its generic `event.error.message` branch does catch the official `turn.failed` envelope; the separate display parser skips `turn.failed` and thread/item lifecycle records. Generic usage extraction can still consume `turn.completed` usage. These are specific compatibility gaps, not a claim that all current failures or token counts are lost. [`packages/agent-adapters/src/codex.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/agent-adapters/src/codex.test.ts) tests those older shapes, including empty-output success, so those passing tests would not prove compatibility with the current CLI.

Direct comparison: official `openai/codex` **7924f95421a326bb66426a3ce232b72550948470**, [`codex-rs/exec/src/exec_events.rs`](https://github.com/openai/codex/blob/7924f95421a326bb66426a3ce232b72550948470/codex-rs/exec/src/exec_events.rs), defines dotted thread/turn/item events and tagged nested items. [`codex-rs/app-server-protocol/src/protocol/common.rs`](https://github.com/openai/codex/blob/7924f95421a326bb66426a3ce232b72550948470/codex-rs/app-server-protocol/src/protocol/common.rs) is the richer RPC authority. This is the evidence for DISCARDing Optio's Codex parser and REWRITEing a small official-protocol adapter. Keep the CLI external; do not port or embed the agent engine.

## Process management and crash recovery

Pod path: [`packages/container-runtime/src/run-protocol.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/container-runtime/src/run-protocol.ts) generates a supervisor with durable `agent.sh`, `supervisor.sh`, stdout/stderr/stdin files, PID/start identity and exit marker. Operations are start, attach-from-byte, stdin-deliver and group kill. It uses Linux `/proc/<pid>/stat`, `setsid` and GNU `tail --pid`, so its generated scripts are not directly portable to stock macOS.

[`packages/shared/src/reconcile/attachment.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/attachment.ts) interprets started/exited/never-started and attachment leases (default 60 seconds, renew 20). [`apps/api/src/services/run-attachment.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/run-attachment.ts) claims attachment/renewal and settles exits; [`packages/container-runtime/src/fake-protocol.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/container-runtime/src/fake-protocol.test.ts) covers reattachment, PID-scoped kill and lost exits. These are useful test scenarios.

Local process placement: [`packages/container-runtime/src/process.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/container-runtime/src/process.ts) spawns Bash in its own process group, appends output files and replays from byte offsets. It relies on the parent's in-memory run map; a later runtime with no live entry and no exit marker returns `lost`. Its comment says runs end with the parent, but `detached: true` by itself is not an OS guarantee of child death on parent crash. The safe conclusion is **no dependable reattachment by the new process**, with possible orphan effects needing inspection. We did not test parent SIGKILL behavior.

Local terminals: [`apps/cli/src/local/terminal-manager.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/cli/src/local/terminal-manager.ts) uses `node-pty`, directory allowlists, duplicate-terminal detection, TERM then timed KILL and cleanup. [`apps/cli/src/__tests__/terminal-manager.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/cli/src/__tests__/terminal-manager.test.ts) tests lifecycle behavior. A PTY is valuable for interactive terminal handoff, not necessary for every JSONL job. Do not parse rendered terminal text as authoritative status when a structured protocol is available.

**Classification:** REWRITE process supervision for macOS; PORT run identity/offset/reattach concepts. No exactly-once claim: a crash after PR/deploy succeeds but before journaling requires observation, not blind replay.

## Worktree isolation

[`apps/api/src/services/repo-pool-service.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/repo-pool-service.ts) constructs per-task branches/worktrees, task/repo locks, Git credentials/settings and recovery scripts. It resets an existing retry worktree (`git clean -fd`) and uses forced worktree removal/prune paths; the shared repo setup includes `git reset --hard`. These are intentionally aggressive operations for owned runtime directories. They must not target a user's existing checkout.

[`apps/api/src/services/repo-pool-service.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/repo-pool-service.test.ts) and [`apps/api/src/services/repo-pool-service.dind.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/repo-pool-service.dind.test.ts) offer script/pool cases; the latter requires infrastructure and was not run. Worktrees separate files/indexes but share Git object storage/config and host privileges; they do not isolate secrets, processes, network, build caches or simulators.

**Classification:** REWRITE narrowly around installed Git: engine-owned paths, canonical containment, leases per repository mutation, unique attempt branch, recorded base/head, no reset of dirty work, quarantine after unknown outcomes. Git itself remains EXTERNAL TOOL.

## GitHub PR and CI monitoring

[`apps/api/src/workers/pr-watcher-worker.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/pr-watcher-worker.ts) polls at a default 30 seconds under a poller lease, reads CI for the PR head SHA, computes check/review state, updates status and wakes reconciliation. PR review head changes trigger stale review state. [`packages/shared/src/reconcile/reconcile-pr-review.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-pr-review.ts) handles review lifecycle; [`apps/api/src/workers/pr-watcher-worker.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/pr-watcher-worker.test.ts) supplies observer cases.

[`packages/shared/src/reconcile/reconcile-repo.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-repo.ts) completes merged PRs, fails closed-unmerged PRs, reacts to newly failing CI/conflicts/review changes, bounds auto-resumes and can launch reviews or request auto-merge. It is a PR lifecycle, not proof of backend deployment. Merged does not mean packaged, migrated, deployed, healthy or compatible with mobile.

**Classification:** REWRITE a small observer with gh JSON and explicit freshness/unknown state; PORT stale-review and edge-trigger ideas. Prototype auto-merge should be off. Independent verification is stronger than “a review agent ran.”

## Retry, recovery and persistence semantics

There are multiple retry layers: provisioning errors in the task worker, stale/error/deferred reconciliation, attachment recovery, PR-reactive continuation and operator retry/restart. [`apps/api/src/workers/task-worker.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/task-worker.ts) classifies provisioning failures and caps retries; [`apps/api/src/services/reconcile-executor.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/reconcile-executor.ts) persists rate/auth cooldowns to suppress all producers, not only one delayed job. [`apps/api/src/workers/reconcile-worker.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/reconcile-worker.ts) resyncs missed events.

Keep distinct budgets in Rust: transport retry without new model execution, process-attempt retry, validation repair loop and deployment observation retry. Authentication failure, denied permission, malformed output, unavailable evidence and unknown remote effects must not trigger an unbounded agent loop. Preserve attempts/worktrees/evidence. Never automatically compensate with remote deletion or overwrite.

## Persistent agents

[`packages/shared/src/reconcile/reconcile-persistent-agent.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/packages/shared/src/reconcile/reconcile-persistent-agent.ts) models idle → queued → provisioning → running → idle with pause/archive/restart, inbox wakeups and consecutive-failure limits. A five-minute no-open-turn grace protects setup/settling. [`apps/api/src/services/persistent-agent-pool-service.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/persistent-agent-pool-service.ts) distinguishes always-on, sticky and on-demand pods, one turn per logical agent. [`apps/api/src/workers/persistent-agent-worker.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/workers/persistent-agent-worker.ts) executes turns; [`apps/api/src/services/persistent-agent-pool-service.test.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/services/persistent-agent-pool-service.test.ts) covers pool behavior.

A durable identity/session/inbox is separable from a warm compute process. Our prototype needs bounded planner/developer/validator runs first; persistent roles can come later with recorded inbox consumption and context revisions. **REWRITE and defer**; DISCARD always-on cluster infrastructure.

## Dashboard architecture

[`apps/web/src/hooks/use-dashboard-data.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/web/src/hooks/use-dashboard-data.ts) composes REST requests and refreshes summaries every ten seconds, usage on a slower interval. [`apps/web/src/hooks/use-websocket.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/web/src/hooks/use-websocket.ts) patches task state and notifications from live events; [`apps/web/src/hooks/use-store.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/web/src/hooks/use-store.ts) uses Zustand/localStorage for UI state. [`apps/api/src/ws/events.ts`](https://github.com/jonwiggins/optio/blob/f5f5b86da3cfa75e240637292a299dc5c6ffc5ba/apps/api/src/ws/events.ts) is the API event surface. This is a mix of polling and event updates, not an append-only evidence UI.

**REWRITE in SwiftUI:** feature readiness/blocked reasons, approval inbox, artifact diff, architecture options, active attempts, log/evidence viewers and environment status. The Rust engine owns durable state; the GUI reconnects with a sequence cursor and refreshes a snapshot on gaps. Provider credential/usage introspection panels are unnecessary.

## Final extraction boundary

PORT: selected reconcile semantics, explicit observations/actions, small graph/readiness patterns, Claude final-result handling and recovery test scenarios.

REWRITE: scheduler predicates, SQLite executor/outbox, macOS supervisor, worktrees, current Codex adapter, PR/CI observer, native dashboard, product discovery/approval/evidence/deployment layers.

EXTERNAL TOOL: official CLIs, Git/gh and test/scanner executables.

DISCARD: credential relay, bypass defaults, hosted tenancy/SSO/cluster machinery and any assumption that PR-open/exit-zero/file-present means the feature is verified.
