# Jojo screens: Execution Plan

Transcribed on 2026-11-16 from the canvas board "Execution Plan" (`ExecutionPlan.dc.html`, drawn in dark only; feature "FT-142 Lobby invite links" with twelve nodes, "T7" inspected). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 6 are the canvas. Section 7 is what the app builds today and why it differs.

## 1. Frame

| Part | Spec |
|---|---|
| Toolbar | Title "Execution Plan". Breadcrumb "Workspace › Crew › FT-142 Lobby invite links". Four buttons, 28 high: "Review Plan" and "Approve Plan" (secondary; both open the Approval Inbox), "Pause Admission" (secondary; "Stops new attempts from starting. Running attempts continue."), "Start Eligible Tasks · 1" (primary; "Asks the core to start T9. Tasks with unmet dependencies, approvals or claims are never started.") |
| Sidebar counter | None. The item is in the feature group, between Specifications and Agent Runs |
| Lifecycle bar | Below the toolbar, background `#1D1D20`, padding 9 by 18, a hairline below. Leading: the label "Lifecycle" (11.5, `text3`) and the feature lifecycle stepper (COMPONENTS.md, section 2.1) with Discovery, Specification and Planning done, Execution current, Verification and Evidence to come. Trailing: the label "Tasks" and three operational pills, "2 running", "3 blocked", "2 awaiting approval" |
| Columns | Three columns that wrap when narrow: filters (flexible from 210), the plan (from 460, takes the rest), the inspector (flexible from 330). Filters and inspector: background `#1E1E22`, a 1 pt edge (white at 6%) toward the plan |

## 2. Filters

Padding 14 top, 10 trailing, 10 bottom, 14 leading; gap 14. A heading "Filters" (12, semibold, `text2`) with a ghost button "Reset". Four groups, each a label (11.5, `text3`) over its options; an option is a checkbox line 26 high at 12.5 with a trailing count.

| Group | Options |
|---|---|
| Repository | "crew-api" 6, "crew-app" 4 |
| Provider | "Claude Code", "Codex", "Unassigned" |
| Status | "Running" (pill, 2), "Blocked" (pill, 3), "Eligible" 1, "Completed" 3 |
| Wave | A segmented control: "All", "1", "2", "3", "4" |

Below them the heading "Plan checks" and a card (radius 10) of three lines, each a glyph and a sentence: "No cycles", "No missing dependencies", "1 file conflict: T4 and T6".

## 3. Plan

Padding 14 top, 16 at the sides, 18 bottom; gap 12.

**Revision notice.** An accent notice (radius 9, `accentText` at 8% with an edge at 28%): "Plan rev 2 is approved. Rev 3 is proposed and adds T6." over "Running and eligible work follows rev 2 until rev 3 is approved." Two compact buttons: "Compare rev 2 → 3" and "Review Plan".

**Graph.** A well (radius 12, `#17171A`, padding 12 top and 8) that scrolls sideways, 1172 wide. Five column headings at 11, semibold, `text3`: "Wave 1", "Wave 2", "Gates", "Wave 3", "Wave 4" (four columns of 236 and one of 196). Two lanes, one per repository, each a faint panel with the repository's name and color square: "crew-api" (`#2E7D5B`) and "crew-app" (`#7B4FD6`). Edges are drawn between the nodes.

| Node | Kind | State | Second line |
|---|---|---|---|
| T1 Invite table migration | Task, wave 1, crew-api | Passed | Claude Code |
| T3 Invite API contract | Task, wave 1, crew-api | Passed | Claude Code |
| T2 Invite share sheet | Task, wave 1, crew-app | Passed | Codex |
| T4 Invite endpoint + token service | Task, wave 2, crew-api | Running | Claude Code |
| T9 Invite API docs | Task, wave 2, crew-api | Idle, marked "Eligible" | Claude Code |
| T5 Deep link handler | Task, wave 2, crew-app | Running | Codex |
| Approval gate | Gate | "0 of 2 approved" | "ADR-031 rev 5 + Plan rev 3" |
| Deployment gate | Gate | "0 of 4 checks" | "Backend on staging" |
| T6 Token expiry job | Task, wave 3, crew-api | Blocked | "Not in approved plan" |
| T7 Lobby join screen | Task, wave 3, crew-app | Blocked | "4 blockers" |
| Verification gate | Gate | "Not run" | "Candidate fully verified" |
| T8 Maestro flow: invite join | Task, wave 4, crew-app | Waiting | Codex |

