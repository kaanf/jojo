# Jojo status and next step

Read this first in a new session. Updated 2026-11-16. Update it whenever a step finishes or a decision changes.

## Where we are

| Step | State |
|---|---|
| Phase 0, Phase 1 (P1-01 … P1-17) | Done |
| Phase 2 (P2-01 … P2-05) | Done. The one live Codex attempt ran; the human accepted its candidate on 2026-11-16. |
| **M0** (close-out of Phase 2) | **Done** |
| **Refactor R-03** (one data-driven gate) | **Done**, with the open points below |
| **Refactor R-01** (typed protocol messages) | **Done** (2026-11-16). Details: [DESIGN_ROADMAP_DIFFERENCES.md](DESIGN_ROADMAP_DIFFERENCES.md), E4 |
| Refactor R-02 (split the long functions) | Optional, not started. Needs its own GO |
| **M1** (Claude Code adapter) | **Done except automatic discovery (D3).** Details: DESIGN_ROADMAP_DIFFERENCES.md, E5 |
| Design system as text | Done (`docs/design/`) |
| **Design wave 1, app shell** | **Done** (2026-11-16): window, sidebar, toolbar, theme tokens for dark and light, status components (pills, tag, counter, buttons). Details: `docs/design/COMPONENTS.md`, section 13 |
| **Design wave 1, screens** | **In progress.** Recovery Center with its launch review, Agent Runs, Verification & Evidence, Source Control, the Approval Inbox (spec and plan), Specifications, Activity & Audit and Settings & Providers, done (2026-11-16), with the shared surfaces (banner, card, list row; timeline, console, inspector section; result row, evidence row, diff, input field; changed-file row, change badge; request row, fact line, value line; document row, document section, trace line; list search field, filter chip, kind tag, record row; section tile, section item, setting line). Details: `docs/design/screens/RECOVERY_CENTER.md`, `docs/design/screens/AGENT_RUNS.md`, `docs/design/screens/VERIFICATION.md`, `docs/design/screens/SOURCE_CONTROL.md`, `docs/design/screens/APPROVAL_INBOX.md`, `docs/design/screens/SPECIFICATIONS.md`, `docs/design/screens/ACTIVITY.md`, `docs/design/screens/SETTINGS.md`, `docs/design/COMPONENTS.md`, sections 14 to 21. One old screen (Start under Execution Plan) still runs unchanged inside the shell |
| M2 … M6 | Not started |

## Next step: design wave 1, the remaining screens

Chosen by the human on 2026-11-16. The app shell, the status components, the shared surfaces, the Recovery Center (with its launch review), Agent Runs, Verification & Evidence, Source Control, the Approval Inbox, Specifications, Activity & Audit and Settings & Providers are built. Next: the last screen whose data exists today, Execution Plan, where the old Start screen still runs. Also open, small: the "Approval History" links of the Approval Inbox and of Specifications, which can open Activity & Audit now (`docs/design/screens/ACTIVITY.md`, section 5.3). Write each screen down in `docs/design/screens/` from its canvas board before building it, and build it on `Surfaces.swift`. The other two candidates stay open:


1. **D3, the rest of M1.** Find the Claude Code and Codex executables automatically, record version and digest per run, warn instead of refusing when the binary changes. Needs a decision first: where discovery runs and how the version is learned without running the CLI (E5, "Not done").
2. **Design wave 1, the screens.** The screens whose data exists today, built from `docs/design/` inside the shell that now exists.
3. **M2.** Feature Chat lead agent. It can now run on either CLI.

The roadmap order is in [SOURCE_FIRST_REUSE_MAP.md](SOURCE_FIRST_REUSE_MAP.md), section 3. Refactor R-02 stays optional and needs its own GO.

To start a helper that can run Claude Code steps, pass (with `--disposable-root`): `--claude-executable <resolved native binary> --claude-version "2.1.311 (Claude Code)" --claude-sha256 <digest> --claude-home <home> --claude-user <account> --claude-auth native-subscription`. `scripts/live_m1_claude.py` shows the whole flow.

## The three documents

| File | What it holds |
|---|---|
| [SOURCE_FIRST_REUSE_MAP.md](SOURCE_FIRST_REUSE_MAP.md) | Jojo inventory, upstream code per capability (repository, pinned SHA, files, license), roadmap M0–M6 and its order |
| [DESIGN_ROADMAP_DIFFERENCES.md](DESIGN_ROADMAP_DIFFERENCES.md) | Every decision where the Claude Design canvas and the roadmap disagreed (D1–D4), engineering decisions (E1–E3), how to reverse each, and the change log |
| This file | Current state and next step |
| [docs/design/](../design/README.md) | The design system as text, transcribed from the canvas on 2026-11-16: tokens, components, states. Read before any SwiftUI design work |

