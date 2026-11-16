# Design and roadmap differences

Written 2026-11-15. This is the record of where the Claude Design canvas and the source-first roadmap disagree, what was chosen, and how to change each choice later. Nothing here is implemented yet.

- Design: "Jojo macOS" canvas, https://claude.ai/artifact/UuyW4DFxPEciFKZm8ZGM24 (version `1794756651-93ae`).
- Roadmap: [SOURCE_FIRST_REUSE_MAP.md](SOURCE_FIRST_REUSE_MAP.md), steps M0–M6.

How the design was read: the text content of the 14 Product artboards and the Components and States references. The layouts were not rendered. The Light theme, Explorations and Themes-and-tokens artboards were not read. Statements about what Jojo has today come from the protocol command list and module names, not from a per-feature code check.

## 1. Decisions where the two disagreed

### D1. Approval model

| | |
|---|---|
| Design | Approval Inbox with six kinds, each approved on its own: architecture decision, specification, execution plan, external action (push), baseline change, scope expansion. "There is no approve-all." |
| Roadmap, first version | One approval of the whole generated bundle (PRD, ADR, plan, validation plan). |
| **Chosen** | Four kinds, no approve-all: **spec** (PRD and ADR together), **plan**, **push**, **PR**. Baseline change and scope expansion appear as "not available yet" until their features exist. |
| Why | Keeps the design's inbox and its exact-revision binding while removing two of the four clicks Jojo needs today before a run. |
| To move toward the design | Split spec back into PRD and ADR approvals. Jojo already approves them separately today, so this is a UI change only. |
| To move toward the first roadmap version | Add a "approve spec and plan together" action. The inbox text "no approve-all" would have to change. |
| Affects | M2, M5, Approval Inbox, Specifications. |

### D2. What happens when a task fails

| | |
|---|---|
| Design | Jojo does not retry by itself. The task stops and the human picks a recovery action. The validator is a command runner only; there is no reviewing agent. |
| Roadmap | M4: after a failed validation Jojo retries with the failure output, and a second agent of the other provider reviews the change. |
| **Chosen** | Automatic repair, **at most two tries**, then a human-review hold. Cross-provider review stays. External actions (push, PR) are never retried automatically, exactly as the design says. |
| Why | The stated goal is "failed work triggers bounded repair or human review". Without it every failing test needs the human. |
| Design additions needed | A "Repairing" operational state; a repair counter (for example "repair 1 of 2") and a review verdict line in Agent Runs; the Settings line "Validator: command runner only · no agent" must change. |
| To move toward the design | Set the repair limit to 0 and switch the reviewer off. Both should be settings, so no code is removed. |
| Affects | M4, Agent Runs, Components (status model), Settings. |

### D3. Finding the Claude Code and Codex executables

| | |
|---|---|
| Design | Found on PATH through the login shell, re-checked before each attempt. |
| Jojo today | An explicit path and file digest are configured by hand; any change is refused. |
| **Chosen** | Automatic discovery as in the design. The version and digest found are recorded with every run. A changed binary shows a warning in Settings instead of blocking. |
| Why | A CLI update should not need manual reconfiguration. The record keeps runs traceable. |
| To move back | Make the warning a refusal again (the current behaviour). |
| Affects | M1, Settings & Providers. |

### D4. Parallel capacity (open)

The design's Settings screen shows 3 parallel attempts overall and 2 per repository. The roadmap proposes 2 for the demo, one per provider. Not decided. It is a configuration value in Jojo's scheduler either way.

## 2. In the design, not in the first roadmap