A task node is the dependency node of COMPONENTS.md, section 6: 196 × 84, radius 10, the repository's color square, the task number, a status pill, the name, the provider. The chosen node has a 2 pt `accentText` ring. A gate has a dashed edge.

**Legend.** One line at 11.5 in `text3`: "Dependency met" (a green line), "Dependency pending" (a white line), "Gate requirement" (a dashed amber line), "Gate: approval, deployment, verification" (a dashed box), and the key caps "←" "→" "move between nodes · Space inspect".

## 4. Inspector

One node at a time. A header (padding 14 top, 16 at the sides, 12 bottom): a line at 11 in `text3` ("Task · crew-api · Plan rev 2"), the name at 15, semibold ("T1 · Invite table migration"), then the node's pills. Then sections (COMPONENTS.md, section 9), each with a title at 11.5, semibold, `text3`.

| Section | Content on the board |
|---|---|
| Why it's blocked · n | Only on a blocked or waiting task. One line per blocker with a link: "Only in Plan rev 3, which is not approved." · "Review"; "Depends on ADR-031 rev 5, not approved." · "Open"; "File conflict with T4 on InviteTokenRepository.kt. Will run after T4 settles."; "T4 and T5 are still running."; "Deployment gate: backend not on staging (0 of 4 checks)." · "Gate"; "Question Q4 is open: what a revoked link shows. Blocks this task only." · "Answer"; "Resource claim: shared/navigation is held by T5 until it settles."; "Waiting for T7." |
| Description | One sentence |
| Acceptance criteria | Lines "AC-1" and a sentence |
| Assignment | Repository; Provider with a "Change" link |
| File scope | Monospaced paths; one marked "(requested)" |
| Dependencies | "None", or the tasks it depends on, each with its pill |
| Required approvals | "Approved" · "PRD-142 rev 4 · ADR-031 rev 4 · Plan rev 2"; or "Proposed" lines with "Review"; or "Pending" · "Scope expansion: iosApp/Info.plist" · "Review" |
| Execution history | "No attempts yet", or attempts: "Attempt 2" · "Fresh continuation · 12 min" · Running; "Attempt 1" · "Settled after recovery" · Interrupted; "Attempt 1" · "Exited 0 · validated" · Completed |
| Verification requirements | "Verified in V-2", or the commands to run ("./gradlew test integrationTest") |
| Spec revisions | Document tags: "PRD-142 rev 4" · "ADR-031 rev 4" · "ADR-031 rev 5 (proposed)" |

**Actions**, in a bar below the sections (padding 12 by 16, a hairline above). Completed task: "View Evidence". Running task: "Inspect Attempt", "View Logs". Eligible task: the primary "Start Task" ("Requests admission from the core. Starts only if the claim on crew-api/docs is granted.") with the note "Starting asks the core for admission. It can still be refused, for example if admission is paused." Blocked task: "Start Task", "View Blockers", "Inspect Task".

**A gate** has the header (its kind, its name, its count) and three sections: "Required evidence" (lines with a state: "Proposed", "Not run", "Needs approval"), "Unblocks" (the tasks behind it), "How it opens" (one sentence: "Opens only when both exact revisions are approved in the Approval Inbox. An edit to either revision resets it."; "Opens when the core records a deployment receipt tied to the verified commit and all checks pass. Not manually overridable."; "Feeds Evidence. Unknown, skipped and not-run checks keep it closed.").

## 5. What the board does not draw

A plan without tasks, a loading plan, a disconnected core, closed admission, a Start that was sent and has no answer, a refused Start, a repair (D2), the narrow window, and the light appearance.

## 6. Where the board and the decisions in force disagree

| Board | Decision in force |
|---|---|
| A failed task stops and waits for the human; no state between Running and Failed | D2: automatic repair, at most two tries, then a human hold. A "Repairing" operational state and a repair counter are to be added when repair is built (M4) |
| "Pause Admission" | No such request exists; admission closes only through recovery |
| Three tasks run at once across two repositories | D4 is open: no parallel capacity is decided |
| "Approval gate" with scope expansion as a pending approval | D1: four approval kinds (spec, plan, push, PR); scope expansion is "not available yet" |
| A gate is a node between two waves, with a count ("0 of 2 approved") | The core stores a gate as the predicate of an edge (`ArtifactApproved` or `Evidence`) and reports no evaluation of it. The app follows the core: no gate node, the predicate is named in the inspector (found on 2026-11-16) |
| "Plan rev 2 is approved. Rev 3 is proposed" | In the core a newer revision that is not approved makes the plan's approval stale: a Start is refused and `ApprovedPlan` returns nothing. An approved and a proposed revision never stand side by side (found on 2026-11-16) |

