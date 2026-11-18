# Execution Plan

Source: the canvas "Jojo macOS", page "Product", board "Execution Plan" (`ExecutionPlanMac.dc.html`, 1440 × 1300, light by default). Read on 2026-11-17 from version `1794942048-8ee3`; the board file is byte-identical to the copy read at version `1794922102-b518` (sha256 `fa85a70d…f2b6`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). The earlier document (design waves 1 and 3, sections 7 and 8) is in the git history (this file at commit `921322a`).

Built as the fifth screen of step 4 of the Liquid Glass order (STATUS.md), with protocol 1.9 (DESIGN_ROADMAP_DIFFERENCES.md, E15).

## 1. The board

| Element | Board | Design system |
|---|---|---|
| Header | "Execution Plan" (20 bold), "Crew › FT-142 Lobby invite links"; a capsule "Review Plan", "Approve Plan", "Pause Admission"; primary "Start Eligible Tasks · 1" | header row (4.3) |
| Lifecycle card | Planning current, the three count pills | lifecycle stepper (2.1) |
| Filters (180 to 200) | "Filters" with "Reset"; check boxes by Repository, Provider (Claude Code, Codex, Unassigned) and Status (Running, Blocked, Eligible, Completed) with counts; a segmented Wave (All, 1 to 4); "Plan checks" (No cycles, No missing dependencies, "1 file conflict: T4 and T6") | check box, segmented control |
| Revision banner | blue: "Plan rev 2 is approved. Rev 3 is proposed and adds T6." with "Compare rev 2 → 3" and "Review Plan" | banner |
| Grid (card, radius 16) | columns "Wave 1", "Wave 2", "Gates", "Wave 3", "Wave 4"; a lane per repository; task nodes (key, state pill, "Eligible", title, provider or "4 blockers"); gate nodes on `grayBg` (Approval gate "0 of 2 approved", Deployment gate "0 of 4 checks", Verification gate "Not run") | task node, gate |
| Legend | Dependency met, Dependency pending, dashed gate, "← → move between nodes · Space inspect" | — |
| Inspector (glass, 300 to 340) | a task: "Task · repo · Plan rev n", "T4 · title", pills; "Why it's blocked · n" on `redBg`; a definition card (Description, Acceptance, Provider, File scope, Dependencies with states, Approvals, History, Verification); buttons (Inspect Attempt, View Logs, Start Task, View Evidence, View Blockers, Answer Q4, Review scope request). A gate: Required evidence, Unblocks, How it opens | inspector (4.4) |

## 2. Where each value comes from

| Element | Source | Since |
|---|---|---|
| Plan | the held feature's newest `ExecutionPlan` document (`Documents`); several plans give a chooser | 1.6 |
| Steps: key, title, kind, provider, prerequisites, wave | `PlanDetail` (the lead plan's steps, the stored plan's edges and resources; the wave is derived from the edges) | **1.9** |
| File scope, verification command, test inventory | `PlanDetail`: the approved validation plan's entry of the step | **1.9** |
| State | `PlanDetail`: the run's step state (`PlanRun`), else derived from the step's newest attempt | **1.9** |
| Eligible, blockers | `PlanDetail`: unapproved revision, open blocking step questions, prerequisites not accepted or integrated, the run's detail of a held step | **1.9** |
| History | `PlanDetail.attempts`: ordinal, outcome, host validation, acceptance | **1.9** |
| Plan checks | `PlanDetail.checks`: cycle, missing prerequisites, overlapping write scopes | **1.9** |
| Approvals | the plan's approval (`PlanDetail.approval`) | 1.9 |
| Revision banner, approval gate | drawn while the newest revision is not approved | 1.9 |
| Verification gate | how many steps' newest attempts passed host validation | 1.9 |
| Start Eligible Tasks | `RunPlan` without a run, `ControlPlan Resume` for a paused one | 1.3 |
| Pause Admission | `ControlPlan Pause` (Resume Admission for a paused run) | 1.3 |
| Approve Plan | `HumanDecision` for the plan's `ReviewBundle` | 1.1 |
| Start Task | `Start` with the approved plan's subjects (`ApprovedPlan`), without a run | 1.1 |
| Inspect Attempt, View Logs, View Evidence | Agent Runs and Verification & Evidence with that attempt | — |