| Item | What the design shows | Decision | Where |
|---|---|---|---|
| Feature Chat | Jojo asks questions, the human answers, the PRD draft follows the answers. A question can block one task only. | **Before the demo** | M2 |
| Project record | A project owns repositories, test and CI commands, and instructions given to every agent session. | **Before the demo** | Start of M3 |
| Default provider per task kind | Backend tasks to Claude Code, mobile tasks to Codex. | **Before the demo**, as a simple setting | M3 |
| Scope-expansion approval during a run | A write outside the approved scope is refused and sent for approval while the agent waits. | After the demo. Today Jojo detects this only after the run. Routing a tool permission to a human in headless mode needs its own mechanism. | Phase 4 |
| Changing the plan while it runs | Plan rev 2 runs while rev 3 is proposed and adds a task. | After the demo. In the demo a plan is approved once and runs. | Phase 4 |
| Checkpoints, token usage | Named checkpoints per attempt; tokens per session. | After the demo | Phase 4 |
| Request Safe Stop | Ask the agent to finish its step and exit. | After the demo. `codex exec` has no interrupt channel; whether Claude Code has one is not verified. | Phase 4 |
| Continue with History | Resume the provider session. | After the demo (session continuation) | Phase 4 |
| Spikes, assumption tracking, decision reuse, transcript export | Parts of Feature Chat. | After the demo | Phase 4 |
| Deployment gate, Maestro flows, snapshot baselines | Gates in Execution Plan, suites in Verification. | Old roadmap Phase 6 | Phase 6 |
| OSV-Scanner, backups, evidence retention | Verification and Settings. | Old roadmap Phase 7 | Phase 7 |
| Figma link import | Marked "not available yet" in the design itself. | Old roadmap Phase 5 | Phase 5 |

To pull any "after the demo" item forward, move its row to the M step named in the screen table below and add it to that step's done-when line in the roadmap.

Feature Chat without session continuation: each question round is a new run that receives the questions and answers so far. It looks like a conversation, but the agent re-reads the repository every round, so rounds are slow until continuation exists.

## 3. In the roadmap, not in the design

| Item | Needed design addition |
|---|---|
| Automatic repair and reviewing agent | See D2. |
| Plan branch that integrates accepted steps | Source Control shows one branch per task. A plan-level branch and its diff need a place. |
| Generated PR title and body | Source Control has "Create Draft PR…" but no preview of the text. |
| Weft and other third-party provenance | None in the app; a document only. |

## 4. Screen to step mapping

| Screen | Filled by | Design wave |
|---|---|---|
| App shell, sidebar, status model | Refactor R-01 | 1 |
| Recovery Center, launch review | Existing recovery commands | 1 |
| Agent Runs (one attempt) | Existing log, events, cancel | 1 |
| Source Control (local diff, scope) | Existing candidate diff | 1 |
| Verification (one command) | Existing validator | 1 |
| Approval Inbox (spec, plan) | Existing revision-bound approvals | 1 |
| Specifications | Existing artifact revisions | 1 |
| Settings & Providers | Existing config; Claude after M1 | 1 |
| Activity & Audit | Existing event journal (hash chain not verified) | 1 |
| New Feature, Feature Chat | M2 | 2 |
| Projects, Execution Plan, Dashboard | M3 | 3 |
| Repair state and review verdict | M4 | with M4 |
| Push and draft PR | M5 | 4 |

## 5. Engineering decisions outside the design

### E1. Stopping agent child processes that left the process group (M0)

| | |
|---|---|
| Finding | Real Codex starts its tool processes in their own process groups (P2-05; also `codex-rs/core/src/code_mode_host.rs:255` in the pinned Codex source). Jojo already finds them by parent chain (`Supervised::discover`) and already has fixtures for the case (`escaped-known`, `escaped-holder`). It refuses to signal them: `signal_verified` returns a refusal for every live process outside the owned group, and about six tests assert that refusal. |
| Jojo today | Escaped descendants are recorded and never signalled; the stop is reported as unconfirmed. Reason: between reading a PID and signalling it, the PID could be reused by an unrelated process. |
| **Chosen** | Signal an escaped descendant when a snapshot taken immediately before the signal shows the same PID, start time and boot identity as recorded. Order: stop the process (STOP), check identity again, then KILL; if the identity no longer matches, resume it (CONT) and refuse. When the escaped process leads its own group, signal that group. The graceful INT → TERM → KILL sequence for the agent stays first. Anything still alive is reported as an unconfirmed stop, as today. The Agent Guard is extended the same way. Inspection keeps using `/bin/ps`. |
| Why | Without this, Codex tool processes can never be stopped and live runs stay closed. Other tools (Codex itself, agent-orchestrator, weft, commandcenter) signal such processes with weaker or no identity checks. PID plus start time is the fix proposed for a real PID-reuse kill reported against Claude Code. |
| Remaining risk | `ps` reports start time to the second. A PID reused within the same second and within the check-to-signal window would be stopped briefly, and killed only if the second check also matched. Cleanup on macOS is best effort: there is no kernel-enforced process-tree containment. |
| Tests that change | The assertions of refusal in `t09_process.rs` and `p2_04_1_guard.rs` for live, identity-matching escaped descendants. Refusal for a wrong start time, wrong boot or reused PID stays. |
| To move back | Restore the unconditional refusal in `signal_verified`; escaped descendants are then recorded only. |