Design canvas: "Jojo macOS", https://claude.ai/artifact/UuyW4DFxPEciFKZm8ZGM24

## How to verify

```sh
python3 scripts/verify.py              # everything, including 17 GUI scenarios (the app opens on screen; leave the Mac alone)
python3 scripts/verify.py --skip gui   # without the GUI stage; result is passed but complete: false
```

Requirements live in `scripts/gates/current.json`; the result goes to `verification/gate-dw1-result.json` and `.log` (earlier results stay in `gate-m0-…`, `gate-r01-…` and `gate-m1-…`). Last full run (2026-11-16, after Settings & Providers of design wave 1): 105 script tests, 420 Rust tests, 85 Swift tests, 17 GUI scenarios, no failures. Rust test names in `required` are written as the test runner prints them, with the module path for unit tests (`protocol::tests::…`).

The twelve P2-04 rehearsal scenarios are run by hand (they passed after R-01; evidence in `~/JojoDisposable/r01-rehearsal/20261116T034931Z`):

```sh
R=~/JojoDisposable/r01-rehearsal/$(date -u +%Y%m%dT%H%M%SZ); mkdir -p $R; chmod 700 $R
cd scripts && python3 gui_p2_04.py --app ../macos/JojoApp/.build/debug/JojoApp --helper ../target/debug/jojo-helper \
  --fixture ../target/debug/jojo-codex-fixture --root $R --out $R/result.json
```

When a step adds tests, raise the minimums and add the new test names to `required` in the gate file. The old `scripts/verify_p*.py` are historical and are not run.

## Decisions in force

