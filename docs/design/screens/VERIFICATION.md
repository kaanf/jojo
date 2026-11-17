# Verification & Evidence

Source: the canvas "Jojo macOS", page "Product", board "Verification & Evidence" (`VerificationMac.dc.html`, 1440 × 1240, light by default). Read on 2026-11-17 from version `1794942048-8ee3`; the board file is byte-identical to the copy read at version `1794922102-b518` (sha256 `7fe73c8a…6188`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). The design wave 1 document is in the git history (this file at commit `921322a`).

Built as the eighth and last screen of step 4 of the Liquid Glass order (STATUS.md), with protocol 1.9 (DESIGN_ROADMAP_DIFFERENCES.md, E15).

## 1. The board

| Element | Board | Design system |
|---|---|---|
| Header | "Verification & Evidence", "Crew › FT-142 Lobby invite links"; capsule "Inspect Failure", "Compare Validation Runs", "Export Evidence Bundle" | header row (4.3) |
| Verdict card (radius 18) | a 44 tile, "Not verified" pill, "V-3 incomplete", "Unknown, skipped and not-run checks never count as passed…"; count tiles Passed, Failed, Unknown, Not run, Skipped, Needs approval, Missing | card, pills |
| Candidate | Candidate, Source under test (two repositories @ commits), Snapshot with "Clean", Validator | key-value card |
| Suites · V-3 | "Review Missing Checks", "Run Eligible Verification"; a table (Suite, Result, Count, Command and detail, a row button) | table |
| Evidence manifest | approvals, attempt logs, test reports, tool versions, deployment receipt, each with a pill; "Inspect Evidence" | key-value card |
| Aside | Seal evidence (requirements met or not, "Seal Evidence" disabled), Export ("Export Evidence Bundle"), Verification history (V-3 current Incomplete, V-2 Invalidated, V-1 Failed) with "Compare Runs" | inspector |

## 2. Where each value comes from

| Element | Source | Since |
|---|---|---|
| Candidate chooser | the held feature's attempts that have a candidate (`Attempts`) | 1.7 |
| Verdict, Run Validation, Accept, Reject | `Candidate` (`validation`, `decision`, `canAccept`); `Validate`, `CandidateDecision` | 1.1 |
| "V-n", counts, Verification history | `AttemptDetail.validations`: each host validation receipt, oldest first | **1.9** |
| Source under test, snapshot | `AttemptDetail`: repository, candidate head, worktree base and snapshot | **1.9** |
| Validator | the receipt's validator (`AttemptDetail.validations`) | **1.9** |
| Suite row | the step's test inventory and command (`AttemptDetail.tests`, `command`) with the detail and reasons of the newest validation | **1.9**, 1.1 |
| Write scope row | the core's scope verdict and the paths inside it (`CandidateChanges`); "View Changes" opens Source Control | 1.1 |
| Evidence manifest | the approval bundle a run was bound to, the log records, the runs, the executable version (`AttemptDetail.executable`) | **1.9**, 1.1 |
| Export Evidence Bundle | the app writes these records as Markdown, marked unsealed, through the save panel | — |

### Not built, and why

| Board element | Why |
|---|---|
| Not run, Skipped, Needs approval, Missing counts | The core records one host validation per run; Passed, Failed and Unknown are counted |
| Several suites per run, durations, per-suite counts | Not reported; a note says so |
| Seal Evidence | The core seals no evidence (it would need a schema); disabled with the requirements shown |
| Deployment receipt | No deployment in the core; "Missing · Not available yet" |
| Compare Validation Runs, Compare Runs | The runs are listed in the history; a comparison is not built |
| Baseline approvals, setup fixes ("Fix setup") | No such checks in the core |

## 3. Decisions

Decided by the human on 2026-11-17: each validation receipt is a "V-n"; a suite row is the step's command and test inventory; only Passed, Failed and Unknown are counted; the history and runs come from the receipts. Seal is "not available yet"; Export writes the records as unsealed Markdown through the save panel. Accept and Reject of a candidate stay here (2026-11-16).

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/Verification.swift`: `VerificationState` (`bundle` is the export), `VerificationView`, `VerificationScreen` (reads `Features`, `Attempts`, the newest candidate when none is loaded, and `AttemptDetail` while `visible`; `fileExporter` with `TranscriptDocument`). The header's "Inspect Failure" opens Agent Runs on the attempt's log.
- The agent output and the report text that the wave 1 screen drew are on Agent Runs (Logs) and in the export.
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S11VerificationTests` writes `s11-verification-…`, `-failed-…`, `-accepted-…`, `-none-…`, `-narrow-…`, `-disconnected-…`.

## 5. What the app shows today / Known gaps

Verification & Evidence shows the loaded candidate's verdict, its host validation runs as "V-n" with their counts and history, the source under test, the validator, the suite and write scope rows, the evidence the core records, the seal's requirements, and the human's decision. The human runs validation, accepts or rejects the candidate and exports the records. Known gaps:

- No seal, deployment receipt, run comparison, per-suite counts or durations.
- The report text is in the export, not on screen.