### E2. Deferred: inspect processes by system call instead of `ps`

| | |
|---|---|
| Idea | Read the process table with macOS system calls (as weft does with `proc_pidinfo`) instead of running `/bin/ps`. |
| Benefits | Start time to the microsecond (stronger identity check); a check-to-signal window of microseconds instead of tens of milliseconds; works when the machine is at its process limit; faster. |
| Costs | Changes the "no unsafe, no FFI" rule or adds dependencies to the allowlist; the boundary guard and its tests change; many process tests inject a fake `ps`, and the Agent Guard takes the `ps` path as an argument. |
| **Chosen** | Not in M0. Planned for Phase 4, after the demo. The inspector is already a replaceable part. |
| Performance note | Not measured in Jojo. commandcenter measured about 60 ms per `ps` snapshot on a machine with 1300 processes. Snapshots are taken when stopping, not while the agent runs, so this is not a reason to do E2 early. |

### E3. Refactor R-03: one data-driven gate

| | |
|---|---|
| Built | `scripts/verify.py` (runner), `scripts/gates/current.json` (what the gate requires), `scripts/tests/test_verify.py` (the gate fails for each check class). It imports none of the `verify_p*.py` scripts; those stay as historical records. |
| How to run | `python3 scripts/verify.py` (all stages) or `python3 scripts/verify.py --skip gui`. Result: `verification/gate-m0-result.json` and `.log`. A skipped stage gives `passed: true, complete: false`. |
| Differs from REFACTOR_PLAN R-03 | The plan asked the new gate to reproduce the P2-04.1 result on the P2-04.1 tree. That tree no longer exists (M0 changed it), so this was not done. The gate requires minimum counts and named tests instead of exact inventories. |
| Not carried over | The twelve P2-04 rehearsal scenarios (`scripts/gui_p2_04.py`); the per-milestone coverage maps, launch-site inventory and allowed-changed-files lists. |
| Known open point | One of the first three runs failed while compiling `libsqlite3-sys` a second time during the test step (missing generated `bindgen.rs`). Root cause not found. The compile order was changed to the one the milestone gates used (`cargo test --no-run`, then the helper) with the environment fixed before the first compile; three consecutive runs then passed with a single compile each. If it returns, start from `verification/gate-m0.log`. |
| Found by the gate | The M0 test `t09_known_escaped_child_is_signalled…` called `/bin/ps` directly, which the gate sandbox forbids. It passed outside the sandbox and failed inside it. Fixed to use the injected inspector. |

### E4. Refactor R-01: typed protocol messages

