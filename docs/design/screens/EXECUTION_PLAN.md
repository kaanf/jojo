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

## 7. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/ExecutionPlan.swift` on the shared surfaces of COMPONENTS.md, sections 14 to 21. It replaces the old Start screen (`StartView` in `Views.swift`), which ran unchanged under this destination; everything that screen did is kept: the five identity fields, the Start command and its result.

**What the app knows about a plan.** Almost nothing, and the screen is built on that:

1. **The Start request** (`Start` in `SPECS`, `crates/jojo-domain/src/protocol.rs`; `StartSubjects` in `Workspace.swift`) takes five identities and nothing else: `prd`, `adr`, `plan`, `validation`, `step`. `WorkspaceModel.start(_:)` sends it.
2. **The Start answer** is a receipt with two more fields: `attempt` (the identity of the new attempt, which is the identity of the command) and `dispatch` ("launched", or "not repeated" when the same command is answered from its receipt).
3. **A refusal** is one text, the `code` of the error answer (`crates/jojo-helper/src/service.rs`, `crates/jojo-storage/src/slice.rs`): `SliceNotConfigured`, `AdmissionClosed`, `CapacityOne: …`, `RecoveryHold: …`, `StartRefused: …` (a missing or stale approval of the PRD, ADR, plan or validation plan; a step that is not in the approved plan; an unregistered or changed repository), `NoProductionAdapter: …`, `AdapterRejected: …`, `DuplicateStart: …`, `StartFailed: …` (accepted and committed, but nothing was launched), and a few plain sentences from the transaction ("repository differs from the approved plan", "validation plan is not the one the plan binds").
4. **The snapshot has no plan.** Its `aggregates` section has rows of four kinds only: `artifact` (a PRD), `review_document` (an ADR, a plan or a validation plan; the value is a digest and the row does not say which of the three it is), `scheduler_attempt` (the text `RESERVED`) and `worktree_admin`. There is no row for a plan, a step, a task or a dependency, and the identity of a step is in no row.
5. **No request returns a plan**: not its steps, not its edges, not its text. `ReviewBundle` returns the revision and digests of a document. The plan the core holds today has exactly one step and no edge (`FeatureBundle` in `slice.rs`: "a one-step execution plan").

DESIGN_ROADMAP_DIFFERENCES.md, section 4, gives the Execution Plan board to M3 (design wave 3); that stays. Design wave 1 builds only what has data today.

