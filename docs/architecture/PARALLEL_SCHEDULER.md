# Jojo — Parallel agent scheduler

Status: proposed scheduler contract under confirmed multi-agent requirements. Related: [core model](CORE_STATE_MODEL.md), [ADR D07](ADR-DRAFTS.md#d07--parallel-resource-and-worktree-safety), [shutdown](SHUTDOWN_RECOVERY.md). One SQLite writer is compatible with many concurrent agent process trees: only state transitions are serialized.

## Goals, limits and scope

Support multiple Claude Code and Codex invocations, mixed providers/roles, independent repository concurrency and eventually same-repository concurrency with distinct worktrees. A default limit of one is configuration, never a global architecture mutex. Every invocation is a separate attempt; a provider's internal subagents/background tools are part of its process/effect tree, not individually scheduled Jojo tasks. Slot limits count Jojo-owned invocation trees, not all model requests or every process on the Mac.

The core is one local coordinator, not a distributed scheduler. Inputs are a current approved plan revision, persisted facts, validated resources, a supplied clock and a configuration revision. Output is explainable decisions and durable reservations; reducer code does not read the clock, call random functions, perform Git I/O or launch processes.

## Resource policy

Proposed defaults: `global_agent_slots=1`, Claude slots one, Codex slots one, repository slots one, device slots one per named device; independent test-process pool also bounded. These are U05 defaults, not confirmed numeric user decisions. Every process-producing step declares its resource class. Gracefully stopping, awaiting-input and unknown-live attempts retain their slots. A known-dead process releases its process slot after durable settlement; shared-effect/worktree holds may remain separately.

| Resource key | Holding policy |
|---|---|
| global agent / provider slots | Capacity reservations for each Jojo provider invocation; do not count same invocation twice across retries of IPC commands |
| repository execution slots | Capacity per canonical Git common-dir; distinct clones/repositories independent; cap may exceed one |
| repository activity gate | Normal private-worktree execution holds SHARED; provisioning/pruning/shared config/GC/ref integration operations require EXCLUSIVE and no unknown-live repo activity |
| worktree writer | EXCLUSIVE for one attempt; validator uses a distinct snapshot/worktree; path aliases cannot create a second owner |
| attempt output/evidence writer | EXCLUSIVE attempt directory; no concurrent process writes a previous attempt's raw stream/results |
| named simulator/device, build directory, mutable cache | EXCLUSIVE unless tool contract explicitly proves compatible sharing; separate DerivedData/build outputs preferred |
| future deployment environment/remote ref | Separate key only when approved adapters exist; no live effects in Phase 1 |

Claims are durable resource reservations with holder attempt/operation, epoch and generation, not provider-enforced fencing. Agent Git operations on their private branch/index rely on Git's own internal locking within the declared activity envelope. Jojo prohibits shared config/ref rewrites/GC in an agent plan while other runs are active. Cooperative resource coordination does not constrain malicious user shell commands or another app using that repository.

Repository administration and execution are separate resource phases. Provision worktrees under a short exclusive activity claim, seal the owned path/branch/base binding, then release that administration claim before execution admission. Never wait for an exclusive gate while holding a shared gate. Admission acquires the full execution set at once. If two same-repo jobs are to start together, pre-provision their distinct worktrees first. Provisioning can wait for existing shared execution to finish; this conservative policy trades utilization for clear safety. An exclusive administrative waiter blocks new shared admissions for that repo until serviced; unrelated repositories continue. No Git reset/clean/prune of unknown or dirty work.

Retained interrupted worktrees are not silently reused. Default continuation forks a new owned worktree from a reviewed candidate commit/snapshot and carries validated changes under a new attempt; original worktree/evidence remains inspectable. In-place ownership transfer would require an additional approved design, not an assumed resume capability.

## Eligibility and dependencies

A candidate step must have RUN intent, the current approved plan and execution permission scope, valid source bindings, all typed predicates SATISFIED, no recovery hold and no unsealed attempt. UNKNOWN is never treated as false-success; show missing/stale/failed-read reasons. The exact same predicate evaluator serves event-driven updates, scheduler scans, restart recovery and pre-launch revalidation.

[Core predicate table](CORE_STATE_MODEL.md#step-projection-and-dependencies) defines artifact, source, merge, build, deployment and validation facts. All edges AND-combine initially. PR opened alone satisfies nothing. A predecessor agent's success/URL also cannot select an unverified candidate output automatically. A dependent source base is resolved from the selected output digest/vector, not the current default branch by convenience.

During a running attempt, approval/source/deployment invalidation requests stop and makes its output ineligible. Already committed results stay immutable. A ready-cache is only an optimization keyed by plan/fact/config revisions; the transaction checks current revisions rather than trusting cache membership.

## Fairness proposal

Use deterministic round-robin across canonical repository queues, with a persisted repository cursor. Within each repo choose an eligible task by effective priority, oldest original ready sequence, then step ID. Proposed bounded priority levels 0–3; increment priority by one for each ten minutes of eligible waiting, capped at three. Persist original ready timestamp/sequence; capacity waits or retries of observation do not reset age. Clock/elapsed-time decisions are injected and event-recorded; after uncertain reboot clock changes use preserved order and do not artificially age jobs by an untrusted clock jump.

At most one job per repo is chosen per pass; repeat passes to fill available slots. Skip resource-infeasible candidates with a reason so a saturated provider/repo does not block unrelated work. No preemption of running provider work solely for priority. Provider cooldown blocks that provider, not all scheduling. Paused/blocked tasks do not consume agent slots. User-prioritized jobs have bounded boost, so an endless stream cannot outrank an old eligible job indefinitely under stable finite arrivals.

Multi-resource starvation needs an explicit limit: after three eligible skips caused by smaller jobs consuming the resources of an older ready candidate, create a **logical drain barrier** for its contested resources. Do not reserve physical resources partially; stop admitting later conflicting candidates until incumbents drain and the older candidate can acquire its entire set. Unrelated work continues. Increment a skip only when a younger conflicting candidate is actually admitted, not on every polling tick. Conflicting drain barriers are ordered by original ready sequence then step ID; the oldest barrier has precedence, so barriers cannot mutually block each other. Clear the barrier on cancellation/ineligibility or once its entire reservation is admitted. Fairness promises assume finite running jobs or user resolution of hung/unknown jobs; a quarantined live writer cannot be displaced for fairness.

Fairness constants are proposed U05 policy; deterministic simulations must show service ordering before tuning for utilization. No claim of optimal scheduling or unbounded-arrival starvation freedom.

## Atomic admission and launch sequence

1. Receive wake-up or periodic reconciliation input. If engine boot/recovery/close gate is closed, do not admit anything. Cancellation and result settlement still execute.
2. Build current ready candidates and resource needs from the approved plan; apply fairness and drain barriers.
3. In one short writer transaction, recheck step/control/plan/approval/fact/config revisions, engine ownership, prerequisites and complete resource availability. Insert attempt RESERVED, all claims, event and NOT_ISSUED launch intent. Enforce one unsealed attempt per step revision. Commit or roll back all of it.
4. Dispatcher owns that attempt's launch token; perform non-mutating final repository/capability/input validation. If preparation requires mutation, it must be an earlier separately claimed administration operation, not hidden here.
5. Recheck gate/stop/revisions. If definitely not issued, revoked reservation seals NOT_STARTED and releases claims. Otherwise mark STARTING and launch intent ISSUED_UNKNOWN in a transaction before spawn.
6. Spawn with explicit cwd/environment, unique output bindings and process group; record verified process identity. Never hold DB transaction across spawn. A failure with no reliable “no child created” evidence enters RECOVERY_REQUIRED.
7. A stop racing spawn routes any created child into stop before normal work proceeds. On output/exit, tag observations with attempt/process generation/epoch. Seal outcome/evidence before safe resource release; wake eligible dependents through the evaluator.

The unavoidable gap between issue journal and process identity registration is **ambiguous**, including crash before actual spawn. No replacement may start until recovery inspection resolves it. Generation tokens prevent stale DB writes; they do not kill old processes or prevent a repeated HTTP side effect. Exactly-once launch/external effects are not claimed.

## Configuration, retries and cancellation

Configuration changes are revisioned human commands. Positive finite capacities required; capacity zero is a deliberate pause for that class. Raising a cap changes admission policy within the approved execution envelope; it is not permission to broaden filesystem/tools/network scope. Lowering cap below occupancy stops new admissions and displays over-target occupancy, without killing work. Quarantined occupancy counts.

Task cancel sets CANCEL intent first, invalidates NOT_ISSUED launches and requests stop for matching live attempts. Step HOLD stops future admissions but does not cancel active work unless command scope explicitly requests it. Confirmed GUI shutdown adds a global admission gate and stop reasons for all in-flight work; it preserves steps for continuation rather than permanently cancelling them. Cancel-close occurs before any stop is requested. Cancellation is handled before backoff/cooldown checks.

Upstream failure blocks downstream with a dependency reason; do not recursively mark every downstream step permanently failed. Independent branches may continue unless feature/user policy holds them. Ineligible prerequisites cannot consume output from the wrong attempt. Repair and agent retry require a new bounded, authorized attempt; no infinite failure loop. Read-only observation can retry on a deterministic deadline, while issued-unknown effects cannot. A supported provider rate-limit event yields waiting/needs-user with no account/provider/key substitution.

## Crash recovery and synchronization

One OS-held singleton lock protects the helper lifetime. Persist engine epoch before any admission; old callbacks cannot mutate current canonical state. On boot, admission remains closed while inspecting all unsealed attempts, launch intents, claims, worktree bindings and close operations. Known-finished results keep their immutable identity; queued steps stay queued/paused until user chooses to continue queue.

Lease deadlines/heartbeats are liveness hints only. If the old engine died but a provider child remains, quarantine its resource set. Stop the verified child/tree if possible; absence of a provider terminal event is not process death. A PID mismatch/reused PID must never be signalled. Missing process-start identity or insufficient permissions means UNKNOWN, not absent. No automatic takeover because a timeout elapsed.

No live-stream reattachment is promised in Phase 1. Inspect and stop surviving verified children, then offer a new validated attempt. Cross-repo work may use remaining slots only when the affected resource set is known and no global recovery barrier remains. Unknown launch identity consumes its original slot and quarantines its repository/worktree; unbounded ambiguity or DB corruption holds the entire engine. Mac sleep invalidates stale external facts and delays; reobserve before admissions, never release a lease based on wall-clock jump alone.

## Required deterministic scenarios

| Scenario | Required observation |
|---|---|
| Cap one, two eligible jobs | One invocation; second waits without losing queue age |
| Cap two, Claude repo A + Codex repo B | Both fake processes start before either is released by test barrier |
| Same repo cap two, pre-provisioned worktrees | Separate writer/output ownership; no common working directory |
| Alias paths into one Git common-dir | Shared repo capacity/admin gate, no duplicate independent identity |
| Same-repo exclusive admin waiting | Existing shared runs drain, new shared admissions wait, repo B continues |
| Competing admits for same step | One reservation/result lineage; loser retries observation only |
| Three resource-starvation skips | Logical drain barrier eventually services eligible large request; no partial claims |
| PR opened / unknown CI / expired deployment | Dependent never starts |
| Approval change after reservation or during run | No start if unissued; stop and ineligible output if already running |
| Cancel in cooldown, reservation, spawn and exit race | Durable ordered outcome; no new attempts without authorization |
| Crash with old provider alive or PID reused | Quarantine; no unsafe kill or replacement |
| Cap shrink/restart/duplicate event | No over-admission, duplicate result or stale lease release |

These scenarios become Phase 1 tests with fake clock, barriers, fake facts and temporary Git repositories. Tests must prove parallel-capable semantics even while the shipped default is one. Real-provider concurrency remains subject to supported CLI behavior and subscription limits; no such jobs run in this phase.