## 7. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/ExecutionPlan.swift` on the shared surfaces of COMPONENTS.md, sections 14 to 22. The Start flow of the first version (which replaced the old Start screen, `StartView`) is kept unchanged: the five identity fields, the Start command and its result. Since 2026-11-16 the screen also shows the approved plan.

**What the app knows about a plan.**

1. **The approved plan** (`ApprovedPlan` in `SPECS`, `crates/jojo-domain/src/protocol.rs`; `ReadView::ipc_approved_plan` in `crates/jojo-storage/src/ipc_queries.rs`; `ApprovedPlan` in `Workspace.swift`; the fields are listed in `docs/architecture/STDIO_PROTOCOL.md`). One answer holds the approved revision of one plan: its identity, revision and digest, the PRD and ADR it was stored under, the validation plan found by the digest the plan binds, its steps (identity, revision, provider, repository, priority, repository use, environment, base commit), the edges between steps with the kind of their predicate, and the approvals the plan binds. Several plans can be approved (one per imported bundle); `next` leads to the following one.
2. **Not stored, so not reported:** a name, description, acceptance criteria or file scope of a step; a lifecycle state of a step; whether an edge's predicate is met; a gate as an object of its own; the result of a plan check; a color or name of a repository. Stored but not returned by the query: the attempts of a step.
3. **The plan the core holds today has exactly one step and no edge** (`FeatureBundle` in `slice.rs`: "a one-step execution plan"; `ImportBundle` is the only request that stores a plan). Plans with several steps and edges exist only in the core's tests. The graph is built for them and is exercised with test rows; in the running app it draws one node.
4. **The Start request** takes five identities and nothing else: `prd`, `adr`, `plan`, `validation`, `step` (`StartSubjects`). Its answer is a receipt with `attempt` and `dispatch`; a refusal is one text, the `code` of the error answer (`SliceNotConfigured`, `AdmissionClosed`, `CapacityOne: …`, `RecoveryHold: …`, `StartRefused: …`, `NoProductionAdapter: …`, `AdapterRejected: …`, `DuplicateStart: …`, `StartFailed: …`, and a few plain sentences from the transaction).
5. **The snapshot still has no plan**: its rows are `artifact`, `review_document`, `scheduler_attempt` and `worktree_admin`. The screen does not read the plan from the snapshot.

DESIGN_ROADMAP_DIFFERENCES.md, section 4, gives the whole Execution Plan board to M3 (design wave 3); that stays. This step draws what the stored plan can carry.

**When the plan is read** (decided by the human on 2026-11-16). As Activity & Audit reads its events: `WorkspaceModel.loadPlans()` runs when Execution Plan becomes the shown destination and, while it is shown, when the snapshot's event number changes. It sends one `ApprovedPlan` per plan, at most eight plans, and replaces what was held when the read has ended. `refresh` and `pollEvents` are unchanged and no request is added at launch. A read that fails drops what was held and shows a "Try Again" button.

**The graph** (decided by the human on 2026-11-16) is drawn from the stored plan only:

| Board | App |
|---|---|
| Task node, 196 × 84 | The same box (`TaskNode`) with three lines: the first twelve characters of the step's identity (monospaced), the provider ("Claude Code", "Codex", "No provider bound"), "Step rev n". No status pill, no state outline, no "Eligible" tag, no name |
| Chosen node | 2 pt `accent` ring; choosing a node inspects the step and fills the Start fields |
| Edges | A curve with an arrowhead from the prerequisite to the dependent (`GraphEdge`), in one color. Solid for an `Evidence` predicate, dashed for `ArtifactApproved`. Not green or white: whether a dependency is met is not reported |
| Columns "Wave 1" … "Wave 4", "Gates" | "Wave n" columns of 236. A step's wave is **derived** by the app: one more than the latest wave of its prerequisites. The core stores no wave. No "Gates" column |
| Lanes with the repository's name and color | One lane per repository in the order the plan names them (`LaneBand`), labelled "Repository" and the first twelve characters of its identity, with a neutral square. Steps of one repository in one wave stand below each other. A step bound to no repository has a lane "No repository bound" |
| Gate nodes | Not drawn (section 6). The predicate of an edge is named in the inspector |
| Legend | "Evidence dependency" (solid), "Approval dependency" (dashed), "Whether a dependency is met is not reported.", and a sentence that waves are derived and steps have no name or state. With no edge: "This plan has one step and no dependency." |
| "←" "→" "Space" | Not built |