**So the screen shows the Start flow and says what is not reported** (decided by the human on 2026-11-16; the alternatives were lists of the snapshot's documents and attempts, which Specifications and Agent Runs already show, and a new query that returns the plan, which changes Rust and the protocol and is out of this step). The graph, the dependency nodes, the gates, the pipeline cells (COMPONENTS.md, section 5.4) and the lifecycle stepper (section 2.1) are **not built**: none of them can be drawn from reported data, and no example graph stands in their place.

**The five identities are typed** (decided by the human on 2026-11-16). A pick list was the alternative: the PRD could be picked from the snapshot's `artifact` rows, but ADR, plan and validation plan would share one list of `review_document` rows whose kind is not reported, and the step is in no row. The fields are `InputField`s; a filled field that is not an identity says so under it.

**No control without a request.** "Pause Admission", "Approve Plan" (a decision is made in the Approval Inbox), "Compare rev 2 → 3", the filters, "Reset", the wave control, "Change" of a provider, "View Evidence", "Inspect Attempt", "View Logs", "View Blockers" and "Inspect Task" are not built, neither as a button nor as a disabled one. "Review Plan" is built: it opens the Approval Inbox. No segmented control and no toggle were added.

**When Start can be sent.** The old screen disabled the button only while a command was unanswered; the model refused anything that was not five identities, and the helper refused the rest. The rule is the same, with each case said in words beside the button, and three cases the helper would refuse (or could never answer) stopped before sending:

| Case, in this order | Text beside the button |
|---|---|
| The core is not connected | "Unavailable: the core is not connected." |
| The core says it holds no slice configuration (Hello `slice` false) | "The core has no slice configuration; it cannot start a step." |
| The snapshot's admission is reported and is not `Open` | "Admission is closed. The core refuses a Start until it is open." |
| A Start or recovery command is unanswered (`commandPending`) | "Unavailable while a command awaits the core." |
| A field is empty | "Enter all five identities." |
| A field is not 32 lowercase hex characters | "Not an identity: PRD, Step." (the fields that fail) |

Before the first snapshot the admission is unknown and does not block: the helper decides.

**The result of a Start** is kept by the workspace as a value (`WorkspaceModel.startOutcome`, new in this step; the status line of the window keeps its words, which the GUI scenarios read). It is what the helper answered, nothing is inferred:

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
| Filters, Plan checks | Not built. There are no tasks to filter and no check of a plan is reported |
| Revision notice | Not built. Which revision of a plan is approved is not reported; the Approval Inbox shows the loaded document's revision |
| Graph, lanes, legend | Not built (see above) |
| Inspector | Not built as an inspector. Its one action that has a request, "Start Task", is the screen's "Start Step" |
| "Start Eligible Tasks · 1", "Start Task" | "Start Step" (primary, one per screen), with the board's note in the words of today: "Starting asks the core to admit the step. It can still be refused, for example when an approval is stale." |
| (not on the canvas) Step to start | A heading, the old screen's sentence ("Start runs one approved plan step in a Jojo-owned worktree. The core refuses it unless admission is open, every approval is current and no earlier work is on hold."), and a card of five lines: PRD, ADR, Plan, Validation plan, Step, each an input field with the prompt "identity (32 hex)" |
| (not on the canvas) Reported by the core | Three lines with the words of the sidebar's status well and of Settings & Providers: Core, Admission, Slice configuration |
| (not on the canvas) Not reported | A card that names what the board draws and the core does not report: Tasks and waves, Dependencies, Gates, Lifecycle, Plan checks, Plan revisions, Step state; and two decisions: Repair ("Decided, not built: automatic, at most two tries, then a human hold.", D2) and Parallel capacity ("Not reported, and not decided.", D4; no number) |

### 7.2 States

- Loading (`notStarted`, `handshaking`): the fields can be filled; Start is disabled with "Unavailable: the core is not connected."; "Reported by the core" says "Connecting to core…".
- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "A Start cannot be sent. What was typed is kept." There is no "Reconnect" yet.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Diagnostics only. A Start cannot be sent."
- Recovery required / admission closed: danger banner "Execution admission is closed" · "No new attempt starts until you review the inspection." with the button "Open Recovery Center" (the banner of Agent Runs).
- Requested, Unknown outcome, Error (refused): see the outcome table above.
- Empty: the screen has no list, so no Empty form. With all fields empty Start says "Enter all five identities."
- Stale and Permission denied have no source on this screen.
- Width: the content is at most 760 wide.

### 7.3 Open points

- A query that returns the approved plan (its steps, their provider and repository, the edges, the revision) is what the graph, the inspector and a pick list of steps need. Until then a step's identity is known only to whoever imported the bundle.
- The snapshot does not say which `review_document` is a plan; with that, the four document fields could be picked instead of typed.
- "Repairing" (D2) is not an operational state yet; it is added with M4.

### 7.4 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Content | At most 760 wide, padding 18 top, 22 at the sides, 24 bottom, gap 18 (as Settings & Providers) |
| Identity line | The fact line of COMPONENTS.md, section 18 (label 130) with a monospaced input field as its value; an invalid value has a line in `warn` at the meta size under the field |
| Not-reported lines | The setting line of section 21 in its missing form (`text3`) |
| New components | None. The screen is made of existing surfaces, so nothing was added to the component gallery |
| Off-screen renders | `execution-plan-empty-…`, `-ready-…`, `-invalid-…`, `-narrow-…`, `-requested-…`, `-accepted-…`, `-refused-…`, `-unknown-…`, `-admission-closed-…`, `-no-slice-…`, `-loading-…`, `-incompatible-…`, `-disconnected-…` |