| | |
|---|---|
| Built | `crates/jojo-domain/src/protocol.rs`: `RequestKind`, the `SPECS` table (name, class, minor, queue, field-set strictness, fields) and one payload type per request. `crates/jojo-helper/src/service.rs` dispatches on the enum and reads each payload through its type; Hello's lists and the control-queue rule come from the table. Swift: `macos/JojoApp/Sources/JojoCore/Protocol.swift` (`Request`, `RecoveryAssessment`); every call in the app goes through `Request`. |
| The one wire change | `Recovery` rows for frames of minor ≥ 1 carry `blockers`, `blockers_complete`, `process_absent` and `choices` in addition to the unchanged `assessment` text. The minor version stays 1. Documented in `docs/architecture/STDIO_PROTOCOL.md`. |
| How "identical" was checked | Before any production change, 168 frames were sent through the old `Service` and their answers recorded in `crates/jojo-storage/tests/r01_frames.rs`, including frames with two faults so that the order of the checks is pinned. The test passes unchanged on the new code. To keep that order, each payload is parsed at the point where the helper used to read its first field, not before the session, epoch and replay checks. |
| Differs from REFACTOR_PLAN R-01 | The plan says payloads are "parsed once at the boundary". Parsing everything before the handshake, epoch, compatibility and replay checks would change which error a frame with two faults gets, so parsing happens once per request but inside its handler. The structured blockers are bounded (8 entries of 256 characters); the plan did not say so. |
| Client behaviour that changed | A request whose payload is not exactly its fields is not sent; the caller gets `InvalidClientPayload`. The UI-test condition `choices=` compares the structured choices and no longer searches the text. The Recovery tab still shows the `assessment` text and still offers all four choice buttons. |
| Reverse | `git revert` the R-01 commits (baseline: `1f6d981`). The wire addition can be dropped alone by removing the `structured` branch in the `Recovery` arm of `service.rs`; the client then shows rows without structured fields, as it does for an older helper. |
| Not done | `handle` and `slice_command` are shorter but still long; splitting them is R-02. The twelve P2-04 rehearsal scenarios are still outside the gate. |

## 6. Change log

| Date | Change |
|---|---|
| 2026-11-15 | First version. D1–D3 decided; D4 open. Feature Chat and the project record moved before the demo. |
| 2026-11-15 | Added E1 (signal escaped descendants after an identity check, using `ps`) and E2 (system-call inspection deferred to Phase 4). |
| 2026-11-15 | M0 code: E1 implemented in `crates/jojo-storage/src/process.rs` (`signal_escaped`, `escaped_from`, `Supervised::signal_verified`, `run_agent_guard`); two tests changed from "not signalled" to "stopped" (`t09_process.rs`, `p2_04_1_guard.rs`). The real Codex 0.169.0 stream is a parser test in `crates/jojo-domain/tests/p2_01_codex.rs` (inlined, because the boundary guard forbids `include_str!`). The intermittent `OwnershipHeld` test failure was a real lock-release race, fixed by an explicit unlock in `OwnershipLock::drop` (`ownership.rs`). Result on the final tree: 395 Rust tests passed, 5 ignored; 87 boundary tests passed; fmt and clippy clean. Swift tests and the `verify_p2_*.py` gate scripts were not run; the input hashes recorded by the P2-04.1 gate no longer match the changed files. Pre-change backup: `~/Projects/jojo-backups/pre-m0-20261115T165918Z.tar.gz`. |
| 2026-11-16 | The human accepted the P2-05 candidate in the app (reported by the human; not checked in the database here). Refactor R-03 added (E3). Gate result without the GUI stage, three consecutive runs: 97 script tests, 396 Rust tests (5 ignored, one of them run explicitly outside the sandbox), 35 Swift tests, no failures. The GUI stage (17 scenarios) has not been run through the new gate yet. |
| 2026-11-16 | Full gate run, nothing skipped: `passed: true, complete: true`. 97 script tests, 396 Rust tests, 35 Swift tests, 17 GUI scenarios (9 + 3 + 5), no failures. Recorded in `verification/gate-m0-result.json`. |
| 2026-11-16 | `git init` (the human chose it); baseline commit `1f6d981`. Refactor R-01 done (E4): typed protocol messages in `jojo-domain`, helper dispatch on `RequestKind`, structured assessment fields in `Recovery` rows for minor ≥ 1, typed requests in the Swift client. Full gate, nothing skipped: `passed: true, complete: true`; 104 script tests, 403 Rust tests, 39 Swift tests, 17 GUI scenarios. Recorded in `verification/gate-r01-result.json`. The twelve P2-04 rehearsal scenarios were run by hand afterwards and passed (`~/JojoDisposable/r01-rehearsal/20261116T034931Z`). |