Edges that do not order the steps (a cycle) cannot be stored by the core; if an answer had one, no graph is drawn and a notice says so.

**The inspector** shows the plan until a step is chosen, then that step:

| Section | Plan | Step |
|---|---|---|
| Header | "Plan · rev n", "Plan" and the short identity, the tag "Approved" | "Step · Repository … · Plan rev n", "Step" and the short identity, the provider as a tag |
| Plan / Assignment | Identity, Revision, Digest, "Approved by" (the decision event), Steps, Edges | Step identity, Revision, Repository, Provider, Repository use, Priority. No "Change" link |
| Documents / Source | PRD, ADR, Validation (or "Not named by the core") | Base commit, Snapshot when there is one, Environment |
| Dependencies | | "None", or "After step …" and "Before step …" with the predicate in words ("Evidence: OutputAvailable · …", "Approval of … rev n"), and "Whether a dependency is met is not reported." |
| Required approvals | The approvals the plan binds: short identity, revision, decision event. "Bound when the plan was stored. The core checks that each is current at a Start." | The same |
| Last section | "Choose a step in the graph or in the list to inspect it and to fill the Start." | "Not reported": description, acceptance criteria, file scope, execution history and verification requirements |

The inspector has no action bar: "View Evidence", "Inspect Attempt", "View Logs", "View Blockers" and "Inspect Task" have no data, and the one action with a request, "Start Task", is the screen's "Start Step".

**The step pick list and the five identities** (decided by the human on 2026-11-16). Above the five fields a card lists the steps of the shown plan (short identity, provider, repository). Choosing a step there or in the graph fills all five fields from the plan (`ApprovedPlan.subjects(step:)`) and marks the row "Chosen". The fields stay `InputField`s and can still be typed; a filled field that is not an identity says so under it. When the core names no validation plan for the plan, that field is left empty and a footnote says to type it. With several approved plans a row of chips above the graph chooses the plan; the first is shown by default.

**No control without a request.** "Pause Admission", "Approve Plan" (a decision is made in the Approval Inbox), "Compare rev 2 → 3", the filters, "Reset", the wave control, "Change" of a provider, "Start Eligible Tasks" and the inspector's buttons are not built, neither as a button nor as a disabled one. "Review Plan" opens the Approval Inbox. No segmented control and no toggle were added.

**When Start can be sent.** Unchanged:

| Case, in this order | Text beside the button |
|---|---|
| The core is not connected | "Unavailable: the core is not connected." |
| The core says it holds no slice configuration (Hello `slice` false) | "The core has no slice configuration; it cannot start a step." |
| The snapshot's admission is reported and is not `Open` | "Admission is closed. The core refuses a Start until it is open." |
| A Start or recovery command is unanswered (`commandPending`) | "Unavailable while a command awaits the core." |
| A field is empty | "Enter all five identities." |
| A field is not 32 lowercase hex characters | "Not an identity: PRD, Step." (the fields that fail) |

Before the first snapshot the admission is unknown and does not block: the helper decides. A step that was already started is not marked: the query does not return a step's attempts, and the core answers such a Start with `DuplicateStart`.

**The result of a Start** is kept by the workspace as a value (`WorkspaceModel.startOutcome`; the status line of the window keeps its words, which the GUI scenarios read). Unchanged:

