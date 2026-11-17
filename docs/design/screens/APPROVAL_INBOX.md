# Approval Inbox

Source: the canvas "Jojo macOS", page "Product", board "Approval Inbox" (`ApprovalInboxMac.dc.html`, 1440 × 1160, light by default). Read on 2026-11-17 from version `1794942048-8ee3`; the board file is byte-identical to the copy read at version `1794922102-b518` (sha256 `a6a757d2…929f`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). The design wave 1 document is in the git history (this file at commit `921322a`).

Built as the fourth screen of step 4 of the Liquid Glass order (STATUS.md), with protocol 1.9 (DESIGN_ROADMAP_DIFFERENCES.md, E15).

## 1. The board

| Element | Board | Design system |
|---|---|---|
| Leading section (300 to 340, gap 12) | "Approval Inbox" (20 bold), "6 pending · no approve-all"; a capsule with Filter and Approval history icons; a segmented "Pending 6 / All" and a type menu (All types, Specs, Plans, External, Security); request items (a colored dot, kind and age in `meta`, the title 13 semibold, "Project · FT-n"); a card "Approval policy" ("Push and PR: Separate approvals", "Approvals expire: On content change"); a card "Recently decided" (ADR-031 rev 4 Approved Nov 13; rev 3 Invalidated; Plan rev 2 Approved) | segmented control (5.6), list rows, pills |
| Detail (card, radius 18, padding 24 28 28, gap 20) | "kind · where", a 26 bold title, a `mono` hash; appearance switch; status pill (Awaiting approval, Requested, Stale); a stale banner; "Requested action" (16 medium); a definition card (Why approval, Affected with a repository pill and `mono` paths, Risk and side effects, Staleness with "Current · checked by core 09:42"); "Supporting evidence" rows with chevrons | card, key-value card |
| Decision bar (glass, radius 18) | primary ("Approve Revision…", "Approve Plan…", "Authorize Push…", "Allow for This Attempt…"), secondary ("Request Changes", "Review Plan", "Decline", "Reject"), tertiary link ("Compare Revisions"), and the footnote "Approving sends a request. The core checks validity again before recording it." | buttons (section 3) |

The board's six requests: an ADR revision, a plan revision, a push, a baseline change, a scope expansion and a stale PRD revision.

## 2. Where each value comes from

| Element | Source | Since |
|---|---|---|
| Pending count, sidebar counter | `Documents.pending` (documents `Pending` and waiting candidates) | 1.6 |
| Request items | `Documents`: documents (kind, revision, title, feature, approval, `stale`, creation time) and candidates whose host validation passed and nobody decided | 1.6 |
| Where | the request's feature in `Features` (project, "FT-n") | 1.7 |
| Detail of a document | `Document`: revision digest, documents drafted on it (Affected), the previous revision (Diff, Compare), findings | **1.9** |
| Repository pill | the feature's repository (`Features`) | 1.7 |
| Approve, Reject, Revoke | `HumanDecision` for the loaded `ReviewBundle`; Reject and Revoke confirm in a system dialog | 1.1 |
| Request Changes | opens Feature Chat with a note to the lead (as on Specifications) | 1.2 |
| Compare Revisions, "Diff rev a → b" | opens Specifications in Compare | 1.9 |
| Candidate | `Documents.candidates`; "Open in Verification" loads it there, where it is accepted or rejected | 1.6 |
| Recently decided | `Decisions`: the newest decisions with `applies`; an approval that no longer applies is "Invalidated" | **1.9** |
| Approval policy | the decisions in force (four kinds, no approve-all, approvals bind exact content) | — |

### Not built, and why

| Board element | Why |
|---|---|
| Push, pull request, scope expansion and baseline requests; "External" and "Security" types | Push and pull request belong to M5; the core has no scope expansion or baseline approval. The "External" type says so; the place is kept for M5 |
| Risk and side effects | No source; "Not reported" |
| "Checked by core 09:42" | The time the app read the documents |
| Filter icon, appearance switch | The segmented control and the type menu filter; the app is light |

## 3. Decisions

Decided by the human on 2026-11-17: candidates are listed with the documents and open Verification & Evidence; push, pull request, scope expansion and baseline requests are "not available yet", ready for M5; "Risk and side effects" is not reported and "Affected" is derived (documents drafted on it). An approval is sent at once; Reject and Revoke confirm (as since design wave 1).

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/ApprovalInbox.swift`: `InboxItem`, `InboxFilter`, `InboxType`, `ApprovalInboxState`, `ApprovalInboxView`, `ApprovalInboxScreen` (reads `Decisions`, `Features`, the chosen document's `ReviewBundle` and `Document` while `visible` and with each new snapshot). The header's "Approval History" opens Activity & Audit.
- The type menu is a system menu on screen and its title off screen (a menu does not draw there).
- `ApprovalInboxState.documents(in:)` stays for Activity & Audit's "Go to".
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S7ApprovalInboxTests` writes `s7-approval-inbox-…`, `-plan-…`, `-candidate-…`, `-external-…`, `-empty-…`, `-narrow-…`.

## 5. What the app shows today / Known gaps

The Approval Inbox lists what waits for the human (documents and candidates), newest first, filtered by Pending or All and by type; one request with its requested action, why it needs approval, what it affects and whether it is current; the approval policy and the newest decisions. Documents are approved, rejected or revoked here; candidates open Verification & Evidence. Known gaps:

- No push, pull request, scope expansion or baseline approvals (M5 and later).
- Risk and side effects are not reported.
- The app reads the newest 16 documents and decisions.
