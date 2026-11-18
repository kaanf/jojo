# Source-first reuse map and minimal roadmap

Written 2026-11-15. Planning only: no production code, existing report, pin file or gate was changed; no provider was launched; nothing was pushed. This document supersedes the *ordering* of [IMPLEMENTATION_ROADMAP.md](IMPLEMENTATION_ROADMAP.md) for the path to the first multi-agent demo. The earlier reports stay as historical records.

Strategy: **Reuse > Port > Adapt > Build**. Target: objective → approved task plan → Claude Code and Codex execute in dependency order → validation → bounded repair or human review → review-ready changes.

## 0. What was examined, and how far

| Repository | Pinned SHA (HEAD on 2026-11-15) | Language | License as found |
|---|---|---|---|
| [SoloJiang/weft](https://github.com/SoloJiang/weft) | `12303b30e3ccabfb91a116bd0b1658fd859aaed9` | Rust (Tauri) + TS | **Owner permission.** The repository itself carries no LICENSE file and GitHub reports no license (root `Cargo.toml` says MIT for workspace crates only; `src-tauri/` declares none). On 2026-11-15 the human reported that the weft owner gave permission to copy the whole repository. No written record is on file in this project yet. |
| [fabro-sh/fabro](https://github.com/fabro-sh/fabro) | `81bbe204aad0c872e476a4ee2aba4a3d291031d8` | Rust + TS | MIT (`LICENSE.md`, Lithos Computer Co.), `THIRD_PARTY_NOTICES.txt` |
| [OrchestratorInc/agent-orchestrator](https://github.com/OrchestratorInc/agent-orchestrator) | `e4ded08c6f150a632909e84bcc65d8cf772b4c2f` | Go + TS | Apache-2.0 (`LICENSE`; no root NOTICE found) |
| [addyosmani/factory](https://github.com/addyosmani/factory) | `8af116567166a0a16588b7ab1b9934ece0b775bc` | Markdown + shell | MIT (Addy Osmani) |
| [cgeene/commandcenter](https://github.com/cgeene/commandcenter) | `85f39956976ceb03aec668bd5955e5304ddd5dae` | TypeScript | MIT (Caleb Geene) |
| [Kilbex/Vigla](https://github.com/Kilbex/Vigla) (added, already in `research/upstream`) | `bbd19ae2d5a77401502756c550b5dcd4ae59bbc9` | Rust | Apache-2.0 with `NOTICE` |
| [github/spec-kit](https://github.com/github/spec-kit) (added, already in `research/upstream`) | `2ee2b1e8005905db05f5717300a469b3068b908f` | Markdown + Python | MIT (GitHub, Inc.) |

File paths below are relative to each repository root at these SHAs; a permalink is `https://github.com/<owner>/<repo>/blob/<sha>/<path>`.

Depth of this review: directory structure, license files, module-level documentation, public signatures and test counts. Function bodies were read only where quoted. **No upstream test suite was run and no upstream code was executed.** The five new repositories were shallow-cloned to a temporary directory, not into `research/upstream` (that would alter the pin inventory the Phase 1 gates hash). Every "port" below still needs the per-file read that Jojo's own port rule requires.

Why two additions: Vigla is the only reference that is Rust, clearly licensed, already pinned locally and already contains a task DAG scheduler, a bounded recovery policy, a rework arbiter and a Claude stream adapter. spec-kit supplies MIT-licensed PRD/plan/task templates and is already the basis of Jojo's PRD schema.

Three findings that shape everything else:

1. **Weft is a code source by owner permission, not by a published license.** (Updated 2026-11-15; the first version of this document classed weft as design-only.) Its code may now be ported with attribution. It is the closest functional match to the target workflow: lead proposes a plan, human confirms scope, Claude and Codex workers run in worktrees, PR state is tracked. It is still Tokio/Tauri code that uses `libc`, so it is adapted, not dropped in. Keep the owner's permission in writing with the provenance record.
2. **Fabro no longer drives the Claude/Codex CLIs.** Its changelog `docs/public/changelog/2026-05-18.mdx` tells users to replace `backend="cli"` with API-backed execution or an ACP process. It is useful for PR preparation and loop design, not for subscription-CLI dispatch.
3. **Little code is drop-in.** Jojo's core is synchronous, forbids unsafe/FFI and allowlists dependencies; Vigla and fabro are Tokio-based, agent-orchestrator is Go, commandcenter is TypeScript. What is directly reusable is prompts, templates, test cases and a few pure functions. The time saved comes from porting proven logic instead of designing it, and from extending Jojo components that already exist.

## 1. Jojo inventory

### Implemented and verified

"Verified" means an offline gate script passed with fixture processes, unless marked live.

| Area | Where | Evidence |
|---|---|---|
| SQLite authority, single writer, revision CAS, events, outbox, epochs | `jojo-storage` (`schema.rs`, `writer.rs`, `tx.rs`, `engine.rs`) | P1-02 … P1-04 |
| PRD/use-case schema, artifacts, revision-bound approvals | `jojo-domain/prd.rs`, `jojo-storage/artifacts.rs`, `approvals.rs` | P1-03, P1-04 |
| Typed dependency graph, topological order, predicate evaluation | `jojo-domain/dependency.rs` (`Graph::order`, `evaluate_step`), `jojo-storage/dependencies.rs` | P1-05 |
| Resource scheduler with capacities, priorities, cooldowns | `jojo-domain/scheduler.rs`, `jojo-storage/scheduler.rs` (`queue_step`, `schedule`) | P1-06; tests run concurrent mixed-provider attempts |
| Owned worktrees and branches | `jojo-storage/worktrees.rs` | P1-07 |
| Provider contracts, bounded line framing, normalized events | `jojo-domain/provider.rs`, `stream.rs` | P1-08, P1-09 |
| Process supervision, stop budgets, Agent Guard | `jojo-storage/process.rs`, `launch.rs` | P1-09, P2-04.1 |
| Candidate sealing, repository-derived diff, write-scope check | `jojo-storage/outcomes.rs`, `validation.rs` | P1-10 |
| Recovery holds, shutdown | `recovery.rs`, `shutdown.rs` | P1-11, P1-12 |
| Stdio protocol 1.1: `RegisterRepository`, `ImportBundle`, `HumanDecision`, `Start`, `Validate`, `CandidateDiff`, `CandidateDecision`, `RecoveryResolve`, `Log`, `Events` | `jojo-helper/service.rs` | P1-13, P2-02, P2-03 |
| Codex `exec --json` adapter with digest-pinned binary | `jojo-domain/codex.rs`, `jojo-storage/codex.rs` | P2-01 offline; **one live run** (P2-05) |
| Host validation runner on a clean copy of the candidate | `jojo-storage/validator.rs` | P2-03; live once |
| SwiftUI shell: runs, review, start, candidate, recovery, inspector | `macos/JojoApp/Sources` | P1-14, P2-02 … P2-04 GUI scenarios |

### Incomplete

- **P2-05 ended NO-GO.** Real Codex puts every tool process in its own process group, so group signals and the Agent Guard reach only the agent. One file (`/private/tmp/xcrun_db`) was written outside the worktree.
- No accept/reject decision was recorded for the live candidate.
- The real 0.169.0 JSONL was not turned into parser fixtures.
- One Rust test is intermittent (`p2_03_closing_while_validation_is_active…`, `OwnershipHeld`), cause unknown.
- The project is not a Git repository.

### Missing for an end-to-end workflow

| Gap | Current state |
|---|---|
| Lead agent, PRD/task generation | None. `ImportBundle` accepts a hand-written bundle. `Role` has only `Developer` and `Validator`. |
| Claude Code execution | `ProviderKind::ClaudeCode` and a synthetic parser profile exist. No adapter, no argv, no real fixture. |
| Driving a whole plan | `Start` launches one approved step. Nothing walks the plan, queues ready steps or launches them in parallel in production, although the storage scheduler can. |
| Passing work between steps | Each candidate is a sealed snapshot. No integration branch, so a dependent step cannot build on an accepted one. |
| Automatic validation, review, repair | Validation runs only on a human `Validate`. No reviewer role, no retry policy. |
| Review-ready output | No commit, branch hand-off or PR text. |
| Session continuation | DTOs exist; continuation is rejected by design. |
| Progress view | Run rows only. No plan or step view. |

### Preserve, do not rewrite

All four crates and the Swift package. In particular: the storage kernel, `dependency.rs`, both scheduler modules, `worktrees.rs`, `process.rs`, the Codex adapter shape (the Claude adapter should mirror it), `validator.rs`, the candidate/diff pipeline, the stdio protocol and the existing verification scripts (keep them as the regression suite).

## 2. Source-code mapping

Classes: **DIRECT** (copy with notice), **ADAPT** (port or rework into Jojo's synchronous Rust, with attribution), **DESIGN** (read, then write independently), **NEW**.

### 2.1 Lead agent and PRD/task generation

| Source | Files | Class | Notes |
|---|---|---|---|
| spec-kit (MIT) | `templates/commands/specify.md`, `plan.md`, `tasks.md`; `templates/tasks-template.md`, `plan-template.md` | DIRECT (prompt content) | Task template already marks parallel-safe tasks and dependencies. Jojo's PRD schema descends from `spec-template.md`. |
| factory (MIT) | `template/.claude/skills/factory-spec/SKILL.md`, `template/docs/factory/CONTRACT.md` | DIRECT (prompt content) | Spec skill with explicit gates. Tests under `tests/` cover scripts, not prompts. |
| Vigla (Apache-2.0) | `crates/orchestrator/src/mission_supervisor_run/prompts.rs`, `task_graph/descriptor.rs`, `mission.rs` (`MissionSpec::validate`, `normalize_scope_paths`) | ADAPT | Supervisor prompt plus the task descriptor it must emit. 22 tests in `mission.rs`. |
| commandcenter (MIT) | `src/prompts/orchestrator.ts`, `src/daemon/orchestration.ts` | DESIGN | "Main studies the request, then dispatches." Tied to a live tmux session. |
| weft (unclear) | `src-tauri/src/planner.rs` (`ProposedDirection`, `Proposal`, `resolve`, `confirm`), `brief.rs`, `crates/weft-brief/src/lib.rs` | ADAPT | Best model of "lead proposes, human edits scope, confirm creates worktrees". 99 tests in `planner.rs`. |

Fit: the lead is one more provider attempt with a new `Role::Planner`, read-only, whose final message is a JSON bundle parsed by Jojo's strict JSON and fed to the existing `ImportBundle` path as unapproved drafts. Codex has `--output-schema` (observed in P2 finding F5). The equivalent for Claude Code is unverified.

### 2.2 Dependency-aware scheduling

| Source | Files | Class | Notes |
|---|---|---|---|
| Jojo | `dependency.rs`, `scheduler.rs` | **Extend** | Already has ordering, cycle errors, capacities and conflict detection. Missing only a production loop. |
| Vigla | `task_graph/scheduler.rs`, `task_graph/validate.rs` (Kahn, `Dag`, `GraphError`; 13 tests), `mission_supervisor_run/mission_loop.rs`, `run_task.rs` | ADAPT (loop shape), DIRECT (test cases) | Concurrent workers up to `max_parallel_workers`, serial integration. Tokio. |
| commandcenter | `src/daemon/scheduler.ts` (`tick`, `watchdog`, `reworkDispatchSweep`), `src/lib/blockers.ts`; `test/scheduler.test.ts` (1423 lines), `test/blockers.test.ts`, `test/capacity.test.ts` | DESIGN, DIRECT (test scenarios) | Single `blocked_by` chain only, weaker than Jojo's graph. |
| weft | `crates/weft-scheduler/src/lib.rs`, `src-tauri/src/coordinator.rs` | ADAPT | A message-delivery state machine, not a DAG scheduler. Low relevance. |

### 2.3 Claude Code and Codex dispatch

| Source | Files | Class | Notes |
|---|---|---|---|
| Jojo | `jojo-domain/codex.rs`, `jojo-storage/codex.rs` | **Template** | Copy this structure for Claude: fixed argv, digest-pinned binary, environment allowlist, versioned parser profile. |
| Vigla | `crates/adapters/claude/src/lib.rs` (`handle_system`, `handle_assistant`, `handle_tool_use`, `handle_result`, `ingest_line`, `finalize`; 870 lines), `crates/orchestrator/src/vendor_profile.rs`, `resources/vendor_profiles/claude.json` | ADAPT | Reviewed in P1-09. Do not take its bypass flags, and do not let `finalize` invent success. |
| weft | `src-tauri/src/adapters/mod.rs:100-180` (Claude `--input-format/--output-format stream-json --verbose --resume`; Codex `exec --json … resume`), `lead_chat/proto.rs` (36 tests) | ADAPT | Its docs call the three dialects "spike-verified live". Cross-check argv only. |
| commandcenter | `src/daemon/spawn.ts:380-440`, `src/codex-policy.ts` | DESIGN | Interactive tmux panes, not headless. |
| agent-orchestrator | `backend/internal/adapters/agent/claudecode/claudecode.go`, `codex/codex.go`, `codex/turn_failures.go` | DESIGN | Interactive TUI. `codex.go:652` adds `--dangerously-bypass-hook-trust`; `claudecode/local_credential.go` reads credentials. Neither fits Jojo's rules. |
| fabro | none | — | CLI backends removed. |

Process-tree gap from P2-05 (blocks any further live run):

| Source | Files | Class | Notes |
|---|---|---|---|
| commandcenter | `src/daemon/proctree.ts` (`parseProcessTable`, `collectProcessTree`, `selfAncestry`, `terminatePaneTree`, `escalateSurvivors`); `test/proctree.test.ts` (538 lines) | ADAPT | Uses a `ps` snapshot, which is what Jojo's guard already calls. No FFI needed. |
| weft | `src-tauri/src/proc_registry.rs` (module doc; `is_ours`, `reap`; 21 tests) | ADAPT | Documents the same Codex behaviour Jojo measured: descendants get their own groups and a scrubbed environment, so only the parent-PID chain identifies them. Rule: snapshot the descendant closure while the root is alive, then signal each distinct group once. Port the closure logic and test cases; its enumeration and signalling use `libc`, which Jojo forbids, so those parts go through Jojo's existing `ps`-based guard. |
| fabro | `lib/foundation/fabro-proc/src/process.rs`, `signal.rs` | DESIGN | States plainly that "escaped groups" are outside its contract. |
| Vigla | `crates/orchestrator/src/process_tree.rs` | Rejected in P1-09 | Single group kill, unsafe `libc`. |

### 2.4 Multi-repository coordination

| Source | Files | Class | Notes |
|---|---|---|---|
| Jojo | `RepositoryId`, `repository_key`, `SourceVector`, `Predicate` | **Extend** | Per-repository resources and typed cross-step evidence already exist. |
| weft | `planner.rs` (per-repo `ScopeEntry`), `brief.rs` (cross-repo contracts in the worker brief), `curator.rs`, `manifest.rs` (32 tests), `host/judge.rs` (upstream PR must merge first) | ADAPT | The only reference built around multiple repositories. |
| Vigla | `mission_workspace/merge.rs` (`try_rebase_then_ff`, `parse_unmerged`; 5 tests) | ADAPT | Single repository, but this is the missing "integrate accepted step" piece. |
| agent-orchestrator | `backend/internal/adapters/scm/github/stacks.go` | DESIGN | Stacked PRs. |

### 2.5 Automated validation and review

| Source | Files | Class | Notes |
|---|---|---|---|
| Jojo | `validator.rs`, `validation.rs` | **Extend** | Trigger automatically after an attempt settles. |
| factory | `template/.claude/agents/factory-verifier.md`, `factory-critic.md`, `template/.claude/scripts/gates.sh`, `template/.factory/gates.conf`, `template/.factory/scripts/prove-test.sh`; `tests/test-gates.sh`, `test-proof.sh` | DIRECT | Reviewer prompts and a cheap-to-expensive gate ladder. |
| commandcenter | `src/prompts/reviewer.ts` (`buildReviewerPrompt`: same inputs as the worker, none of its conversation), `src/daemon/review.ts` (`maybeAutoReview`, `handleVerdict`); `test/review.test.ts`, `test/reviewloop.test.ts` | DIRECT (prompt text), DESIGN (flow) | |
| Vigla | `audit/test_pass.rs` (12 tests), `audit/scope.rs`, `judgment/verdict.rs`, `acl/check.rs` | ADAPT | Scope gate was already reimplemented in P1-10. |
| agent-orchestrator | `backend/internal/review/planner.go` (`Plan`: one review per PR head SHA), `review/prompt.go` | DESIGN | |
| weft | `src-tauri/src/check.rs` (`infer_checks`, `run_checks`; 9 tests) | ADAPT | Infers check commands from manifests. |

### 2.6 Bounded repair loops

| Source | Files | Class | Notes |
|---|---|---|---|
| Vigla | `recovery/policy.rs` (`RecoveryPolicy`, `recover`; 13 tests), `recovery/classify.rs` (`classify_failure`; 10 tests), `recovery/quota.rs`, `arbiter/rework.rs`, `arbiter/bound.rs` | ADAPT, close to DIRECT | Documented as pure and deterministic, one match arm per failure class. Check imports before copying. |
| fabro | `.fabro/workflows/implement-plan/workflow.fabro` (`verify -> fixup -> verify`, `max_retries`), `lib/components/fabro-workflow/src/operations/retry.rs` | DESIGN | |
| commandcenter | `src/daemon/review.ts` (`reviewMaxCycles`, escalation to the human), `test/rework-dispatch.test.ts`, `test/rework-transition.test.ts` | DESIGN, DIRECT (test scenarios) | |
| agent-orchestrator | `backend/internal/lifecycle/reactions.go` (`reviewMaxNudge = 3`, persisted attempt counts) | DESIGN | |

### 2.7 GitHub PR preparation

| Source | Files | Class | Notes |
|---|---|---|---|
| fabro | `lib/components/fabro-workflow/src/prompts/pr_body.md` | DIRECT | PR title/body prompt sized to the change. |
| fabro | `lib/components/fabro-workflow/src/pull_request.rs` (`build_pr_content`, `open_pull_request`, existing-PR reconcile; 38 tests) | DESIGN | Uses a GitHub App token and fabro's own LLM client; Jojo should use `gh` and a provider attempt. |
| commandcenter | `src/daemon/worktree.ts` (`pushQuiet`: never `--force`, `GIT_TERMINAL_PROMPT=0`, timeout), `src/daemon/prsync.ts` (`fetchPrState`, `computeCheckRollup`); `test/prsync.test.ts`, `test/publication.test.ts` | ADAPT | Draft PR first; also has a "leave uncommitted, human publishes" mode. |
| weft | `src-tauri/src/host/github.rs` (`parse_pr_json`; 30 tests), `host/judge.rs`, `host/gate.rs` | ADAPT | `gh` with the user's own login. |
| Jojo | `journal_attempt_effect` / `issue_attempt_effect` in `outcomes.rs` | **Extend** | Already gives idempotency for one remote effect. |

### 2.8 Session continuation and recovery

| Source | Files | Class | Notes |
|---|---|---|---|
| Jojo | `recovery.rs`, `SessionRef`, `Continuation`, protocol `ContinueFreshContext` / `ContinueWithHistory` | **Extend** | Crash recovery works. Provider resume is rejected. |
| commandcenter | `src/daemon/taskresume.ts` (`resumeReviewedWorker`, `resumeArchivedTask`); `test/taskresume.test.ts`, `test/worker-resume.test.ts` | ADAPT | |
| agent-orchestrator | `adapters/agent/claudecode/continuation.go`, `codex/continuation.go` | DESIGN | |
| Vigla | `supervisor/adapter_supervision.rs` (`capture_session_id_once`), `recovery/wakeup.rs` | ADAPT | |
| weft | `src-tauri/src/lead_chat/revive.rs`, `checkpoint.rs` (shadow-repo snapshots; 25 tests) | ADAPT | |

### 2.9 SwiftUI progress visualization

No reference contains Swift. All five use web front ends.

| Source | Files | Class | Notes |
|---|---|---|---|
| commandcenter | `src/lib/board.ts`, `src/lib/task-priority.ts` | DESIGN | Pure column/ordering helpers. |
| fabro | `docs/public/images/runs-board.png`, `apps/fabro-web` | DESIGN | Working / Pending / Verify / Merge board. |
| Jojo | `Views.swift` (`RunsView`, `CandidateView`), `Workspace.swift` | **Extend**, NEW views | |

### 2.10 Summary by class

- **Directly reusable:** spec-kit command and task templates; factory spec, verifier and critic prompts and gate scripts; commandcenter reviewer prompt; fabro `pr_body.md`; upstream test scenarios as case lists.
- **Requires adaptation:** Vigla Claude adapter, recovery policy and classifier, rebase-then-fast-forward integration, task-loop shape; weft planner proposal types, Claude/Codex argv and stream parsing, process-closure logic, check inference, `gh` PR parsing and merge-readiness judgement; commandcenter process-tree teardown, push/PR sync, resume.
- **Design only:** agent-orchestrator; fabro's Rust code; commandcenter's tmux and scheduler code.
- **Genuinely new:** the helper's plan-driving loop over Jojo's own scheduler; `Role::Planner` and `Role::Reviewer` wiring; SwiftUI plan and step views; the provenance file.

## 3. Minimal integration roadmap

Each step extends an existing Jojo component. "Read first" lists the upstream code to examine before writing anything.

| # | Step | Jojo component extended | Read first | Done when |
|---|---|---|---|---|
| M0 | Unblock live runs: stop and account for descendants by parent-PID closure; add the real Codex JSONL as fixtures; fix or quarantine the flaky test | `process.rs`, Agent Guard, P2-01 parser tests | commandcenter `proctree.ts` + its test; weft `proc_registry.rs` module doc | A fixture that reproduces the P2-05 tree (children in their own groups) leaves no survivor after a stop |
| M1 | Claude Code adapter, headless. Executables are discovered automatically; version and digest are recorded per run, and a change raises a warning instead of a refusal | new `claude.rs` beside `codex.rs` in domain and storage; `stream.rs` profile | Vigla `adapters/claude/src/lib.rs`; weft `adapters/mod.rs:100-125`; official headless docs | Golden argv, fixture stream parsed, then one live run on the sample repository |
| M2 | Lead agent with question rounds (Feature Chat): reads the repositories read-only, asks questions, updates the PRD draft from each answer, then drafts the plan. Each round is a fresh run carrying the questions and answers so far | `Role::Planner`; existing clarification questions/answers in `artifacts.rs`; `slice.rs` `FeatureBundle` / `ImportBundle`; separate spec and plan approvals | spec-kit `commands/specify.md`, `plan.md`, `tasks.md`; factory `factory-spec/SKILL.md`; Vigla `prompts.rs`, `descriptor.rs`; weft `planner.rs` | Typing an objective leads to questions in Feature Chat, including one that blocks a single task only; the answers yield a PRD and a multi-step draft plan with dependencies, write scopes and a validation command, each approved in the Approval Inbox |
| M3 | Project record first (repositories, test commands, instructions, default provider per task kind). Then the plan runner: queue every ready step, launch up to capacity, pick the provider per step, integrate accepted steps onto a plan branch | `service.rs` (new `RunPlan`), `scheduler.rs`, `dependencies.rs`, `worktrees.rs` | Vigla `task_graph/scheduler.rs`, `mission_loop.rs`, `mission_workspace/merge.rs`; commandcenter `test/scheduler.test.ts` | A three-step plan (two independent, one dependent) runs with Codex and Claude in parallel, then the dependent step |
| M4 | Automatic validation, cross-provider review, bounded repair (two tries, then a human hold). External actions are never retried automatically | `validator.rs` trigger on settle; `Role::Reviewer`; retry counter on the step | Vigla `recovery/policy.rs`, `classify.rs`, `arbiter/rework.rs`; commandcenter `prompts/reviewer.ts`, `review.ts`; factory verifier and critic prompts | A failing step is retried with the failure output at most twice, then stops in a visible human-review hold |
| M5 | Review-ready hand-off (push and PR are separate approvals): plan branch with commits, diff, generated PR title and body; `gh pr create --draft` only on an explicit button | `outcomes.rs` effect journal; `CandidateView` | fabro `pr_body.md`, `pull_request.rs`; commandcenter `worktree.ts` `pushQuiet`, `prsync.ts` | The user gets a local branch and PR text; pushing is a separate, confirmed action |
| M6 | Claude Design migration, in waves alongside M1–M5 (see below), including plan and step progress | `Views.swift`, `Workspace.swift`, `Snapshot`/`Events` projections | commandcenter `board.ts`; fabro runs board | Steps show queued, running, validating, repairing, needs-human, done, with logs per step |

Steps were first numbered R0–R6. They were renamed M0–M6 on 2026-11-15 because [REFACTOR_PLAN.md](../planning/REFACTOR_PLAN.md) already uses R-01 … R-03.

**The demo is complete at M5.**

### Order, with the refactor and the design migration

M0 is the close-out of Phase 2; M1–M6 are Phase 3.

1. M0.
2. Refactor R-03 (one data-driven gate), then Refactor R-01 (typed protocol messages). M2–M5 add protocol commands, and the new interface must not be built on debug-text matching.
3. Design wave 1, in parallel with M1: app shell, sidebar, status model, and the screens whose data exists today (Recovery Center and launch review, Agent Runs, Source Control local diff and scope, Verification with one command, Approval Inbox for spec and plan, Specifications, Settings, Activity).
4. M1.
5. M2 with design wave 2: New Feature, Feature Chat.
6. M3 with design wave 3: Projects, Execution Plan, Dashboard.
7. M4, with the design additions for repair (a "Repairing" state, a repair counter and a review verdict in Agent Runs).
8. M5 with design wave 4: push and draft-PR parts of Source Control and Approval Inbox.

**Order from 2026-11-17.** Steps 1 to 7 are done. The human decided to build the canvas's current Product page (the `…Mac.dc.html` boards, macOS 26 Liquid Glass) as drawn and to extend the core for the data it shows. Step 8 is replaced by:

8. The Liquid Glass migration: tokens, materials, the glass shell and the status components (`docs/design/DESIGN_SYSTEM.md`, `COMPONENTS.md`). Done 2026-11-17.
9. User-facing data in the core: names of features, tasks and documents; the live state of an attempt; pending approvals; time and actor of events. Done 2026-11-17: protocol 1.6, no schema change (DESIGN_ROADMAP_DIFFERENCES.md, E11).
10. Dashboard. Done 2026-11-17: protocol 1.7 (`Features`; more fields of `Attempts` and `Events`), no schema change; the app opens on it (DESIGN_ROADMAP_DIFFERENCES.md, E12).
11. The screens in flow order, each from its `…Mac.dc.html` board: New Feature (done 2026-11-17, E13), Feature Chat (done 2026-11-17, protocol 1.8, E14), Specifications, Approval Inbox, Execution Plan, Agent Runs, Source Control, Verification & Evidence (done 2026-11-17: protocol 1.9 with `Document`, `Decisions`, `PlanDetail`, `AttemptDetail`, no schema change; the app is light; E15). Done.
12. M5 with design wave 4 (push and draft PR on the rebuilt Source Control and Approval Inbox). Done 2026-11-17: schema 16, protocol 1.10 (`ExternalActions`, `RequestExternal`, `AuthorizeExternal`, `InspectExternal`, `ResolveExternal`); the plan branch of a finished run is pushed to `origin` with `/usr/bin/git` and its draft pull request opened with `gh`, each after its own authorization, once, resolved only by an observation; unknown outcomes wait in the Recovery Center (DESIGN_ROADMAP_DIFFERENCES.md, E16). Built after this map's notes on commandcenter's `pushQuiet` (never `--force`, `GIT_TERMINAL_PROMPT=0`, a timeout) and fabro's PR body; no upstream code was copied or cloned. The existing `outbox` effect journal was extended. **The demo is complete.**
13. Projects, Recovery Center, Activity & Audit, Settings & Providers. Done 2026-11-18: protocol 1.11 (`RecoveryDetail`, `ProjectDetail`; events with a category and a project), derived, no schema change; Settings › Appearance is the app's one setting (DESIGN_ROADMAP_DIFFERENCES.md, E17). Onboarding and New Project stay deferred by the human. **Next:** the human chooses (STATUS.md, "Next step").

Refactor R-02 (splitting `supervise` and `spawn_guarded`) follows the demo; the helper part of it happens while M3 touches `service.rs`.

The design is the "Jojo macOS" canvas (https://claude.ai/artifact/UuyW4DFxPEciFKZm8ZGM24). Where the design and this roadmap disagreed, the choices made and how to reverse them are recorded in [DESIGN_ROADMAP_DIFFERENCES.md](DESIGN_ROADMAP_DIFFERENCES.md).

After the demo, in this order: session continuation (2.8), a second repository in one plan (2.4), PR status sync, then the deferred phases of the old roadmap (Figma, deployment gates, packaging).

Order rationale: M0 is the only item that blocks every later live run. M1 before M2 so the lead can run on either CLI. M3 is the largest piece and is mostly new glue over verified parts. Neither repair (M4) nor the question rounds (M2) need session resume: each is a fresh run whose prompt carries the earlier output. Rounds will be slower until session continuation arrives after the demo.

## 4. Changes to working rules

These follow the stated engineering rules and need the human's decision; none is assumed here.

| Topic | Today | Proposed |
|---|---|---|
| Authorization | A separate GO per milestone and per live attempt | One standing authorization for runs inside registered disposable or explicitly registered repositories, within a set attempt and time budget |
| Approvals inside a run | PRD, ADR, plan and validation plan each approved separately | **Decided 2026-11-15:** four approval kinds, no approve-all: spec (PRD and ADR together), plan, push, PR. Edits re-open the approval |
| Still always human | — | Push, PR creation, merge; registering a repository; anything touching credentials; raising budgets; resolving a repair-limit hold |
| P2-05 stop condition "write outside the worktree" | Any such write halts | **Decided 2026-11-18:** treat writes that the provider's own sandbox allows (`/tmp`, `$TMPDIR`) as recorded observations. The process-group gap is the demonstrated risk and gets the fix (M0) |
| Verification | Gate script and report per milestone, hashes over all inputs | Keep the suites as regression tests run per step; one short report per roadmap step |
| Provenance | `research/REPOSITORY_PINS.json` for nine checkouts | Add the five new pins and a `THIRD_PARTY_NOTICES` file before the first ported line lands; each ported file names its source path and SHA |
| Version control | Project is not a Git repository | Initialize one locally (human action). Provenance and review of Jojo's own changes depend on it |

License duties when code or prompt text is taken:

- MIT (fabro, factory, commandcenter, spec-kit): keep the copyright and permission notice with the copied portion.
- Apache-2.0 (Vigla, agent-orchestrator): keep the license, carry Vigla's `NOTICE`, and mark modified files as changed.
- weft: copied under the owner's permission. Record who granted it, when and in what words in the provenance file, and name weft and the source path in each ported file. A LICENSE file upstream would still be the durable form, and matters if Jojo is ever distributed (U08).
- Distribution beyond personal use is still open decision U08.

## 5. Recommendation

Build M0 → M5 on the existing Rust core and stdio protocol, using Vigla and weft as the code sources (weft first for the planner, CLI dispatch and PR handling; Vigla first for scheduling, recovery policy and integration), and spec-kit, factory, commandcenter and fabro for prompts and test cases. Do not add a new subsystem: the lead, the reviewer and the repair attempt are all ordinary provider attempts with a different role and prompt, scheduled by the scheduler Jojo already has.

Decided on 2026-11-15: the approval model, automatic repair with a limit of two, automatic executable discovery, Feature Chat and the project record before the demo (details in [DESIGN_ROADMAP_DIFFERENCES.md](DESIGN_ROADMAP_DIFFERENCES.md)).

Still open before M0 starts:

1. The other rule changes in section 4: standing authorization, the `/tmp` stop condition, lighter verification reports, initializing Git.
2. Save the weft owner's permission in writing (message or upstream LICENSE) for the provenance record.
3. Parallel capacity for the demo (2 proposed: one Codex, one Claude; the design's Settings screen shows 3 overall and 2 per repository).