- Strategy: Reuse > Port > Adapt > Build. Keep the Rust core and the SwiftUI app. Prefer native CLI subscriptions over API keys.
- Code sources: Vigla (Apache-2.0) and weft (owner's permission reported by the human on 2026-11-15; no written record in the project). Prompts and test cases from spec-kit, factory, commandcenter, fabro.
- Approvals: four kinds, no approve-all (spec, plan, push, PR).
- Repair: automatic, at most two tries, then a human hold. External actions are never retried automatically.
- Executables: discovered automatically; version and digest recorded per run.
- Feature Chat and the project record come before the demo.
- Escaped agent child processes are signalled after a freeze-and-check of PID and start time (E1). Inspection stays on `/bin/ps`; the system-call version is deferred to Phase 4 (E2).

## Open decisions

1. Standing authorization for runs in registered repositories, instead of a GO per step and per live attempt. The human authorized the M1 probes and one M1 live run on 2026-11-16; both are used up. **No further live provider run is authorized.**
2. Whether a write the provider's own sandbox allows (`/tmp`) stays a stop condition.
3. Parallel capacity for the demo (2 proposed; the design shows 3 overall, 2 per repository).
4. A written record of the weft owner's permission.

## Known gaps

- Automatic executable discovery (D3) is not built; see "Next step".
- The Recovery Center shows only what the helper reports. Not reported, and said so on screen: external side effects and quarantined claims as held subjects, task and feature names, the time and age of an inspection, settled subjects, separate facts for worktree, snapshot, evidence, session and claims. There is no pause-admission command and no way to reconnect a lost helper. The list is in `docs/design/screens/RECOVERY_CENTER.md`, section 4.
- Agent Runs shows only what the helper reports. The core reports no lifecycle state per attempt: the snapshot has the scheduler's record text (`scheduler_attempt`, written once as `RESERVED`) and a worktree record, so "Running" is never shown. The result, validation and log of an attempt come from the `Candidate` and `Log` queries, one attempt at a time, shared with Verification & Evidence and read on "Load Result". Not reported, and said so on screen: tasks, features and lifecycle stage, start and stop times, duration, provider and session, PID and process group, liveness, worktree path, branch, base commit, continuation, resource claims, checkpoints, events per attempt. There is no safe-stop command. The list is in `docs/design/screens/AGENT_RUNS.md`, section 4.
- Verification & Evidence shows only what the helper reports: one host validation per candidate (state, detail, receipt, reasons), the write scope with its paths counted by scope, the first 64 log records, the report text and the decision. Not reported, and said so on screen: suites, test counts, commands, durations, the source under test, document and tool versions, deployment receipts, earlier validation runs. There is no command to seal evidence, export a bundle or compare runs. The agent output and accept or reject are on this screen although the canvas puts them in Agent Runs and the Approval Inbox; the changed files and the diff moved to Source Control. The list is in `docs/design/screens/VERIFICATION.md`, section 4.
- Source Control shows only what the helper reports about the one loaded attempt: its changed paths with kind and scope, the core's verdict on the write scope, the base, the candidate digest and the diff. It has no load of its own. Not reported, and said so on screen: repositories, the branch and commit of the candidate, the patterns of the approved scope, scope requests, external actions (push, pull request) and their outcomes, CI results, the worktree path. There is no command to revert a path, to widen the scope, to mark a file reviewed, to push or to open a pull request (push and pull request belong to M5). The diff is unified only. The list is in `docs/design/screens/SOURCE_CONTROL.md`, section 5.
- The Approval Inbox shows only what the helper reports: the review bundle of one loaded document (identity, revision, digests, the events of its upstream approvals and of its latest decision) and the documents the snapshot lists (`artifact` and `review_document` rows). Not reported, and said so on screen: which documents wait for approval (so there is no pending list and no sidebar counter), the name, text and kind of a document (the kind is read from the number of upstream approvals), what a decision affects and risks, the time of a request, supporting evidence, what the latest decision was, the history of decisions. A human approves digests, not a text shown on this screen. There is no push or pull request approval (M5), no "request changes" and no comparison of revisions. Reject and Revoke confirm in a system dialog; the dialog of COMPONENTS.md, section 7, is not built. Accept and Reject of a candidate stay on Verification & Evidence; the Approval Inbox shows that decision read-only (decided by the human on 2026-11-16). The list is in `docs/design/screens/APPROVAL_INBOX.md`, section 5.
- Specifications shows only what the helper reports: the documents the snapshot lists and, for the one loaded document, its revision, its digests, the events of the upstream approvals it binds and the event of its latest decision. The core has no request that returns the text of a document, so the screen shows no text. Not reported, and said so on screen: the name, text, sections and status of a document, its earlier revisions and a comparison between them, validation findings, the history of decisions, requirements and the tasks that implement them. The loaded document is the one the Approval Inbox holds (`WorkspaceModel.review`); choosing a document on either screen changes it on both. No decision is made on this screen; it has a button that opens the Approval Inbox. No sidebar counter. The list is in `docs/design/screens/SPECIFICATIONS.md`, section 5.
- Activity & Audit shows only what the helper reports about an event: its sequence number, epoch, type, aggregate, revision and the first 512 characters of its payload. The workspace reads the newest 64 events with the existing `Events` query when the screen is shown, and 64 older ones on "Load Older"; search and the type filters cover the loaded events only. Not reported, and said so on screen: the time and the actor of an event, a category, a sentence that says what happened, a cause, record hashes. **No hash chain is verified**; the screen says "Not verified". There is no export, no filter by project or date, and the rest of a long payload is not read (`EventDetail` is not in the app's request list). The list is in `docs/design/screens/ACTIVITY.md`, section 5.
- Settings & Providers is read-only and shows only three things: the options the app was started with and passes to the helper (executable path, version text, digest and sign-in kind per provider, the data, worktree and disposable roots; never a provider's home directory or account name), the helper's Hello answer (compatibility, largest frame and page, whether a slice configuration exists, the names of its commands and queries; `HelperClient.hello`), and the connection with the words of the sidebar's status well. The helper reports no provider configuration and no setting back, and the app has no request that writes one, so there is no button, switch or segmented control. Not reported or not built, and said so on screen: whether an executable is compatible, the sign-in state, capabilities, usage and limits, discovery (D3), default providers, parallel capacity (D4; no number is shown), Git and GitHub settings, disk space, backups, retention, scanning. The screen has all ten sections of the board and one more, "Core". The system Settings window (⌘,) still says "Jojo Phase 1 has no settings.". The list is in `docs/design/screens/SETTINGS.md`, section 5.
- The Claude Code stream profile knows only the record kinds seen on 2026-11-16; an unseen kind ends a run as a protocol failure.
- A stop of a running Claude Code process tree was not exercised live.

- The gate once failed compiling `libsqlite3-sys` twice (missing `bindgen.rs`). Root cause unknown; not seen in the four runs after the compile order was changed.
- The twelve P2-04 rehearsal scenarios (`scripts/gui_p2_04.py`) are not in the new gate.
- README and the architecture documents still describe the old rule that escaped descendants are never signalled.
- The Agent Guard cannot reach an escaped descendant whose parent chain is already broken (agent died first, then the helper).
- The test fixture has one escaped child; it does not reproduce Codex's nine-process tree. The next live run is the real evidence.

## Working notes for an assistant

- The project is a local Git repository since 2026-11-16 (branch `main`; the human chose `git init`). Remote `origin` is https://github.com/kaanf/jojo.git, a public repository; the human authorized the first push of `main` on 2026-11-16, and each later push needs the human again. `6188317` is the tree before R-01. `research/upstream` and `__pycache__` are excluded in `.git/info/exclude`, not in `.gitignore`, because the historical gate scripts hash `.gitignore`. Nothing is pushed without the human. Older backup, before M0: `~/Projects/jojo-backups/pre-m0-20261115T165918Z.tar.gz`.
- A provider CLI update needs: new probes, the version constant and parser name in `crates/jojo-domain/src/claude.rs` (or `codex.rs`), the captured streams in the parser tests, and the digest given to the helper. `scripts/tests` and the domain tests may not contain the bare words `File` or `Command` outside the allowed crates, even inside captured data.
- Protocol changes: add the request to `SPECS` in `crates/jojo-domain/src/protocol.rs` and, if the app sends it, to `Request` in `Protocol.swift`; `scripts/tests/test_protocol_fields.py` fails until both agree. `crates/jojo-storage/tests/r01_frames.rs` pins 168 answers; when an answer changes on purpose, change that line of `EXPECTED` and say so in the change log.
- The boundary guard treats the bare word `Command` as a process launch anywhere outside the process crates, in type and variant names too (`Class::Mutation` is named that way for this reason).
- The architecture boundary guard (`scripts/check_p1_01_boundaries.py`) forbids `include_str!`, `unsafe`, FFI, network types and filesystem access outside the allowed crates, in tests too. Run `python3 -m unittest discover -s scripts/tests` after touching Rust or scripts.
- Tests run inside the gate's sandbox cannot execute `/bin/ps`. Use the injected inspector (`inspector()`, `JOJO_TEST_PS`), never the free `verify_identity`. A test that passes with plain `cargo test` can still fail in the gate.
- Fourteen historical reports and the nine `research/upstream` pins are hash-checked by the gate. Add new documents; do not edit those.
- The five reference repositories of the reuse map were only cloned to a temporary directory. Clone them again at the pinned SHAs when their files are needed; adding them under `research/upstream` needs the preservation baseline to be extended.
- Live evidence of P2-05: `/Users/kaan/JojoDisposable/p2-05/20261115T133947Z`.
- Commit messages follow Conventional Commits: `type(scope): summary`, lower case, no full stop; the milestone goes in parentheses at the end when there is one (`feat(claude): … (M1)`). Types in use: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`. Scopes in use: `app`, `protocol`, `claude`, `design`, `status`. The first ten commits were reworded this way on 2026-11-16 and `main` was force-pushed, so every hash from before that day's rewrite is gone; `6188317` is the baseline (formerly `1f6d981`).
- Design work: read `docs/design/` before the canvas. `JOJO_SNAPSHOT_DIR=<directory> swift test --filter DesignShellTests` (in `macos/JojoApp`) writes the shell, every status component and shared surface, and each redesigned screen in its states as PNG files in dark and light, off screen; look at them instead of opening the app. New components go into the `Gallery` view of `DesignShellTests.swift`. A screen takes its data as a plain value (`RecoveryCenterState`, `AgentRunsState`, `VerificationState`, `SourceControlState`, which wraps the `VerificationState` of the loaded attempt, `ApprovalInboxState`, `SpecificationsState`, which wraps the `ApprovalInboxState` of the loaded review, `ActivityState`, `SettingsState`) so that it can be rendered with test rows; system controls and scroll views do not draw off screen (use `CheckBox`, `InputField` and `Scrolling` from `Surfaces.swift`).
- The boundary guard rejects the bare word `Process` anywhere in the Swift sources outside `HelperProcess.swift`, in user-facing strings too ("Process group" failed the gate; "process group" passes).
- The snapshot has no `attempt` rows from the real helper: attempts are the `scheduler_attempt` rows and `worktree_admin` rows carry the same identity (`AgentRunsState.attempts(in:)`). A redesigned screen is added to `MainView` beside `RecoveryCenterScreen`, outside the padded group, and to `MainView.built`, and navigates through a closure that sets the shell's screen. `MainView` keeps every screen alive at opacity 0, so `onAppear` fires for all of them at launch: a screen that reads on its own (Activity & Audit) is told when it is the shown one (`visible`) and reads only then.
- The Swift package keeps one test target. This toolchain runs each test target as its own bundle and the gate reads the last test summary only, so tests in a second target would not be counted.
- The GUI scenarios drive the model through `--ui-script`, not the views; they do not notice a broken layout.
- The human writes in Turkish and is answered in Turkish; project documents are in English.
