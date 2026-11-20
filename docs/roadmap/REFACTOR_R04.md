# Refactor R-04: Jojo's core in Zeroshot's shape

Decided by the human on 2026-11-18 (option C of three): Jojo is transformed in place. It is not rebuilt on a fork of another tool and not rewritten. The target shape is Zeroshot's: pure decisions with no IO, a typed adapter per provider, and storage behind narrow functions. Six named algorithms are taken from Zeroshot, Weft, Kandev and Luvus. Jojo's safety core stays as it is: revision-bound approvals, the outbox with once-only external actions, and the Recovery Center. None of the four tools does these better.

This file is the plan. It is not built yet. Each step is recorded in DESIGN_ROADMAP_DIFFERENCES.md (E21 onwards) when it is built.

## 1. Sources

Ideas are taken; code is not copied. Each algorithm is written again against Jojo's own types.

| Repository | Pinned SHA (2026-11-18) | License | Taken |
|---|---|---|---|
| [the-open-engine/zeroshot](https://github.com/the-open-engine/zeroshot) | `8d41128442e4321fefed52db9b738dfcb37a9659` | MIT | Pure reducer over execution history (`zeroshot/src/full_v1_reducer`); plan verification with an execution bound (`crates/openengine-cluster-server/src/graph_verifier`, `analyzer_bounds.rs`); the harness adapter contract and per-provider module split (`zeroshot/src/harnesses`) |
| [SoloJiang/weft](https://github.com/SoloJiang/weft) | `12303b30e3ccabfb91a116bd0b1658fd859aaed9` | MIT | Usage-limit awareness (`src-tauri/src/engine_quota.rs`); facts-to-action state machine (`crates/weft-scheduler`); read-only PR monitor (`src-tauri/src/host/monitor.rs`) |
| [kdlbs/kandev](https://github.com/kdlbs/kandev) | `a7f4cdd6a75c8ee25676e6c8f74c06e450f0c1df` | **AGPL-3.0** | **The idea only, never code:** dependencies released by the predecessor's completion instead of re-evaluating the whole graph (`apps/backend/internal/task/service/service_dependencies.go`) |
| [RizRiyz/luvus](https://github.com/RizRiyz/luvus) | `d57faf89633033dee68ea9e613cdb8125b8a927c` | Apache-2.0 | Decisions made in memory by a single writer and then persisted (`src/orch/mod.rs`); `newly_ready` after a completion |

Not taken: Kandev's code (AGPL, and its quality), Luvus's path leases (Jojo gives each attempt its own worktree), Weft's large modules, and Zeroshot's run ledger (it leaves out fencing and hash chains on purpose; Jojo's outbox is stronger).

## 2. Baseline (measured 2026-11-18, tree `86bf745`)

| Measure | Value | Target |
|---|---|---|
| `jojo-storage` source lines | 30,752 in 41 modules | No crate over 12,000 lines; no module over 1,200 lines |
| Modules in one dependency cycle | 19 of 41, including `tx` and `writer`: approvals, artifacts, briefs, dependencies, engine, features, launch, outcomes, overview, recovery, repairs, runner, shutdown, slice, turns, tx, validation, worktrees, writer | No cycle between crates; the store depends on nothing above it |
| Payloads stored as text | 90 event writers formatting pipe-joined or `Debug` text, provider events stored as `Debug` (`format!("{:?}", e.payload)`), and the hand parsers that read them back (`recovery.rs` `parse_session`, `outcomes.rs` `terminal_status`, `exit_status`, `stream_closed`, the `split('|')` readers in `features.rs` and `validation.rs`). (The first count, "36 parsers", was too high: most `strip_prefix` calls read git output, URLs or fixture scenarios.) | Every event and provider event payload is a versioned record (`jojo_domain::payload`); no hand parser of stored text is left |
| Inline SQL strings | 704 in 29 modules | SQL only in the store crate |
| Scheduler scan | `schedule_with_bindings`, 275 lines, a DB query inside the inner loop, about O(P³) queries in the worst case | One read, a pure decision, one batched write |
| Dependency evaluation | `evaluate_dependencies` for every candidate on every scan | In memory, O(V+E) per scan |
| Child process waits | `try_wait` with 2 to 50 ms `sleep` loops (`process.rs`, `remote.rs`, `discovery.rs`, `shutdown.rs`) | **Withdrawn after reading them (2026-11-18):** each is a bounded wait with a deadline and a kill, not a busy loop on a hot path. The standard library has no wait with a timeout and the guard forbids FFI, so a waiter thread could only kill through a second process. They stay. |
| Read models | N+1 queries per feature row and per event sentence (`features.rs`, `overview.rs`, `details.rs`) | A fixed number of queries per answer |
| Verification | 109 script tests, 517 Rust tests, 161 Swift tests, 18 GUI scenarios; 260 pinned protocol frames | All kept; new tests are added |

## 3. Rules for the whole refactor

1. **Steps 1 to 3 change no behaviour.** The protocol stays at 1.14, the app is not touched, and every one of the 260 pinned frames in `r01_frames.rs` keeps its answer, with one exception (step 1, rule 4). The pinned frames are the regression net, as they were for R-01.
2. **The database starts fresh at schema 20 (decided by the human on 2026-11-18).** Event payloads (`model.rs` `digest`), outbox payloads (`tx.rs`, `payload_digest`) and command receipts are covered by SHA-256 digests, so rows cannot be rewritten in place. They are dropped instead. The human's database held 26 events and an empty outbox, so no external action could be sent twice. The helper refuses a database older than schema 20 and names the file to move aside; it never deletes it itself. The human's `jojo.sqlite3` is moved once to `jojo.sqlite3.pre-r04.bak`, with the app closed, when step 1 starts.
3. **The boundary guard moves with the code.** `scripts/check_p1_01_boundaries.py` names crates and files (`FILESYSTEM_CRATES`, `PROCESS_SURFACE`, `DISCOVERY_SURFACE`, `REMOTE_SURFACE`, `GIT_SURFACES`, the dependency allowlist). Every move updates the guard and its negative tests in the same step. The rules themselves stay: no `unsafe`, no FFI, no network types, and launch surfaces are named one by one.
4. **No new third-party dependency.** `serde` and `serde_json` are already allowed in `jojo-domain`. Serialization lives there, so the store crate needs no new dependency.
5. **One gate per step**, at its end and not between its parts (`gate-r04-1` to `gate-r04-6`). New tests raise the minimums and go into `required`.
6. **Historical documents and reports are not edited.** New decisions go into DESIGN_ROADMAP_DIFFERENCES.md and STATUS.md.

## 4. The six steps

### Step 1: typed payloads and the provider adapter contract

**As built (2026-11-18):**

- `jojo-domain/src/payload.rs`: `Record` writes a canonical JSON object with named members and `"v":1` (keys sorted, through the existing `ipc::encode`); `read` refuses anything but an object of that version; `provider_event` and `provider_event_from` encode every `EventPayload` variant and read it back. The project's own `Json` type is used, as `dependency_wire.rs` does; no `serde` derive is added.
- `Tx::append_event` takes `&Record` instead of text, so formatted text cannot be written as an event any more: the compiler refuses it. All 90 writers and the two raw inserts in `writer.rs` name their fields (an attempt is always `attempt`). Provider events, the `ROOT_EXITED` and `STREAM_CLOSED` observations the core reads back, and the scheduler's launch effect are records too.
- The readers use named fields: `event_summary`, `event_category`, `event_project`, the validation runs and rejections (matched with `payload::identity_pattern`, sound because records are canonical), `parse_session`, `terminal_status`, `exit_status`, `stream_closed`. The Events answer derives its sentence from the whole payload and still sends 512 characters of it.
- Schema 20: triggers refuse an event or provider event payload that is not a record. The production plan refuses an existing database older than 20 (`StorageError::SchemaTooOld`, with the file's path); a new database runs every step. Plans built in tests keep no floor, so the upgrade machinery is still tested.
- The gate fails when a process started from its scratch directory outlives the checks, and stops it (`verify.py`, `left_running`).
- **Moved to step 2:** the provider adapter contract (`command`, `input`, `session`, `transcript`, `version` under `claude/` and `codex/`). Those modules move to `jojo-runtime` in step 2; splitting them first and moving them after would touch them twice. The existing `ProviderAdapter` trait in `jojo-domain/src/provider.rs` is the starting point.
- **Not changed:** `Debug` text in other columns (for example `human_decisions.action` and `origin`, a command request's payload, and digests taken over `Debug` text such as `facts_digest`). They are written and compared by the same build and never parsed. They are listed for step 2, where the SQL moves into the store.

**Exception to rule 1:** `EventDetail` returns a payload's text, which becomes JSON instead of `Debug`. The pinned frames whose answer carries a payload change on purpose. They are counted before the change and listed in the change log, as R-01 did. The Activity & Audit sentence does not change, because it is derived.

**Also in this step:** test runs left fake `gh auth status` processes behind. On 2026-11-18 three of them, from `m2_chat` tests in earlier gates, were still running 7 to 18 hours later, orphaned to PID 1. They were stopped by hand. The cause was not found: the sign-in check already has a time limit and kills its child, and the fake `gh` answers `auth status` at once. So the gate now detects and fails on any such leak, which makes the next one visible with its command line.

**Done when:** no event can be written as text (the type refuses it, the schema refuses it); no hand parser of stored text is left; a database older than schema 20 is refused with the file's name; the 260 frames pass, with the listed exceptions.

### Step 2: break the cycle, then split the crate

**From:** Zeroshot's crate boundaries (protocol, server, product).

First the 19-module cycle is broken inside `jojo-storage`. The store core (`tx`, `writer`, `engine`, `artifacts`) stops importing `shutdown` and the business modules. Read models (`overview`, `features`, `details`) stop calling `turns` and `briefs` and read through store functions instead. Only then is the crate split:

| Crate | Holds | May touch |
|---|---|---|
| `jojo-domain` | Types, protocol, planner, payloads, every pure decision (step 3) | Nothing |
| `jojo-storage` (name kept, so paths and notes stay valid) | `sqlite`, `schema`, `tx`, `writer`, `model`, `digest`, `error`, `queries`, `artifacts`, and one function per query moved out of the 29 modules | The database file only |
| `jojo-runtime` (new) | `process`, `claude`, `codex`, `executables`, `discovery`, `remote`, `ownership`, the git half of `worktrees`, the process half of `validator` | Files and the named launch surfaces |
| `jojo-engine` (new) | `runner`, `scheduler`, `turns`, `repairs`, `advisory`, `briefs`, `slice`, `dependencies`, `approvals`, `external`, `recovery`, `outcomes`, `evidence`, `projects`, `launch`, `shutdown`, `validation`, read models | `jojo-storage` and `jojo-runtime` through their functions |
| `jojo-helper` | The serve loop and dispatch | `jojo-engine` |

The N+1 read models become a fixed number of batched queries while their SQL moves into the store.

Moved here from step 1: as `claude` and `codex` move into `jojo-runtime`, each provider follows one adapter contract, growing the existing `ProviderAdapter` trait: `command`, `input`, `session`, `transcript` and `version`, each in its own module (Zeroshot `harnesses`). An unknown record kind still ends a run as a protocol failure; the M1 strictness does not change. The `Debug` text still written to other columns (section 4, step 1, "Not changed") becomes named words as its SQL moves.

**Done when:** `cargo tree` shows no edge from `jojo-storage` or `jojo-runtime` to `jojo-engine`; there is no SQL outside `jojo-storage`; the module and crate size limits of section 2 hold; the 260 frames pass unchanged.

### Step 3: pure decisions

**From:** Zeroshot `full_v1_reducer`, Weft `weft-scheduler`, Luvus `orch`, Kandev's dependency idea.

- **Scheduler:** `jojo_domain::scheduler::decide(&Snapshot, &Config) -> Vec<Decision>`. The store reads the queue, claims, barriers, cooldowns and the cursor in one read. The engine writes the decisions, claims, effects and events in one transaction. The admission algorithm is kept exactly: priority aging, round-robin over repositories from the cursor, the drain barrier after three skips, shared and exclusive units, and the exclusive drain gate.
- **Dependencies:** evaluated once per scan, in memory over the snapshot (Kahn order, O(V+E)), instead of a query per candidate. When a step is sealed, only its successors are checked (`newly_ready`).
- **Runner:** `step_state` (288 lines) and the repair, hold and review choices become `jojo_domain::runner::route(&StepFacts) -> StepAction`, a facts-to-action function as in Weft.
- **Proof of sameness:** while this step is built, the old `schedule_with_bindings` stays as a test oracle. The `t06_scheduler` scenarios and a generated set of queues run through both, and the decisions must be equal. The oracle is deleted at the end of the step.

**Done when:** the decision functions have no IO (the guard checks that they live in `jojo-domain`); the oracle comparison passes; the 260 frames pass unchanged.

### Step 4: plan verification with an execution bound

**From:** Zeroshot `graph_verifier`, `analyzer_bounds.rs`.

- `jojo_domain::planner::verify(&Plan, &ProjectPolicy) -> Result<Bound, Vec<Finding>>` checks that dependencies exist and form no cycle, that the plan has one repository, and that every step is reachable. It also computes the worst case: each step's attempt × (1 + the repair limit), plus a review per attempt when the reviewer is on.
- A plan that fails verification cannot be approved; the approval command refuses it with the findings.
- **Interface (decided by the human on 2026-11-18):** on the plan approval card in the Approval Inbox, one fact line ("At most 9 executions: 3 steps, up to 2 repairs, reviewed"). An invalid plan shows as a Banner in the same place. Both use existing components; no new board.
- Protocol 1.15: `Document` for a plan carries `bound` or `findings`. No schema change (derived).

### Step 5: usage limits

**From:** Weft `engine_quota.rs`.

- Claude Code's `rate_limit_event` record (`status`, `resetsAt`, `rateLimitType`, `utilization`) is read into a typed value. Today only its kind is kept. For Codex, the profiled `exec --json` stream is checked first. If it carries no structured limit, Codex stays "Not reported"; no limit is ever guessed from error text.
- An attempt that ends with `rejected` does **not** count as a failure and does not spend a repair. The step waits until `resetsAt` and then runs again. The scheduler's existing provider cooldown (`provider_is_cooling`) carries the wait.
- Schema 21 (the newest snapshot per provider), protocol 1.16. The places the canvas already draws are filled: the Dashboard's provider row ("Codex limited since 09:12", "Limited") and the Usage group of Settings & Providers.

### Step 6: read-only PR monitor

**From:** Weft `host/monitor.rs`.

- For each pull request Jojo opened that is still open, `gh pr view --json state,mergeable,statusCheckRollup,url` runs every five minutes, and once when Source Control is shown. The answer is written only to Jojo's own table. No mutating subcommand is ever run.
- The guard's `REMOTE_SURFACE` gets an explicit list of `gh` subcommands: `pr create` (M5) and `pr view`.
- Schema 22, protocol 1.17. The canvas places are filled: Source Control's CI field and the "Inspect CI" button (the checks in the inspector).
- This step closes the "PR status sync" candidate of STATUS.md.

## 5. Open points for the human

1. **Decided 2026-11-18:** the database starts fresh (rule 2), and test leftovers were deleted: 767 temporary directories (188 MB), 28 scratch files in `/private/tmp`, the empty `~/Projects/jojo_test` and `~/JojoDisposable/shell-look`, and three orphaned test processes. The live evidence under `~/JojoDisposable`, which the documents cite, was kept.
2. **Decided 2026-11-18:** the store keeps the name `jojo-storage`.
3. **Step 6:** checking the monitor against a real pull request needs the human's authorization (one `gh pr view` against `kaanf/jojo-m5-sample` #1; read-only). Asked when step 6 starts.
4. **Decided 2026-11-18:** the steps run in order, 1 to 6.