| Outcome | Shown |
|---|---|
| None in this session | Nothing |
| Sent, no answer yet | The Requested notice (STATES.md, "Requested"): "Start sent to core · waiting for confirmation" |
| Accepted | A card "Last Start": Result "Accepted by the core", Attempt (monospaced identity), Dispatch (the core's word), and a notice: "Accepted is not completed. The attempt's result is on Agent Runs." with the button "Open Agent Runs" |
| Refused | A warning banner "The core refused the Start" with a sentence for the code (for `StartFailed`: "The Start did not launch"; for a code the app does not know: "Nothing was started. The core's reason is below."), and a card with Result "Refused" and Reason (the code as sent, monospaced) |
| Unknown (no answer, channel lost) | A warning banner "Start outcome unknown" · "It will be reconciled by receipt, not sent again." The command stays pending, so Start stays disabled |

### 7.1 Mapping

| Canvas | App today |
|---|---|
| Toolbar, breadcrumb | The shell breadcrumb stays "Jojo › Feature › Execution Plan". The shell toolbar has no screen buttons; "Review Plan" and "Start Step" are in the screen |
| Sidebar counter | None, as on the board. Nothing reported counts eligible or blocked steps |
| Lifecycle bar | Not built. The core reports no lifecycle stage of a feature and no task counts |
| Filters, Plan checks | Not built. No status or count is reported to filter by, and no result of a plan check is reported |
| Revision notice | An accent notice (radius 9, `accent` at 8% with an edge at 28%): "Plan rev n is approved." over "A Start follows this revision. When a newer revision is stored, the approval is stale and the core reports no plan until that revision is approved." No "Compare" button; "Review Plan" is in the heading of "Step to start" |
| Graph, lanes, legend | Built from the stored plan (see above): task nodes without a status, edges, derived waves, repository lanes. No gate node |
| Inspector | Built for the plan and for one step, from stored fields only (see above), without an action bar. Beside the plan from 820 points of content width (300 wide, as Activity & Audit), below the graph otherwise |
| "Start Eligible Tasks · 1", "Start Task" | "Start Step" (primary, one per screen), with the board's note in the words of today: "Starting asks the core to admit the step. It can still be refused, for example when an approval is stale." |
| (not on the canvas) Plan chips | Only with several approved plans: one filter chip per plan (short identity, number of steps) |
| (not on the canvas) Step to start | A heading, the sentence about a Start, the step pick list of the shown plan, and a card of five lines: PRD, ADR, Plan, Validation plan, Step, each an input field with the prompt "identity (32 hex)" |
| (not on the canvas) Reported by the core | Three lines with the words of the sidebar's status well and of Settings & Providers: Core, Admission, Slice configuration |
| (not on the canvas) Not reported | A card that names what the board draws and the core does not report: Step names, Acceptance criteria, File scope, Step state, Gates, Lifecycle, Plan checks, Plan revisions, Execution history; and two decisions: Repair ("Decided, not built: automatic, at most two tries, then a human hold.", D2) and Parallel capacity ("Not reported, and not decided.", D4; no number) |

### 7.2 States

The plan section ("Approved plan") has its own states (`ExecutionPlanState.Phase`); the rest of the screen is as before.

- Reading (the read is running or has not begun, the core is connected or connecting): the Loading form "Reading the approved plan…".
- No approved plan (the read ended with no plan): the Empty form "No approved plan" · "The core holds no plan with a current approval. A plan arrives as an imported bundle and is approved in the Approval Inbox. A newer revision that is not approved makes the approval stale." with the button "Open Approval Inbox". The five fields can still be typed.
- Unreadable (the read failed): warning banner "The approved plan could not be read" with a sentence for the reason ("This core has no request that returns the approved plan." for an older helper, "The plan is too large for one answer of the core. It is not shown.", "The core's answer was not a plan this app can read. Nothing is shown.", otherwise "The core did not answer the read. Nothing is shown."), the button "Try Again", and the reason as sent, monospaced. Nothing that was read before is shown.
- Loading (`notStarted`, `handshaking`): the plan section is Reading; the fields can be filled; Start is disabled with "Unavailable: the core is not connected."; "Reported by the core" says "Connecting to core…".
- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "A Start cannot be sent. What was typed is kept." With a plan read earlier, the plan stays, read-only, and the banner adds "The plan below was read before the core stopped answering."; without one the plan section says "No plan was read" · "The core stopped answering before the approved plan was read." There is no "Reconnect" yet.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Diagnostics only. A Start cannot be sent."
- Recovery required / admission closed: danger banner "Execution admission is closed" · "No new attempt starts until you review the inspection." with the button "Open Recovery Center" (the banner of Agent Runs). The plan is still shown.
- Requested, Unknown outcome, Error (refused): see the outcome table above.
- Stale: there is no Stale form. The plan is read again whenever the snapshot's event number changes while the screen is shown.
- Permission denied has no source on this screen.
- Width: the sections other than the graph are at most 760 wide; the graph well takes the column and scrolls sideways.

### 7.3 Open points

- A step has no name. Until the core stores one (the roadmap's planner, M3), a node shows an identity, and the graph cannot be read the way the board is.
- The query does not return the attempts of a step, although the core stores them (`scheduler_attempts`). With them the inspector could list "Execution history" and the screen could say that a step was already started.
- No state of a step and no evaluation of an edge is reported; with them the pills, the state outlines, "Eligible", the blocker list and the legend's "met" and "pending" could be drawn. The core computes the evaluation at a Start (`evaluate_dependencies`) and has no read of it over the protocol.
- A plan above the frame limit is refused whole; the steps are not paged. Today's plans have one step.
- The snapshot still does not say which `review_document` is a plan. The plan fields no longer need it.
- "Repairing" (D2) is not an operational state yet; it is added with M4.

### 7.4 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Content | Padding 18 top, 22 at the sides, 24 bottom, gap 18; every section except the graph at most 760 wide |
| Graph well | Radius 12, `fill2` (the board's `#17171A` is not a token), padding 12 by 8; columns of 236 with the node 20 from the column's leading edge; a heading row of 22; lanes 8 apart, each with a header of 24, rows of 84 with 12 between them and 12 below |
| Edge | 1.5 pt in `dashedOutline`, dash 4 on 4 off for an approval dependency, an arrowhead of 6 by 7 in `text3` |
| Node outline | `liftRing` (the board's white at 7% is close to `cardRing`, which does not show on the lane) |
| Identity line | The fact line of COMPONENTS.md, section 18 (label 130) with a monospaced input field as its value; an invalid value has a line in `warn` at the meta size under the field |
| Step pick list | List rows of section 14 in a `.panelList` card; the chosen row has the kind tag "Chosen" in `info` |
| Inspector | 300 wide beside the plan from 820 points of content width; identities and digests wrap instead of being cut |
| Not-reported lines | The setting line of section 21 in its missing form (`text3`) |
| New components | `TaskNode`, `GraphEdge`, `LaneBand` (COMPONENTS.md, section 22) |
| Off-screen renders | `execution-plan-graph-…`, `-graph-step-…`, `-graph-narrow-…`, `-one-step-…`, `-reading-…`, `-unreadable-…`, `-disconnected-plan-…`, and the earlier `-empty-…`, `-ready-…`, `-invalid-…`, `-narrow-…`, `-requested-…`, `-accepted-…`, `-refused-…`, `-unknown-…`, `-admission-closed-…`, `-no-slice-…`, `-loading-…`, `-incompatible-…`, `-disconnected-…` (these now show "No approved plan" or the plan section's state above the Start) |

## 8. The plan run (M3, design wave 3)

Since M3 a plan a lead drafted can be started as a whole. The run is read with `PlanRun` for each approved plan, together with the plans, while the screen is shown.

- **Before a run.** A section "Plan run" under the graph: "Approving a plan does not run it. Run Plan asks the core to start every ready step, up to the parallel capacity of the project record, and to validate each step when it settles. Only a plan a lead drafted can run." and the primary "Run Plan", disabled with a reason when the core is not connected, has no slice configuration, admission is closed or a command awaits the core.
- **During and after a run.** The section states the run in one sentence (Running, Paused, Cancelled, Halted, Done), its revision, the plan branch, the branch head and the base, and lists the steps: key and title, provider and task kind, the detail or the dependencies, and the state as a kind tag. "Pause Run", "Resume Run" and "Cancel Run" send `ControlPlan`. A footnote says that a step in review is accepted or rejected on Verification & Evidence, that an accepted step goes onto the local plan branch, that nothing is pushed and that nothing is retried.
- **Graph and inspector.** A node of a running plan shows the step's key, title, provider and state; the legend says that a dependency is met when its step is on the plan branch. The step inspector's title is the key and title, and a "Run" section shows the state, the detail, the task kind, the attempt and the integration commit.
- **What changed in section 7.** Step names and states are reported for a running lead plan (7.3, first and third points); the attempt of a step is shown in the run (second point). An imported plan still has no names, and a plan that never ran has no states. "Parallel capacity" says "Decided (D4): one number for the whole core, set in the project record. Shown on Projects."
- **Not built.** The lifecycle stepper, task counts, filters, plan checks, a revision comparison, the gate nodes, the blocker list per step (the run gives one detail line), accept and reject on this screen, a sidebar counter.

## 9. Repair and review (M4)

A step's state may also be "Repairing" and "Reviewing" (info tone) or "Needs human" (warn tone; the core's `NeedsHuman`). The step inspector's Run section adds "Repair" ("1 of 2"; "Repairs" with the count when the newest attempt is a first one after repairs) and "Review" (the verdict line). "Repair" left the list of what this screen does not show. The run answers the project's repair limit and reviewer, which this screen does not draw; they are on Projects.
