# Approval Inbox

Source: the canvas "Jojo macOS", page "Product", board "Approval Inbox" (`ApprovalInboxMac.dc.html`, 1440 × 1160, light by default). Read on 2026-11-17 from version `1794942048-8ee3`; the board file is byte-identical to the copy read at version `1794922102-b518` (sha256 `a6a757d2…929f`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). The design wave 1 document is in the git history (this file at commit `921322a`).

Built as the fourth screen of step 4 of the Liquid Glass order (STATUS.md), with protocol 1.9 (DESIGN_ROADMAP_DIFFERENCES.md, E15). Push and draft pull request requests since M5, with protocol 1.10 (E16).

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
| Push and pull request requests, "External" type | `ExternalActions`: each action's kind, remote, URL, full reference, commit, base, title and body (while requested), digest, state and observations; `Documents.external` and `pending` for the counter | **1.10** |
| "Authorize Push…", "Create Draft PR…", "Decline" | `AuthorizeExternal` with the action's digest, after a confirmation dialog that names the commit, the URL and the full reference; Decline is sent at once | **1.10** |
| git and gh sign-in row | `ExternalActions.auth`: git's credential helper, `gh auth status` | **1.10** |
| "Inspect in Recovery" | an action whose outcome is unknown opens the Recovery Center | **1.10** |

### Not built, and why

| Board element | Why |
|---|---|
| Scope expansion and baseline requests; the "Security" type | The core has no scope expansion or baseline approval |
| Risk and side effects of a document or candidate | No source; "Not reported". A push or pull request states its own (fixed text per kind) |
| "Checked by core 09:42" | The time the app read the documents |
| Filter icon, appearance switch | The segmented control and the type menu filter; the app is light |

## 3. Decisions

Decided by the human on 2026-11-17: candidates are listed with the documents and open Verification & Evidence; push, pull request, scope expansion and baseline requests are "not available yet", ready for M5; "Risk and side effects" is not reported and "Affected" is derived (documents drafted on it). An approval is sent at once; Reject and Revoke confirm (as since design wave 1).

Decided by the human on 2026-11-17 for M5 (E16): the plan branch of a finished run is pushed to the repository's `origin` under the same name and its draft pull request opened against the remote's default branch, each requested on Source Control and authorized here on its own; the pull request's title and body are built by the core and shown in full before the authorization; git and gh use their own sign-in.

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/ApprovalInbox.swift`: `InboxItem` (a document, a candidate or, since M5, an external action), `InboxFilter`, `InboxType`, `ApprovalInboxState`, `ApprovalInboxView`, `ApprovalInboxScreen` (reads `Decisions`, `Features`, `ExternalActions`, the chosen document's `ReviewBundle` and `Document` while `visible` and with each new snapshot). The words of an external action are `ExternalWords` in `ExternalActions.swift`; decoding and commands are `JojoCore/External.swift`. The header's "Approval History" opens Activity & Audit.
- The type menu is a system menu on screen and its title off screen (a menu does not draw there).
- `ApprovalInboxState.documents(in:)` stays for Activity & Audit's "Go to".
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S7ApprovalInboxTests` writes `s7-approval-inbox-…`, `-plan-…`, `-candidate-…`, `-external-…`, `-empty-…`, `-narrow-…`; `--filter S12ExternalActionsTests` writes `s12-inbox-push-…`, `-pull-request-…`, `-unknown-…`, `-push-narrow-…`.

## 5. What the app shows today / Known gaps

The Approval Inbox lists what waits for the human (documents, candidates, and push and draft pull request requests), newest first, filtered by Pending or All and by type; one request with its requested action, why it needs approval, what it affects and whether it is current; the approval policy and the newest decisions. Documents are approved, rejected or revoked here; candidates open Verification & Evidence. A push or pull request request shows its remote and URL, full reference, commit, risk, the git or gh sign-in state and, for a pull request, its title and body exactly as sent; "Authorize Push…" and "Create Draft PR…" confirm in a dialog and are sent once, "Decline" ends the request. An action whose outcome is unknown offers "Inspect in Recovery". Known gaps:

- No scope expansion or baseline approvals.
- Risk and side effects of a document or candidate are not reported.
- The app reads the newest 16 documents and decisions and the newest 32 external actions.
- A request is made on Source Control, not here; there is no way to edit a pull request's title or body before it is opened (a new plan revision changes it).

## 6. Follow-ups (protocol 1.14, 2026-11-18)

For a requirements document, architecture decision or plan, the detail shows "Second opinion" above the decision bar, as on Specifications: the other provider's advisory review of the revision, and "Request Review". The decision bar is unchanged and never waits for it.
