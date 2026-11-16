# Jojo screens: Feature Chat and New Feature

Design wave 2, milestone M2. Canvas boards "Feature Chat" (`FeatureChat.dc.html`) and "New Feature" (`NewFeature.dc.html`), read at version `1794837270-18f1` from their markup text. The light and the "Atelier" Feature Chat boards are explorations and were not used. Code: `macos/JojoApp/Sources/JojoApp/FeatureChat.swift` (state, actions, screen, view) and `macos/JojoApp/Sources/JojoCore/FeatureChat.swift` (the answers of the chat queries and the workspace's reads and commands). The protocol is in `docs/architecture/STDIO_PROTOCOL.md`, "Protocol 1.2: Feature Chat".

## 1. The boards

**Feature Chat.** Toolbar with "Create Spike" and "Export Transcript". A lifecycle stepper (Discovery, Specification, Planning, Execution, Verification, Evidence) and task counts (running, blocked, awaiting approval). A warning banner "Feature continues · T7 is blocked by Q4 · Only the join screen waits for this answer. T4, T5 and T9 are unaffected." with "Answer Q4". A transcript: the human's objective, the lead's message ("I read … Two flows … Reusing decision D-12"), status lines that link documents ("Q1, Q2 resolved · PRD-142 rev 4 approved · Open PRD", "ADR-031 rev 5 proposed · Review"), a "Suggestion" card ("Not a decision until approved in a spec"), and a question card with choices ("Q4 · Blocks T7 only", two choices and "Write my own answer"). A composer ("Message", "Send ⌘↵"). A right column: "Open questions · 2" with Open and Resolved filters, each question with its blocking tag, what it affects, "Resolve Question" and "Defer" or "Create Spike"; "Assumptions" (verified, unverified, invalidated); "Decisions in use"; "Discovery history" (revisions with a line each).

**New Feature.** "Describe the feature · Say what users should be able to do. Jojo investigates the code and asks before anything is planned." Fields: Project, "What should it do?" with "No need to name files, APIs or architecture. That's what discovery is for.", Repositories (optional; "Let Jojo decide" or chips), Reference files, Design references ("Figma link import: not available yet"), Technical constraints. "What happens next: 1. Jojo reads the selected repositories, read-only. 2. It asks clarifying questions in Feature Chat. 3. It drafts a PRD for your approval. Nothing is executed before you approve a plan." Buttons "Cancel esc" and "Start Discovery ⌘↵".

## 2. What the app shows today

New Feature is the screen's state when no feature is held, not a destination of its own: the sidebar has no New Feature item, and "New Feature" in the chat header returns to it.

| Board element | In the app | Source |
|---|---|---|
| Describe the feature, What should it do? | As drawn, `TextArea` | typed |
| Repositories | One chip per registered repository, named by the last component of its path; one must be chosen. No "Let Jojo decide": a chat reads one repository | `ChatIndex.repositories` |
| Provider | Two chips, Claude Code and Codex; not on the board | typed |
| What happens next | As drawn, "repository" for "repositories" | fixed text |
| Start Discovery ⌘↵ | Sends the first `Discover` turn with a new feature identity; disabled with a reason | `ChatTurn` |
| (not on the board) Earlier chats | The features that have turns, newest first, to open one again | `ChatIndex.features` |
| Discovery states | The four of STATES.md, section 3 | `FeatureChat` |
| Step banner | "Feature continues · step docs is blocked by Q3 · Only that step waits for this answer. The rest of the plan is unaffected." One per open step question | `questions[].step` |
| Transcript | Per turn: the human's message (or "Another round with the answers so far."), then "Jojo · Discovery" or "Jojo · Planning" with provider and base commit, the turn's state pill, the lead's summary, the failure detail, the document it drafted and the questions it asked | `turns` |
| Question card in the transcript | Number, tag ("Blocking", "Non-blocking", "Blocks step docs only"), prompt, the newest answer. No choices: the core stores a question as text | `questions` |
| Composer | A note (optional), the provider chips, "Draft Plan" (a `Plan` turn), "Send ⌘↵" (a `Discover` turn), "Stop Turn" while a turn runs; each disabled with its reason | `ChatTurn`, `CancelTurn` |
| Open questions | The questions no human answered yet, each with an answer field, "Resolve Question" and "Defer" (or "Make Blocking" for a deferred one); "Resolved n" counts the answered ones. No Open/Resolved filter | `AnswerQuestion` |
| Status lines that link documents | A "Requirements" panel (revision tag, ready or its finding count, who wrote it, "Open in Approval Inbox") and a "Plan draft" panel (step keys and titles, "ADR", "Validation", "Plan" each opening the Approval Inbox on that document) | `prd`, `plan` |
| (not on the board) Requirements draft | The rendered text of the newest version under the composer | `ChatText` `Prd` |

Values not on the canvas: the inspector column is 320 wide and stands beside the transcript from a window width of 900; below that it follows the transcript. The transcript and New Feature are at most 760 wide.

## 3. Not built, and said so on screen

Lifecycle stepper and task counts (the core has no lifecycle stage and runs no plan before M3), assumptions as records (they are in the draft's text), decisions in use, a discovery history beyond the turns and revisions, suggestions and spikes, export of the transcript, project, reference files, design references and technical constraints, feature names, choice buttons for a question, the sidebar counter. "Not reported by the core" at the end of the screen lists them.

## 4. Where the board and the decisions in force differ

- The board's approval links ("PRD-142 rev 4 approved") read as if approval happened in the chat. Approval stays with the Approval Inbox (D1): every document link opens it with that document loaded; the chat never sends a decision.
- The board's question blocks a task (T7). The core's plan steps have keys and titles but no task numbers; a question blocks a step by its key, and only that step's Start is refused.
- The board shows one Project and several repositories per feature. A chat reads exactly one registered repository (decision 2 of M2; a second repository in one plan comes after the demo).

## 5. States

Disconnected and incompatible: a danger banner, the last chat stays visible, nothing can be sent. Admission closed: a warning banner with "Open Review" (Recovery Center). A core without the chat queries (an older helper): a warning banner, and every send says "This core has no Feature Chat." A turn command in flight: the Requested form; a refusal: a warning banner with a sentence and the core's code; an unknown outcome: the Unknown pill, never sent again. The chat and the index are read only while the screen is shown (`visible`), and again on every new snapshot.
