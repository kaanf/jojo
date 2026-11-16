# Jojo screens: Verification & Evidence

Transcribed on 2026-11-16 from the canvas board "Verification & Evidence" (drawn in dark only; one candidate, validation run V-3 with eight suites). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 3 are the canvas. Section 4 is what the app builds today and why it differs.

## 1. Frame

| Part | Spec |
|---|---|
| Toolbar | Title "Verification & Evidence". Breadcrumb "Workspace › Crew › FT-142 Lobby invite links". Secondary buttons at the trailing edge: "Inspect Failure" (with a glyph), "Compare Validation Runs" and "Export Evidence Bundle" (with a glyph) |
| Sidebar counter | None on the Verification & Evidence item |
| Lifecycle strip | The strip of Agent Runs ([AGENT_RUNS.md](AGENT_RUNS.md), section 1): the compact stepper and the task pills "2 running", "3 blocked", "2 awaiting approval" |
| Columns | Two columns that fill the rest and wrap when narrow: main (flexible from 460) and a trailing panel (from 320) |

## 2. Main column

Padding 16 top, 18 sides, 22 bottom; gap 16.

### 2.1 Verdict header

Leading, gap 4: a line with the verdict at 20 bold, tracking -0.02, white ("Not verified") and an evidence pill beside it, gap 10 (Incomplete form, "V-3 incomplete"); under it one sentence in `text2`: "Unknown, skipped and not-run checks never count as passed. Results apply to CAND-142-07 only."

Trailing, gap 6: one evidence pill per result kind that occurs, with the number of suites as its label: Passed "2", Failed "1", Unknown "1", two neutral pills "2" and "1", Needs approval "1". The glyphs of the two neutral pills were not transcribed.

### 2.2 Summary strip

Background `surface`, radius 10, inner ring (white at 5%), padding 14 by 16; fields in a wrapping row (gap 14 by 28), each a label at 11.5 `text3` over a value (gap 3).

| Field | Value |
|---|---|
| Candidate | Identifier at 12.5 medium ("CAND-142-07") |
| Source under test | One identifier line at 11.5 per repository: "crew-api ft-142-t4 @ a71f0de", "crew-app main @ 41d07aa" |
| Snapshot | Identifier at 11.5 ("sha256 9f2c…41ab") and a Passed-form pill "Clean", gap 6 |
| Validator | Plain text: "Isolated worktree · no access to agent sessions" |

### 2.3 Suites

Section heading "Suites · V-3" with two trailing compact buttons, gap 6: secondary "Review Missing Checks" and primary "Run Eligible Verification" (with a glyph; tooltip "Runs Maestro and OSV-Scanner where runnable. Does not rerun passed suites.").

A panel card (radius 10, inner ring) with one result row per suite (COMPONENTS.md, section 10). Row: wrapping, centered, gap 10, padding 10 by 14, a 1 pt edge below (white at 6%) except after the last.

| Column | Spec |
|---|---|
| Name | Flexible (weight 2, from 170). The suite at 13 semibold white over the repository at 11.5 `text3` ("crew-api", "crew-app", "both") |
| Result | 130 wide. An evidence pill |
| Count | From 80, may shrink. Identifier at 11.5: "214 / 214", "37 / 38", "3 changed", "n/a" |
| Command | Flexible (weight 3, from 240), gap 2. The command as an identifier at 11.5 over a note at 11.5 `text3` |
| Action | One compact button |

| Suite | Pill | Count | Command | Note | Button |
|---|---|---|---|---|---|
| Unit · crew-api | Passed | 214 / 214 | `./gradlew test` | "Gradle wrapper · 1m 12s · log V3-01" | ghost "Log" |
| Integration · crew-api | Failed | 37 / 38 | `./gradlew integrationTest` | "revokes on lobby close: expected 410, got 200" | secondary "Inspect Failure" |
| Contract check · crew-api | Unknown | n/a | `./gradlew contractTest` | "Runner killed by signal 9. No result recorded." | ghost "Log" |
| Unit · crew-app | Passed | 129 / 129 | `./gradlew allTests` | "Gradle wrapper · 2m 40s · log V3-04" | ghost "Log" |
| Maestro UI flows · crew-app | Not run | n/a | `maestro test flows/` | "No iOS simulator available on this Mac" | "Fix setup" |
| Lint · crew-app | Skipped | n/a | `./gradlew ktlintCheck` | "Disabled in project config. Counts as not passed." | |
| Snapshot baselines · crew-app | Needs approval | 3 changed | `./gradlew verifyPaparazzi` | "Baseline changes need your approval" | secondary "Compare" (opens the Approval Inbox) |
| OSV-Scanner · both | "Missing" (neutral form: `fill`, `text2`) | n/a | `osv-scanner -r .` | "Required by security policy, not yet run" | ghost "Run" |

