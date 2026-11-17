# Feature Chat

Source: the canvas "Jojo macOS", page "Product", board "Feature Chat" (`FeatureChatMac.dc.html`, 1440 × 1260, light by default). Read on 2026-11-17 from version `1794931431-3abf`; the board file is byte-identical to the one in version `1794922102-b518` (sha256 `04dd3752…a2bd`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). The document written for design wave 2 from the first boards is in the git history (this file at commit `c3a9d64`); New Feature is [NEW_FEATURE.md](NEW_FEATURE.md).

Built as the second screen of step 4 of the Liquid Glass order (STATUS.md), with protocol 1.8 (DESIGN_ROADMAP_DIFFERENCES.md, E14).

## 1. The board

The content column (padding 10 6 12 10, gap 14):

| Element | Board | Design system |
|---|---|---|
| Header | a 36 × 36 "J" tile (app gradient), "FT-142 Lobby invite links" (17 bold), "Jojo · Discovery rev 3 · Crew"; a capsule group "Create Spike", "Export Transcript" and the appearance switch | header row (section 4.3) |
| Lifecycle card | the stepper Discovery (current) … Evidence, then the pills "2 running" (blue), "3 blocked" (red), "2 awaiting approval" (orange) | lifecycle stepper (section 2.1) on a card |
| Conversation card | radius 18, flexible (at least 480): an inline banner "Feature continues · T7 is blocked by Q4" with "Only the join screen waits for this answer. T4, T5 and T9 are unaffected." and "Answer Q4" (primary); then the transcript; then the composer | card, inline banner (section 10) |
| Transcript | a date separator "Discovery rev 1 · Nov 12" (11 semibold `text3`); the human's message (right, `accent`); the lead's replies (a 26 round "J" avatar, a bubble) with inline `code`; receipts ("Q1, Q2 resolved · PRD-142 rev 4 approved · Open PRD", "Q3 resolved by you · ADR-031 rev 5 proposed · Review"); a dashed "Suggestion" card ("Not a decision until approved in a spec", "Add to ADR-031 rev 5 proposal", "Dismiss"); an open question "Q4 · Blocks T7 only" with three radio choices, the last "Write my own answer" | — |
| Composer | a capsule (min height 44): attach, "Message Jojo about FT-142", "⌘↵", a round primary send button | capsule |
| Inspector | a glass panel (290 to 330, radius 18, padding 16, sections 18 apart): "Open questions · 2" with a segmented "Open / Resolved 3", question cards (number, "Blocking · T7" or "Non-blocking", the question, "Affects T7 · AC-6", "Resolve Question", "Create Spike" or "Defer"), resolved rows ("Q1 Resolved in PRD-142 rev 4 · Done"); "Assumptions" (A1 "Unverified" with "Spike S-1 planned · Open spike", A2 "Verified", A3 "Invalidated" struck); "Decisions in use" (D-12, D-31 "Approved" with their sources); "Discovery history" ("rev 3 · Q3 answered · ADR-031 rev 5 · today") | inspector (section 4.4), key-value cards |

## 2. Where each value comes from

| Element | Source |
|---|---|
| Title, project | `Features` (1.7): `number` ("FT-n"), `name`, `project_name`; else the chat index's name |
| "Discovery rev n" | The number of turns of the newest turn's kind |
| Lifecycle and counts | `Features` (1.7): the five stages (Evidence not reached) and `running`, `blocked`, `awaiting` |
| Blocking banner | `FeatureChat`: an open blocking question about the whole feature, else one about a step |
| Date separators, history dates | `FeatureChat` at 1.8: each turn's `started_at_unix_ms` |
| Messages and replies | `FeatureChat`: `message`, `summary`, `detail` (inline code with backticks drawn in `mono`) |
| Receipts | `FeatureChat`: the questions a turn asked and which a human answered; the requirements document a turn drafted and its approval (`prd`); the plan draft (`plan`) |
| Open questions | `FeatureChat` questions without a human's answer; answered with `AnswerQuestion` (Answer, Defer, Block) |
| Assumptions | `FeatureChat` at 1.8: `assumptions` (id, statement, `Current` since a revision or `Dropped` in a revision) |
| Decisions in use | `Documents` (1.6): the feature's documents whose approval is `Approved` |
| Composer | `ChatTurn` `Discover` with the note and the text of each attached file under "## Reference file:" |
| Draft Plan | `ChatTurn` `Plan`, offered once the requirements are approved and no plan was drafted |
| Provider | The default of the project that holds the feature's repository, else the newest turn's |
| Export Transcript | The app writes the conversation as Markdown to the file the human chooses in the save panel |

### Not built, and why

| Board element | Why |
|---|---|
| "J" tile in the header | The shell's header has no leading tile; the title carries the code |
| Create Spike (header, question card), "Open spike" | The core has no spikes: the header button is disabled with "Not available yet: the core has no spikes."; the cards have none |
| Suggestion card | The core records no suggestions |
| Radio choices of a question | The lead's questions have no choices; a written answer is the only form |
| "Affects T7 · AC-6" | A question names its step at most |
| Verified, Unverified, Invalidated of an assumption | The core records no verification of an assumption; the screen shows "In PRD" (since a revision) or "Dropped" (in a revision) |
| D-numbers of decisions | Decisions are the feature's approved documents, named by kind and title |
| Appearance switch | The appearance follows the system |

## 3. Decisions

Decided by the human on 2026-11-17 (the four proposals as given): (1) protocol 1.8: the feature chat dates its turns and lists the requirements document's assumptions, derived, no schema change; (2) a written answer only, no choices; (3) Draft Plan as a receipt once the requirements are approved, every turn on the project's default provider, Attach adds a text file to the message as in New Feature; (4) Export Transcript writes Markdown through the save panel; Create Spike and suggestions are not built.

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/FeatureChat.swift`: `FeatureChatState` (a plain value: the chat, the features row, the documents, the projects, the note and its attachments), `FeatureChatView` and the components of COMPONENTS.md, section 27. `FeatureChatScreen` reads `ChatIndex`, `Projects` and `Features` while shown (`WorkspaceModel.chatShown`), the chat (`FeatureChat` at minor 8) as before, and writes the transcript with `fileExporter` (`TranscriptDocument`).
- "Answer Qn" in the banner and "Resolve Question" in the inspector move the focus to that question's answer field in the transcript, which is ringed in `accent`.
- An approved requirements document's text opens under its receipt ("Open PRD"); "Open in Approval Inbox" there loads its review.
- Keyboard: ⌘↵ sends the note only while the screen is shown.
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S5FeatureChatTests` writes `s5-feature-chat-…`, `-step-blocked-…`, `-approved-…`, `-running-…`, `-refused-…`, `-older-core-…`, `-narrow-…`, `-disconnected-…` in dark and light. The older renders of `DesignShellTests` (`feature-chat-…`) stay as a check of a chat without a features row.

## 5. What the app shows today / Known gaps

Feature Chat shows what the core reports about the held feature: its code, name, project, stages and counts (1.7), its turns with their dates (1.8), what each turn asked and drafted, the open and resolved questions, the requirements document's assumptions (1.8), its approved documents, and its discovery history. The human answers, defers or blocks a question, sends a note with text files attached, drafts the plan once the requirements are approved, stops a running turn and exports the transcript. Known gaps:

- No spikes, suggestions, question choices, a question's affected criteria, or a verified state of an assumption.
- The lead's reply is its summary; the requirements text opens under the receipt of the turn that drafted the current revision.
- A chat reads one registered repository; every turn runs on the project's default provider.
- Lead turns are fresh runs; the questions and answers so far travel in the prompt (no session continuation).
