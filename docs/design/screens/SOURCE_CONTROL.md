# Source Control

Source: the canvas "Jojo macOS", page "Product", board "Source Control" (`SourceControlMac.dc.html`, 1440 × 1360, light by default). Read on 2026-11-17 from version `1794942048-8ee3`; the board file is byte-identical to the copy read at version `1794922102-b518` (sha256 `cc045932…1c7c`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). The design wave 1 document is in the git history (this file at commit `921322a`).

Built as the seventh screen of step 4 of the Liquid Glass order (STATUS.md), with protocol 1.9 (DESIGN_ROADMAP_DIFFERENCES.md, E15). External actions since M5, with protocol 1.10 (E16).

## 1. The board

| Element | Board | Design system |
|---|---|---|
| Header | "Source Control", "Crew › FT-142 Lobby invite links › T4"; capsule "Review Changes", "View Scope Violations", "Inspect CI" | header row (4.3) |
| Lifecycle card | Execution current, the count pills | lifecycle stepper (2.1) |
| Controls | segmented Repository ("crew-api · 5", "crew-app · 3") and Diff layout ("Unified", "Side by side") | segmented control (5.6) |
| Facts card | Branch, Base, Candidate, From ("T4 · Attempt 2") | card |
| Changed files and Status | rows (colored letter A/M/D, file name, directory in `mono` 11, "+48 −6", "Out of scope" red pill); Status card (Local review "Not reviewed", Scope "1 violation", Push "Not performed", Pull request "None", Location "Local candidate only") | list rows, key-value card |
| Scope banner | orange: "1 change is outside the approved scope", "View Scope Violations", "Revert file" | banner |
| Diff | header (path, "9c1e4b2 → a71f0de"), unified (two number columns, sign, text on `redBg`/`greenBg`) or side by side | diff |
| Inspector (glass, 290 to 330) | Scope validation (Approved scope "2 globs", Inside scope "4 of 5", Unexpected "1"); External actions (push "Outcome unknown" with "Inspect in Recovery"; push and draft pull request "Not performed" with disabled buttons and why); CI (candidate "Not pushed", main build and test) | inspector (4.4) |

## 2. Where each value comes from

| Element | Source | Since |
|---|---|---|
| Candidates chooser | the held feature's attempts that have a candidate (`Attempts` with `validation`); choosing loads that candidate | 1.7 |
| Changed files, scope of each path, diff, base | `Candidate`, `CandidateChanges`, `CandidateDiff` (through `loadCandidate`, shared with Verification & Evidence) | 1.1 |
| Line numbers and counts | read from the diff's hunk headers in the app | — |
| Candidate commit, base, from | `AttemptDetail`: `candidate_head`, worktree `base`, `key`, `ordinal` | **1.9** |
| Status: decision, scope | `Candidate`: the human's decision and the core's scope verdict | 1.1 |
| Approved scope | `AttemptDetail.write_scope`: the patterns of the step's approved validation plan | **1.9** |
| Sidebar counter | the loaded candidate's paths outside the scope (orange) | 1.1 |
| External actions cards, Status "Push" and "Pull request", Location, CI "Plan branch" | `ExternalActions`: the held feature's newest plan run, its newest push and pull request with their state (Not performed, Requested, Pushing, Pushed, Outcome unknown, Rejected, PR #n draft) and why a request cannot be made now | **1.10** |
| "Request Push", "Request Draft PR" | `RequestExternal`; nothing is performed until the Approval Inbox authorizes it | **1.10** |
| "Authorize in Inbox", "Inspect in Recovery" | open the Approval Inbox or the Recovery Center | — |

### Not built, and why

| Board element | Why |
|---|---|
| Branch | An attempt's worktree is detached; "Not reported" |
| Local review ("Not reviewed") | The core records no review mark; the row shows the human's decision instead |
| Revert file | No command; disabled with why |
| CI, "Inspect CI" | Not reported; the button is disabled |
| Several repositories of a feature | A feature has one repository; the chooser lists candidates by attempt |

## 3. Decisions

Decided by the human on 2026-11-17: the chooser lists the feature's candidates by repository and attempt and loads the one chosen; side by side is drawn by the app; revert, a review mark and CI are not available; push and pull request stay disabled, ready for M5. M5 (2026-11-17, E16): the feature's plan branch is pushed, not a candidate; the cards request, the Approval Inbox authorizes.

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/SourceControl.swift`: `SourceControlState` (since M5 also `run`, `push`, `pullRequest`, `requestBlocked`), `DiffLayout`, `DiffRow`, `FileDiff`, `SourceControlView`, `SourceControlScreen` (reads `Features`, `Attempts`, `ExternalActions`, the newest candidate when none is loaded, and `AttemptDetail` while `visible`). The header's layout control is `MainView.diffLayout`; "Review Changes" opens Verification & Evidence.
- At most 400 lines of a file's diff are drawn.
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S10SourceControlTests` writes `s10-source-control-…`, `-side-by-side-…`, `-in-scope-…`, `-none-…`, `-narrow-…`; `--filter S12ExternalActionsTests` writes `s12-source-control-not-performed-…`, `-requested-…`, `-unknown-…`, `-opened-…`.

## 5. What the app shows today / Known gaps

Source Control shows the held feature's candidates, one candidate's base, commit and attempt, its changed files with kind, line counts and scope, the human's decision, the scope banner, the diff unified or side by side, the approved scope's patterns and how many paths fall inside. Since M5 the External actions cards show the held feature's plan branch push and draft pull request as the core reports them (Not performed, Requested, Pushing or Opening, Pushed, Outcome unknown, Rejected, PR #n draft with its link), "Request Push" and "Request Draft PR" with why they cannot be sent now, "Authorize in Inbox" and "Inspect in Recovery"; the Status card's Push, Pull request and Location rows follow them. Known gaps:

- No CI, revert or review mark; CI is "Not reported" even after a push.
- No branch per attempt; a single candidate is never pushed, only a finished plan's branch.
- The pull request link is shown as text; it is not opened from the app.