"Missing" is not one of the nine evidence states of COMPONENTS.md, section 2.3.

### 2.4 Evidence manifest

Section heading "Evidence manifest · CAND-142-07" with a trailing compact secondary "Inspect Evidence". A panel card with one evidence row per record (COMPONENTS.md, section 10). Row: wrapping, centered, gap 10, padding 9 by 14, the same edge below: the name at 13 (flexible, weight 2, from 180), a note as an identifier at 11 `text3`, an evidence pill.

| Record | Note | Pill |
|---|---|---|
| "PRD-142 rev 4 · ADR-031 rev 4 · Plan rev 2" | "sha256 3e9a…c21" | Passed form, "Hash verified" |
| "Attempt logs · T4 #2, T2 #2, T1 #1" | "4 records" | Passed form, "Hash verified" |
| "Test reports · V-3" | "8 suites" | Incomplete |
| "Tool and runner versions" | "recorded per run" | Passed form, "Recorded" |
| "Deployment receipt · staging" | "none" | "Missing" (neutral form) |

## 3. Trailing panel

Background `#1E1E22`, a 1 pt edge (white at 6%) on its leading side. Padding 14, gap 18. Three blocks, each a section heading over a panel card.

**Seal evidence.** Card padding 12 by 14, gap 10. A sentence in `text2`: "Sealing makes the manifest immutable for CAND-142-07. Only possible when every requirement is met." Then one line per requirement (gap 8): the requirement at 13 and a trailing evidence pill that says what is in the way.

| Requirement | Pill |
|---|---|
| "All required suites passed" | Failed form, "1 failed" |
| "No unknown results" | Unknown form, "1" |
| "No not-run or skipped checks" | Neutral form, "3" |
| "Baseline changes approved" | Needs approval form, "3" |
| "Deployment receipt present" | "Missing" |

Then a primary button "Seal Evidence" (with a glyph), disabled (opacity 0.42), tooltip "Unavailable: 5 requirements unmet."

**Export.** Card padding 12 by 14, gap 8. "Exports the existing V-3 records as they are, marked unsealed and incomplete. Exporting does not seal." and a secondary button "Export Evidence Bundle" (with a glyph).

**Verification history.** Heading with a trailing compact secondary "Compare Runs". One row per validation run, newest first: padding 9 by 14, gap 10, edge below except after the last. The run as an identifier at 12 ("V-3 · current") over its candidate and time at 11.5 `text3` ("CAND-142-07 · 09:41"), then an evidence pill; a reason at 11.5 `text3` takes a line of its own under the row.

| Run | Second line | Pill | Reason |
|---|---|---|---|
| "V-3 · current" | "CAND-142-07 · 09:41" | Incomplete | |
| "V-2" | "CAND-142-06 · yesterday" | Invalidated (struck through) | "Code changed after the run" |
| "V-1" | "CAND-142-04 · Nov 13" | Failed | |

The board has no diff, no changed-file list, no agent output and no accept or reject: on the canvas those belong to Source Control and the Approval Inbox.

## 4. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/Verification.swift` on the shared surfaces of COMPONENTS.md, sections 14 to 16. It replaces the old `CandidateView` and keeps what that screen did, except that the changed files and the diff moved to Source Control ([SOURCE_CONTROL.md](SOURCE_CONTROL.md)) when that screen was built: loading an attempt by its identity, Run Validation, Accept Candidate, Reject Candidate, the write scope, the agent output and the validation report. The screen takes a plain value (`VerificationState`) and shows only what the helper reports:

