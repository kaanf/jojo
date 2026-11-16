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
| Reverse | `git revert` the R-01 commits (baseline: `6188317`). The wire addition can be dropped alone by removing the `structured` branch in the `Recovery` arm of `service.rs`; the client then shows rows without structured fields, as it does for an older helper. |
| Not done | `handle` and `slice_command` are shorter but still long; splitting them is R-02. The twelve P2-04 rehearsal scenarios are still outside the gate. |

### E5. M1: Claude Code adapter

| | |
|---|---|
| Built | `crates/jojo-domain/src/claude.rs` (the one argument vector, the launch envelope), the `ClaudePrint` profile in `stream.rs`, `crates/jojo-storage/src/claude.rs` (`ClaudeAdapter`), helper options `--claude-executable`, `--claude-version`, `--claude-sha256`, `--claude-home`, `--claude-user`, `--claude-auth`, and Start choosing the adapter by the step's provider. The app forwards the six options to the helper. |
| Argument vector | `-p --output-format stream-json --verbose --input-format text --restricted --strict-mcp-config --tools Read,Edit,Write,Glob,Grep --permission-mode acceptEdits --permission-prompts none --no-session-persistence --disable-slash-commands --no-chrome`. The prompt goes to standard input; the working directory is the owned worktree. |
| Tool envelope (the human's choice, 2026-11-16) | The CLI's own restricted mode with five file tools and no tool that runs a command. Tests are run by Jojo's host validation, not by the agent. To allow commands later: add the tool to `print_argv` and `CLAUDE_TOOLS`, decide how its commands are sandboxed, and repeat the probes. |
| Environment | Exactly `HOME` and `USER`, with values the helper's launcher configured. Observed: with `HOME` alone the CLI reports that it is not signed in. The worktree may lie under the home; the home may not lie inside the worktree, repository or output directory. |
| What was observed, on `claude` 2.1.311 | Five short runs in a temporary repository (`~/JojoDisposable/m1-probes/20261116T041446Z`): record shapes; edits inside the working directory applied; a write to `../outside.txt`, to `/tmp` and to `.git/config` refused by the CLI; no shell tool present. Without `--tools`, restricted mode still lists twenty tools (among them subagents, web search and worktree creation), which is why the vector names the five. Three of the captured streams are parser tests in `crates/jojo-domain/tests/m1_claude.rs`. |
| Stream profile | Fail-closed like the Codex one. The first record of a run must list exactly the five tools, permission mode `acceptEdits`, no MCP server and `apiKeySource` `none`; otherwise the run cannot succeed. A tool use outside the five, a subagent record, an unknown record or subtype, a record for another session and anything after the result are protocol failures. The CLI reports a failed sign-in as result subtype `success` with `is_error: true`; that is an error here. |
| Live run (authorized by the human for this one run) | `scripts/live_m1_claude.py` on the generated sample repository through a real helper: outcome `Succeeded`, one changed path (`slugify.py`, in scope), host validation `PASS`, helper exit 0, nothing left running. Evidence: `~/JojoDisposable/m1-live/20261116T043600Z`. The script sent the four approvals as protocol frames; the candidate was neither accepted nor rejected. |
| Not done: D3 | Automatic discovery of the executables, and a warning instead of a refusal when the binary changes, are not built. The executable path, version text and digest are still launcher options for both providers, and any other version is refused. This needs a decision on where discovery runs (the app may launch only the helper; the helper has no filesystem or process API) and how the version is learned without running the CLI. |
| Known limits | The profile models only the record kinds seen in these runs. A long run that produces another kind (for example a context compaction record) ends as a protocol failure, not a success. The CLI keeps its own state under the home directory (`~/.claude`, `~/.claude.json`), outside the worktree; whether that counts as a stop condition is open decision 2 in STATUS.md. Claude Code's process tree under a stop was not exercised live. |
| Reverse | `git revert` the M1 commits. With no `--claude-*` options the helper behaves as before: a Claude Code step answers `NoProductionAdapter: ClaudeCode`. |

## 6. Change log

| Date | Change |
|---|---|
| 2026-11-15 | First version. D1–D3 decided; D4 open. Feature Chat and the project record moved before the demo. |
| 2026-11-15 | Added E1 (signal escaped descendants after an identity check, using `ps`) and E2 (system-call inspection deferred to Phase 4). |
| 2026-11-15 | M0 code: E1 implemented in `crates/jojo-storage/src/process.rs` (`signal_escaped`, `escaped_from`, `Supervised::signal_verified`, `run_agent_guard`); two tests changed from "not signalled" to "stopped" (`t09_process.rs`, `p2_04_1_guard.rs`). The real Codex 0.169.0 stream is a parser test in `crates/jojo-domain/tests/p2_01_codex.rs` (inlined, because the boundary guard forbids `include_str!`). The intermittent `OwnershipHeld` test failure was a real lock-release race, fixed by an explicit unlock in `OwnershipLock::drop` (`ownership.rs`). Result on the final tree: 395 Rust tests passed, 5 ignored; 87 boundary tests passed; fmt and clippy clean. Swift tests and the `verify_p2_*.py` gate scripts were not run; the input hashes recorded by the P2-04.1 gate no longer match the changed files. Pre-change backup: `~/Projects/jojo-backups/pre-m0-20261115T165918Z.tar.gz`. |
| 2026-11-16 | The human accepted the P2-05 candidate in the app (reported by the human; not checked in the database here). Refactor R-03 added (E3). Gate result without the GUI stage, three consecutive runs: 97 script tests, 396 Rust tests (5 ignored, one of them run explicitly outside the sandbox), 35 Swift tests, no failures. The GUI stage (17 scenarios) has not been run through the new gate yet. |
| 2026-11-16 | Full gate run, nothing skipped: `passed: true, complete: true`. 97 script tests, 396 Rust tests, 35 Swift tests, 17 GUI scenarios (9 + 3 + 5), no failures. Recorded in `verification/gate-m0-result.json`. |
| 2026-11-16 | `git init` (the human chose it); baseline commit `6188317`. Refactor R-01 done (E4): typed protocol messages in `jojo-domain`, helper dispatch on `RequestKind`, structured assessment fields in `Recovery` rows for minor ≥ 1, typed requests in the Swift client. Full gate, nothing skipped: `passed: true, complete: true`; 104 script tests, 403 Rust tests, 39 Swift tests, 17 GUI scenarios. Recorded in `verification/gate-r01-result.json`. The twelve P2-04 rehearsal scenarios were run by hand afterwards and passed (`~/JojoDisposable/r01-rehearsal/20261116T034931Z`). |
| 2026-11-16 | Remote `origin` added (https://github.com/kaanf/jojo.git, public) and `main` pushed on the human's instruction. Design system written down in `docs/design/`; system fonts chosen. M1 done except D3 (E5): Claude Code adapter, fail-closed stream profile for 2.1.311 built from observed runs, helper wiring, one authorized live run (`Succeeded`, validation `PASS`). Gate result in `verification/gate-m1-result.json`. |
| 2026-11-16 | Design wave 1, app shell (Swift app only; no Rust or protocol change). New `Theme.swift` (tokens for dark and light, typography, card elevation), `StatusComponents.swift` (operational pill, evidence pill, document tag, counter, key cap, four button kinds), `Shell.swift` (sidebar with the twelve canvas destinations, toolbar, content area). The primary window has a full-size content view; the tab view and the connection header are gone, their content is in the sidebar, the status well and the toolbar. The five old screens are unchanged and sit under Agent Runs, Approval Inbox, Recovery Center, Execution Plan and Verification & Evidence; the other seven destinations say they are not built. Values the canvas does not give are listed in `docs/design/COMPONENTS.md`, section 13. Ten new Swift tests (`DesignShellTests`), placed in the existing test target because the gate reads one test bundle. Gate renamed `gate-dw1`, Swift minimum raised to 49. Full gate, nothing skipped: `passed: true, complete: true`; 105 script tests, 420 Rust tests, 49 Swift tests, 17 GUI scenarios, no failures (`verification/gate-dw1-result.json`). The shell was reviewed from off-screen renderings in both appearances, not in the running window. |
| 2026-11-16 | Commit messages changed to Conventional Commits (the human's decision). The ten existing commits were reworded with `git filter-branch`; contents, authors and dates are unchanged (the tree was compared with the old branch: no difference) and `main` was force-pushed. All hashes changed; the baseline before R-01 is now `6188317` (was `1f6d981`), and this file and STATUS.md name the new hash. The gate was not rerun because no file content changed. |
| 2026-11-16 | Design wave 1, Recovery Center and launch review (Swift app only; no Rust or protocol change). The two canvas boards are written down in `docs/design/screens/RECOVERY_CENTER.md`. New `Surfaces.swift` (banner, card, list row, group header, section heading, footnote, the Empty and Loading forms, checkbox) and `RecoveryCenter.swift`; the old `RecoveryView` is removed. The screen shows only what the helper reports and says what is not reported instead of showing an example. Differences from the canvas, all listed in section 4 of that document: only prior attempts are held subjects; no inspection age, settled list, toolbar buttons, pause command or reconnect; "Live · unsupervised" cannot be told apart from unknown; the action sentences follow the core's wording; the three actions with an ellipsis ask once in a system confirmation dialog, which the old screen did not do; the launch review is reached with "Open Review" from the Recovery required banner and its checkmarks are kept per inspection digest. Under 920 points of content width the admission and inspection cards sit below the detail. Five Swift tests added (54); the gate minimum is raised. Gate result in `verification/gate-dw1-result.json`. |
| 2026-11-16 | Design wave 1, Agent Runs (Swift app only; no Rust or protocol change). The canvas board is written down in `docs/design/screens/AGENT_RUNS.md`. New `AgentRuns.swift`; the old `RunsView`, which listed every snapshot row, is removed. New shared surfaces in `Surfaces.swift` (wrapping row, labeled field, timeline, console, inspector section with its facts grid, notice well; COMPONENTS.md, section 15). The screen shows only what the helper reports and says what is not reported instead of showing an example. Differences from the canvas, all listed in section 4 of that document: the real helper has no `attempt` rows, so attempts are the snapshot's `scheduler_attempt` rows (the old screen's "Cancel Task" button and the sidebar counter looked for `attempt` and never matched; the counter now counts the same rows as the screen, a one-line change in `Shell.swift`); no lifecycle state is reported per attempt, so the pill is "Not loaded", "Not sealed", the sealed outcome, or the recovery hold, and never "Running"; the result, validation and log are read with "Load Result" because the workspace holds one loaded attempt, shared with Verification & Evidence; the event timeline is replaced by "Recorded facts" without times; no lifecycle strip, task groups, filter, segmented control, toolbar buttons, console search, provider session, supervision facts, workspace, claims or checkpoints; "Request Safe Stop" is drawn disabled because the core has no such command; "Cancel Attempt…" asks once in a system confirmation dialog and is unavailable for a sealed or held attempt. Under 960 points of content width the inspector sits below the detail. Five Swift tests added (59); the gate minimum is raised. Gate result in `verification/gate-dw1-result.json`. |
| 2026-11-16 | Design wave 1, Verification & Evidence (Swift app only; no Rust or protocol change). The canvas board is written down in `docs/design/screens/VERIFICATION.md`. New `Verification.swift`; the old `CandidateView` is removed and everything it did is kept: loading an attempt by its identity, Run Validation, Accept Candidate, Reject Candidate, the write scope, the changed files, the diff, the agent output and the validation report. New shared surfaces in `Surfaces.swift` (result row, evidence row, diff with line numbers read from the hunk headers, input field, requested notice; COMPONENTS.md, section 16). The screen shows only what the helper reports and says what is not reported instead of showing an example. Differences from the canvas, all listed in section 4 of that document: the core records one host validation per candidate, so "Suites" is two result rows (host validation, write scope) without counts, commands or row buttons, and the verdict header has no count pills; the title is "Host validation passed" for `PASS` and "Not verified" for everything else; the evidence manifest lists the four records the app holds; "Seal evidence" is replaced by a Decision card with Accept and Reject, which the canvas puts in the Approval Inbox; Accept is enabled only by `CandidateReview.canAccept` and the rule is not restated on screen; "Reject Candidate…" asks once in a system confirmation dialog, which the old screen did not do; the diff, the changed files and the agent output stay on this screen although the canvas puts them in Source Control and Agent Runs; the diff and the report draw their first 300 lines until "Show All"; no toolbar buttons, lifecycle strip, export, history or run comparison. The loaded attempt is still the one Agent Runs loads ("Load Result", "View Evidence"), and the identity field follows it. Under 800 points of content width the Decision card sits below the main column. Five Swift tests added (64); the gate minimum is raised. Gate result in `verification/gate-dw1-result.json`. |
| 2026-11-16 | Design wave 1, Source Control (Swift app only; no Rust or protocol change). The canvas board is written down in `docs/design/screens/SOURCE_CONTROL.md`. New `SourceControl.swift`; the destination showed "not built yet" before. New shared surfaces in `Surfaces.swift` (changed-file row, change badge; COMPONENTS.md, section 17). The screen has no load of its own: it shows the one attempt result the workspace holds, the one Agent Runs and Verification & Evidence load, as a `SourceControlState` that wraps the `VerificationState` of that result, and reads the scope of a path with `VerificationState.scope(_:)`. One addition to the workspace model: `candidateBase`, the `base` field of the `CandidateChanges` answer, which the helper already sent and the app did not keep. Nothing was removed from Verification & Evidence; it still shows the changed files, the diff and Accept and Reject. Whether those move is open. Differences from the canvas, all listed in section 5 of that document: no repository control, lifecycle strip or toolbar buttons; Branch says "Not reported"; the change badge is read from the core's word (`Added`, `Modified`, `Deleted`, `TypeChanged`); the line counts of a row are counted from that path's section of the loaded diff; choosing a row shows that section, which the board does not draw; the status strip shows the recorded decision, the write scope and the host validation instead of local review, push and pull request; the scope banner has "View Scope Violations" and no "Revert file"; the diff is unified only, with one number column, and draws its first 300 lines until "Show All"; "Scope validation" counts the paths by the core's scope and says that the patterns are not reported; "External actions" is one sentence, because push and pull request belong to M5 and the core has no such commands; no CI. A Review card links to Verification & Evidence. The sidebar counter of Source Control is the number of changed paths of the loaded attempt outside the write scope (`OUT_OF_SCOPE`, `BASELINE`), in `danger`: the board's "1" matches its one violation. Three columns from 1020 points of content width, two from 710, one below. Four Swift tests added (68); the gate minimum is raised. Gate result in `verification/gate-dw1-result.json`. |
| 2026-11-16 | Verification & Evidence no longer shows the changed files and the diff (Swift app only). Decided by the human: the canvas has them in Source Control only, and Source Control now shows them for the same loaded attempt. The write scope row with its counts stays, with a new "View Changes" button that opens Source Control. Accept and Reject stay on Verification & Evidence until the Approval Inbox is built; the agent output stays too. This reverses the sentence "nothing was removed" of the entry above. No test count change (68). |
| 2026-11-16 | Design wave 1, Approval Inbox (Swift app only; no Rust or protocol change). The canvas board is written down in `docs/design/screens/APPROVAL_INBOX.md`. New `ApprovalInbox.swift`; it replaces the old `ReviewView` (removed from `Views.swift`, with the `subject` state of `MainView` that only it used; the identity field now lives in the screen, which stays mounted) and keeps what it did: load one document by identity, Approve, Reject, Revoke, no decision on a stale review. The model calls the GUI scenarios drive (`loadReview`, `decide`) are untouched. New shared surfaces in `Surfaces.swift` (request tile and row, fact line, value line; COMPONENTS.md, section 18). The screen has no load or query of its own: it takes an `ApprovalInboxState` built from `WorkspaceModel.review`, `reviewStale`, the snapshot rows, the loaded attempt result and `HelperClient.outstandingCommands`. Differences from the canvas, all listed in section 5 of that document: the helper reports no pending approvals, so the leading panel lists the documents of the snapshot (`artifact` and `review_document` rows) under "Documents", says that their approval state is not reported, and the sidebar item has no counter; the kind of a document is read from the number of upstream approvals its bundle binds (none: specification, one: architecture decision, two: plan); the facts are the bundle's digests and events instead of "Affected" and "Risk and side effects"; no supporting evidence, filter, history or toolbar buttons; Approve is sent at once, Reject and Revoke confirm once in a system dialog because the dialog of COMPONENTS.md, section 7, does not exist; a sent decision shows as Requested while the `HumanDecision` command is outstanding and as Stale with "Reload" afterwards; push and pull request approvals are named as not available (M5). Decided by the human: Accept and Reject of a candidate stay on Verification & Evidence, and the Approval Inbox shows the loaded candidate's decision read-only with a button that opens that screen; nothing was removed there. Three columns from 1030 points of content width, two from 730, one below. Four Swift tests added (72); the gate minimum is raised. Gate result in `verification/gate-dw1-result.json`. |
