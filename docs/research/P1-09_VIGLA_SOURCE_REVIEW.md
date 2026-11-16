# P1-09 — Vigla source review and independent reimplementation

Reviewed the actual local Git source of [Kilbex/Vigla](https://github.com/Kilbex/Vigla), pinned to **`bbd19ae2d5a77401502756c550b5dcd4ae59bbc9`** (commit date 2026-07-27T07:03:10-07:00). Every source below was byte-compared against `git show bbd19ae2d5a77401502756c550b5dcd4ae59bbc9:path`. No fetch, build, execution of upstream code, or provider-private directory access was needed. The exact SHA-256 inventory is in [REPOSITORY_PINS.json](../../research/REPOSITORY_PINS.json), `Vigla.p1_09_review.sources`. This is a pinned source review, not a claim about the latest Vigla or installed CLI release.

## Actual source paths and functions

- [`crates/adapters/claude/src/lib.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/adapters/claude/src/lib.rs): `ClaudeAdapter::handle_system`, `ClaudeAdapter::handle_assistant`, `ClaudeAdapter::handle_tool_use`, `ClaudeAdapter::handle_result`, `ClaudeAdapter::parse_cost`, `Adapter::ingest_line`, `Adapter::finalize`.
- [`crates/adapters/codex/src/lib.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/adapters/codex/src/lib.rs): `CodexAdapter::handle_thread_started`, `CodexAdapter::handle_turn_started`, `CodexAdapter::handle_item_started`, `CodexAdapter::handle_item_completed`, `CodexAdapter::handle_turn_completed`, `Adapter::ingest_line`, `Adapter::finalize`.
- [`crates/orchestrator/src/parser.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/parser.rs): `read_line_capped`, `read_line_resumable`, `drain_physical_line`.
- [`crates/orchestrator/src/supervisor/adapter_supervision.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/supervisor/adapter_supervision.rs): `Supervisor::supervise_with_adapter`, `Supervisor::capture_session_id_once`, `drain_with_timeout`, `classify_exit`.
- [`crates/orchestrator/src/supervisor/real_workers.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/supervisor/real_workers.rs): `Supervisor::spawn_claude`, `Supervisor::spawn_codex`, `Supervisor::spawn_real_cli`.
- [`crates/orchestrator/src/process_tree.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/process_tree.rs): `configure`, `terminate_and_reap`.
- [`crates/orchestrator/src/vendor_profile.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/vendor_profile.rs): `profile_for_vendor`, `render_command_args`.
- [`crates/orchestrator/resources/vendor_profiles/claude.json`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/resources/vendor_profiles/claude.json): `commands.standalone_worker`, `commands.mission_worker`.
- [`crates/orchestrator/resources/vendor_profiles/codex.json`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/resources/vendor_profiles/codex.json): `commands.standalone_worker`, `commands.mission_worker`.

## Parsing, lifecycle and error findings

`ClaudeAdapter::ingest_line` dispatches `system/init`, assistant content blocks, user tool results, `result` and rate-limit records. Session ID is captured from init, text is emitted incrementally and tool events use block IDs. `handle_result` maps usage before the terminal event, guards duplicate results and distinguishes error subtypes. Malformed JSON is surfaced as ordinary stdout log text. `finalize(AdapterExit::Clean)` synthesizes completion when no terminal result was emitted.

`CodexAdapter::ingest_line` dispatches dotted thread/turn/item event names. Thread ID supplies the session; command/file/tool items and agent messages become normalized events. `handle_turn_completed` records token usage and detects error/quota metadata. At this pin `turn.failed` falls into the unknown-kind branch; `finalize` synthesizes completion or failure from process exit. This is useful as a protocol-shape reference but unsuitable as Jojo's source of successful completion evidence.

`read_line_capped` retains at most max+1 bytes to distinguish a record exactly at the cap from overflow, then drains through the physical newline in bounded chunks. `read_line_resumable` retains partial bytes in caller-owned state before each await, preventing lost bytes when a select branch is cancelled. `drain_physical_line` resynchronizes without accumulating the discarded tail. These are the useful techniques reproduced in Jojo's original byte-oriented `LineFramer`: max+1 framing, explicit discard state, persistent partial bytes and exact raw offsets. Jojo has independent reader threads instead of cancellable futures, so it needs no select/future machinery.

`Supervisor::spawn_real_cli` reserves a worker, writes rows, renders vendor-profile argv, sets cwd/PATH, pipes stdout/stderr and spawns through Tokio. Setup errors roll back worker/task rows and fail the slot; a start barrier prevents supervision before registration. `supervise_with_adapter` selects stdout, stderr, cancellation and child wait; EOF disables an arm to avoid a ready-loop spin. It skips post-cancel drain; on natural exit `drain_with_timeout` gives each line 500 ms. This is a per-line bound, not a whole-drain bound under sustained output. `classify_exit` maps wait errors/nonzero status into adapter failure categories.

`process_tree::configure` sets a fresh process group and kill-on-drop. `terminate_and_reap` uses unsafe libc SIGKILL against the negative child PID, falls back to killing the direct child and then waits for it. It does not provide Jojo's immutable start-identity/known-descendant journal or an atomic handle for arbitrary orphan PIDs. Reusing this code would add Tokio/libc and would not satisfy Jojo's verified-stop invariant.

The bundled profiles demonstrate centralized argv templates, but contain permission/sandbox bypass flags. Jojo does not copy those flags, discover live CLIs via PATH, resume sessions, or execute a real provider in this milestone.

## Comparison with P1-08 contracts

| Concern | Vigla at this pin | Jojo P1-09 over P1-08 |
| --- | --- | --- |
| Provider boundary | Canonical event-schema plus mutable adapter core | Existing provider-neutral `EventPayload`, exact parser fingerprint, one normalizer per attempt |
| Session and turn | Captures provider session for later resume | Requires init/thread identity before terminal success; changed/duplicate identity latches a protocol failure |
| Malformed/oversized records | Logs malformed text; truncated lines can be skipped | Captures bytes first, records truncation/protocol violation, blocks later success |
| Exit-zero / missing terminal | Finalizer can synthesize completion | OS exit is separate evidence; never fabricates a provider terminal or seals a result |
| Output durability | Parsed events persisted/emitted | Exclusive raw spools, synced byte checkpoints, events referencing flushed ranges |
| Cancellation | Tokio select, kill/reap, skipped post-cancel drain | Independent bounded reader threads and control path; capability-specific INT -> TERM -> KILL, whole-drain deadline |
| Identity / tree | Fresh process group; child ID used for group kill | Group leader held unreaped until group quiescence; no signals after reaping or to snapshot-only escaped PIDs; UNKNOWN retains claims |
| Admission authority | Vigla worker/task lifecycle | Existing scheduler reservation, epoch/generation, current approvals and worktree binding; issue committed before spawn |
| Runtime/dependencies | Tokio, libc and multiple Vigla crates | Existing Rust core and standard library; no added dependency or copied implementation |

Only the bounded-line/partial-state techniques and vendor event-shape knowledge were reimplemented. Jojo's fixtures are locally authored, including its own failure/session/truncation cases; upstream fixture files were not adapted or copied. The helper retains SQLite/scheduler authority; parser events cannot mutate approvals, claims or outcomes.

## Applicable licenses and retained attribution

The reviewed Rust crates inherit `license.workspace = true`; the root `[workspace.package]` declares **Apache-2.0**. The root `LICENSE` contains Apache License 2.0 and `NOTICE` identifies Copyright 2026 Kilbex and Vigla contributors. Root NOTICE also points to the third-party notice ledger; no third-party dependency implementation was extracted. Reviewed sources contain no overriding file license. Exact LICENSE/NOTICE hashes are retained in the pin record, and the original checkout files remain intact. This work references algorithmic ideas and protocol shapes; no Vigla source text, fixture text, package or transitive dependency was imported, so there is no vendored derivative-code notice file to distribute.