- the result of one attempt (`Candidate` query: sealed, outcome, reason, the provider's own closing words, candidate digest, number of log records, the host validation with its state, detail, receipt, validated candidate and reasons, and the decision);
- the number of changed paths of the sealed candidate counted by the scope the core gives each, and the core's verdict on the write scope (`CandidateChanges`);
- the first 64 log records (`Log`) and the text of the validation report (`ValidationReport`);
- the helper connection, the admission value and whether a command is pending.

The workspace holds one loaded result at a time. Agent Runs shows the same one: its "Load Result" loads it and its "View Evidence" opens this screen. A part of the canvas without a source says so on screen; nothing is filled with an example.

### 4.1 Mapping

| Canvas | App today |
|---|---|
| Toolbar buttons, breadcrumb to project and feature | Not built. The shell breadcrumb stays "Jojo › Verification & Evidence" |
| Lifecycle strip and task pills | Not built: features, lifecycle stages and tasks are not reported |
| (not on the canvas) Load bar | The first line of the main column: an input field "Attempt identity (32 hex)" (at most 340 wide) and a compact secondary "Load". The field follows the loaded attempt, so an attempt loaded in Agent Runs is named here. Return in the field loads |
| Verdict header | Title "Host validation passed" when the core's validation state is `PASS`, otherwise "Not verified". Pill: Passed, Failed, Unknown, Not run; Not run form "No sealed result" when nothing is sealed. Sentence: "Unknown and not-run checks never count as passed. Results apply to candidate 9f2c41ab0d17 only." (the first twelve characters of the digest), or "… This attempt has no sealed candidate." No count pills: the core records one validation, not suites |
| Summary strip | Attempt (full identity) · Candidate (full digest, "None") · Result (evidence pill as in Agent Runs: Passed form "Succeeded", Failed form "Failed", Incomplete form "Interrupted", Not run form "Not sealed" or the core's text) · Reason (the core's text, left out when empty) · Provider said (its closing words, "Nothing") · Decision (4.3). A footnote: "What the provider said is its own statement, not a validation result. Not reported: the repository, branch and commit under test, whether the snapshot is clean, and how the validator is isolated." |
| Suites | "Host validation", a card with two result rows (4.2) and, when the core gave reasons, one line per reason under them. Trailing: a compact secondary "View Changes", which opens Source Control, and a compact primary "Run Validation", which sends `Validate` for the sealed candidate; before them the reason when it is unavailable (4.4). A footnote: "The core records one host validation per candidate. Suites, test counts, commands, durations and a log per suite are not reported." No "Review Missing Checks", no row buttons |
| Evidence manifest | "Evidence": a card with four evidence rows, each a record the app holds: Sealed candidate (digest shortened in the middle; Sealed, or Not run form "Not sealed") · Attempt log ("n records"; Passed form "Recorded", or Not run form "None") · Validation receipt ("receipt n"; "Recorded" or "None") · Validation report ("n lines"; "Recorded", or Not run form "Not loaded"). A footnote: "Document revisions, hashes checked on read, tool and runner versions and deployment receipts are not reported." No "Inspect Evidence" |
| Changed files, diff | Not on this screen, as on the canvas: Source Control shows them for the same loaded attempt |
| (not on the canvas) Agent output | "Agent output": the console of Agent Runs with the loaded log records, the count in the heading ("3 of 3 records"), and the same last line in `warn` when more than 64 exist |
| (not on the canvas) Validation report | "Validation report": the report text on the console, plain. The first 300 lines, with the same "Show All n Lines". Without text: "No validation recorded." or, with a receipt, "No report text loaded." |
| Seal evidence | "Decision": a card with the sentence "Accepting records your decision only. Nothing is merged, pushed or published.", the facts Validation, Write scope and Decision, then "Accept Candidate" (primary) and "Reject Candidate…" (destructive), each with its reason when unavailable (4.4). No list of requirements: the rule is the core's and is not restated |
| Requested | While a command is pending: a well with a "Requested" pill and "Sent to core · waiting for confirmation" above the buttons (STATES.md) |
| Export, Verification history, Compare Runs | Not built. A footnote under the card: "Sealing evidence, exporting a bundle and comparing validation runs are not available: the core has no such commands and reports only the latest validation of a candidate." |

### 4.2 Result rows and scope

| Row | Under the name | Pill | First line (identifier style) | Note |
|---|---|---|---|---|
| Host validation | "Receipt n", or "No receipt" | The validation state: Passed, Failed, Unknown, Not run | The core's detail text, when it gave one | Not run and sealed: "Jojo has not validated this candidate. Tests the agent says it ran do not count." · Not sealed: "No sealed result yet." · Unknown: "The core could not determine the result. Never shown as passed or failed." · otherwise none |
| Write scope | "n changed paths" | The core's scope verdict: Passed, Failed, Unknown, Not run | The core's reason when the scope is unknown ("no sealed candidate", an error code) | The paths counted by scope, zero counts left out: "3 in scope · 1 out of scope · 1 protected baseline · 1 unknown" |

No count column is drawn: the core reports no test counts.

| Scope of a path (the core's word) | Pill |
|---|---|
| `ALLOWED` | Passed form, "In scope" |
| `OUT_OF_SCOPE` | Failed form, "Out of scope" |
| `BASELINE` | Failed form, "Protected baseline" |
| anything else | Unknown form, "Unknown" |

### 4.3 Decision

The Decision fact and the summary strip show "None recorded" for `NONE` and the core's text otherwise ("ACCEPTED", "REJECTED").

"Accept Candidate" is enabled only when `CandidateReview.canAccept` is true (and the core is connected and no command is pending). The screen does not restate that rule; the core checks it again.

"Reject Candidate…" asks once in a system confirmation dialog (the dialog of COMPONENTS.md, section 7, is not built): "Reject this candidate?" · "Records your decision to reject this candidate. A recorded decision is not changed from this app." The old screen did not ask.

### 4.4 Unavailable actions

The first reason that applies is shown beside or under the button, which is disabled.

| Reason | Run Validation | Accept | Reject |
|---|---|---|---|
| "Unavailable: the core is not connected." | yes | yes | yes |
| "No sealed candidate." | yes | yes | yes |
| "Already decided: ACCEPTED." (the core's text) | yes | yes | yes |
| "Unavailable: the core accepts only a passing host validation of exactly this candidate." (`canAccept` is false) | | yes | |
| "Unavailable while a command awaits the core." | yes | yes | yes |

"Load" is disabled only while the core is not connected; the banner says why.

### 4.5 Navigation and states

- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "Showing the last known state, read-only. No actions can be requested." Every action is disabled. There is no "Reconnect" yet.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Read-only. No actions can be requested."
- Recovery required (admission reported and not `Open`): danger banner "Execution admission is closed" · "No new attempt starts until you review the inspection." with "Open Recovery Center". Validation and the decision stay available; the core decides.
- Empty (nothing loaded): the Empty form with "No attempt loaded" · "Load a finished attempt by its identity, or choose one in Agent Runs and use Load Result." and a secondary "Open Agent Runs". The load bar stays above it.
- Loading: the workspace does not say that a read is pending. The agent output says "Reading…" while its part has not arrived; the last loaded attempt stays until the new one answers.
- Requested: the well of 4.1 and the toolbar's "Requested" pill; no action can be sent again while a command is pending.
- Unknown outcome: a validation the core could not classify has the Unknown pill, the title "Not verified" and the sentence of 4.2; it cannot be accepted.
- Error and Stale: a failed read or a refused command (`StaleCandidate` among them) is reported in the status line under the screen ("Candidate unavailable: …", "Accept refused (…); the candidate is unchanged"). A failed `Candidate` read unloads the attempt.
- Permission denied has no source on this screen.
- Content under 800 wide: the Decision card moves below the main column's sections. Two columns appear from 800, which the window has at its opening size.

### 4.6 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Trailing panel | Background `sidebar`, edge `sidebarEdge`, 320 wide |
| Result row, evidence row, input field, requested notice | COMPONENTS.md, section 16 |
| Verdict title | 20 bold, tracking -0.02 em, `textStrong` (canvas: white) |
| Lines drawn before "Show All" | 300, for the validation report |
| Glyphs (SF Symbols) | Empty state `checkmark.shield` |