### Not built, and why

| Board element | Why |
|---|---|
| Description, Acceptance criteria of a step | A plan step has a key, title and kind only; "Not reported" |
| Deployment gate | The core has no deployment; the legend says "not available yet" |
| Several repositories as lanes | A plan has one repository |
| "Pause Admission" of the whole core | The core has no admission command; the button pauses the plan's run |
| Keyboard navigation between nodes, Review scope request | Not built; no scope requests in the core |

## 3. Decisions

Decided by the human on 2026-11-17: Start Eligible Tasks is `RunPlan` (or `Resume`), Pause Admission is the run's `ControlPlan Pause`, Approve Plan sends the decision; the approval gate (while the revision is not approved) and the verification gate are drawn, the deployment gate is not.

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/ExecutionPlan.swift`: `ExecutionPlanState`, `PlanFilters`, `PlanNode`, `ExecutionPlanView`, `ExecutionPlanScreen` (reads `Features`, `ApprovedPlan` with each plan's `PlanRun`, `PlanDetail`, and the plan's `ReviewBundle` while it is not approved). The header's buttons and primary action are built in `MainView` from the same state.
- The old Start form with five identities, the graph of approved plans (`PlanGraph`, `TaskNode`, `GraphEdge`, `LaneBand`) and the run section left; their content is in the grid and the inspector.
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S8ExecutionPlanTests` writes `s8-execution-plan-…`, `-blocked-…`, `-proposed-…`, `-verification-gate-…`, `-narrow-…`, `-disconnected-…`.

## 5. What the app shows today / Known gaps

Execution Plan shows the held feature's newest plan by waves with each step's state, provider and blockers, the plan checks and the filters; a step's scope, prerequisites, approvals, attempts and verification command; the approval and verification gates; it runs, pauses and resumes the plan, approves a proposed revision and starts an eligible step by hand. Known gaps:

- Step descriptions, acceptance criteria and deployment gates are not reported.
- Since M6 the inspector follows a running step's log, decides a step in review and starts a held, refused or conflicting step again (section 6). A step started by hand is not retried.
- One repository per plan; a step's keyboard navigation is not built.
- An imported plan's steps have no key or title.

## 6. M6: the live log, the decision and the ways out of a hold

Built in M6 (DESIGN_ROADMAP_DIFFERENCES.md, E19), in the inspector of a step; not on the board, which draws none of these:

| Place | When | What | Source |
|---|---|---|---|
| Live log | the step's newest attempt runs, is repaired, validated or reviewed | the attempt's newest activity, then the newest 64 log records, one line each (240 characters at most), read every two seconds while the inspector shows it | `Attempts` (1.7), `Log` |
| Your decision | the step is `Review` (passed, not decided) | Accept Candidate, Reject Candidate… (confirmed), as on Verification & Evidence, on the candidate loaded for the newest attempt | `Candidate`, `CandidateDecision` |
| Start it again | the core offers retries (`retry.actions` at 1.13) | a note field (optional), Repair Once More (a held step whose failure is known) and Restart Step, each confirmed; nothing in a cancelled run | `PlanDetail`, `RetryPlanStep` |

Decided by the human on 2026-11-18: a retry is a decision of its own each time, without a cap; a retry that fails is held again; the automatic repair limit is unchanged. Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S18PlanRetryTests` writes `s18-execution-plan-held`, `-refused`, `-live` and `-decision`.

## 7. Follow-ups (protocol 1.14, 2026-11-18)

Sidebar counter: the plan steps that wait for the human (in review, held, conflicting or refused) in the newest run of every plan, an orange pill, from `Documents.steps_waiting`. Integration worktrees under `<data root>/integration/` are removed after each integration and swept when the helper starts.
