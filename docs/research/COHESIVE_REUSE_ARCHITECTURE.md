# Cohesive reuse architecture — decision review

Status: **read-only research decision, 2026-11-14.** No production code, roadmap, brief, ADR or
dependency changed. P1-09 is **not** started. This document evaluates whether selective reuse of
seven open-source projects can keep Jojo one cohesive native, fast, low-memory macOS app. Normative
inputs: [proposed architecture](../architecture/PROPOSED_ARCHITECTURE.md),
[core model](../architecture/CORE_STATE_MODEL.md), [scheduler](../architecture/PARALLEL_SCHEDULER.md),
[provider contracts](../architecture/PROVIDER_CONTRACTS.md), [shutdown](../architecture/SHUTDOWN_RECOVERY.md),
[accepted U01–U05](../architecture/ACCEPTED-DECISIONS-2026-11-14.md),
[Phase 1 brief](../planning/PHASE_1_IMPLEMENTATION_BRIEF.md) and the
[component matrix](COMPONENT_EXTRACTION_MATRIX.md) (C01–C37).

**Evidence limit.** Upstream facts below come from repository pages, READMEs and published
`Cargo.toml`/`pyproject.toml` read on 2026-11-14. No candidate was cloned. Commit SHAs and LICENSE
hashes are **not pinned here**; any later PORT or REPRODUCE must first add them to
[`research/REPOSITORY_PINS.json`](../../research/REPOSITORY_PINS.json) under the existing provenance
discipline. Per-file claims marked *(unverified)* come from a README or tree listing, not from reading the file.
Memory/CPU figures for other runtimes are rough general estimates, not measurements. They only
explain the ranking, and the benchmark gates in §4 decide.

## 1. Current baseline

| Fact | Evidence |
|---|---|
| P1-01–P1-08 complete, P1-08 GO for P1-09 | `docs/verification/P1-08.md`: 209 Rust tests, 30 boundary tests |
| One Rust core, four crates: `jojo-domain`, `jojo-storage`, `jojo-helper`, `jojo-test-support` | `Cargo.toml` workspace |
| Normal dependencies: `serde`, `serde_json`, `rusqlite =0.37.0`, `sha2` | crate manifests; `Cargo.lock` holds **36** packages |
| **No async runtime yet**; synchronous single writer (`BEGIN IMMEDIATE`, WAL, one epoch) | `crates/jojo-storage/src/writer.rs`, `engine.rs` |
| Scheduler, approvals, worktree ownership, PRD/artifacts, dependency predicates already in the core | `jojo-storage/src/{scheduler,approvals,worktrees,artifacts,dependencies}.rs` |
| Provider DTOs (LaunchSpec, capabilities with Supported/Unsupported/Unknown, normalized events) + FakeClaude/FakeCodex | `jojo-domain/src/provider.rs`, `jojo-test-support/src/provider.rs` |
| SwiftUI shell exists only as a boundary stub (15 lines) | `macos/JojoApp` |

Remaining Phase 1 work: P1-09 process manager and raw streams, P1-10 outcomes/validation/evidence,
P1-11 reopen/continuation, P1-12 shutdown coordinator, P1-13 stdio IPC, P1-14 SwiftUI shell,
P1-15–P1-17 acceptance, durability and readiness. Every one of these already has a single owner
in the existing design. **The question for each candidate is whether it makes one of those owners
smaller without adding a second owner.**

## 2. Admission test for any reuse

A candidate fails automatically if it:

1. Adds a second state authority (another database, GitHub labels, a log directory or an
   in-memory workflow history treated as truth), or a second SQLite binding.
2. Adds a scheduler, approval engine, lease/takeover model or workflow/replay runtime alongside
   `jojo-storage::{scheduler,approvals,writer}`.
3. Adds a resident process, TCP/HTTP/WebSocket listener, daemon, or a Node/Python/.NET/Bun
   interpreter per session or per app.
4. Requires a webview (Tauri/Electron/WKWebView-hosted React) in the GUI.
5. Reads provider-private files (`~/.claude*`, `~/.codex`, auth caches or session logs), bundles
   or modifies a provider binary, or weakens a permission default. All of these are forbidden by
   [provider contracts](../architecture/PROVIDER_CONTRACTS.md#boundary-and-authority).
6. Brings a second async runtime, or requires one before Jojo has chosen its own.

## 3. Candidate evaluation

The seven numbered points from the request appear as columns or rows below. "In-core" means
implementing the same function inside the existing Rust crates, which is the comparison baseline.

### 3.1 ACP Rust SDK

| | `agent-client-protocol-schema` | `agent-client-protocol` (runtime) |
|---|---|---|
| Source / license | crates.io 1.18.0, repo `agentclientprotocol/agent-client-protocol`; `license = "Apache-2.0"` in Cargo.toml | crates.io 5.2.0, repo `agentclientprotocol/rust-sdk`; Apache-2.0 (inherited from the workspace; root file not read) |
| Transitive deps | `anyhow`, `derive_more`, `serde`, `serde_json` (`preserve_order`, `raw_value`), `serde_with`, `strum`; `schemars` is optional but **on by default** | schema crate, derive crate, `futures`, `futures-concurrency`, `rustc-hash`, `tracing`, `uuid`, `anyhow`; features `process`/`stdio` add `async-io`, `async-process`, `blocking`, `rustix` |
| Runtime overhead | none (types only) | A full connection/handler/role framework with its own executor-agnostic async stack. Combined with Tokio later, the binary would carry two async I/O stacks. |
| Integration | Would only make sense inside a future provider adapter, mapping ACP `session/update` and permission requests onto `jojo_domain::provider` normalized events | Would replace the process manager and the event path that P1-09/P1-13 own |
| Duplication / risk | ACP is an **editor ↔ agent** protocol. Jojo talks to official CLIs directly (`claude -p` stream-json, `codex exec --json`, Codex app-server). ACP sits in front of those CLIs only through adapters (§3.2), so taking the dependency implies taking an adapter process too. `serde_json` features `preserve_order`/`raw_value` would unify into Jojo's build and change map ordering globally. | Duplicates the IPC/event layer and stop semantics. Its handler model does not separate "interrupt acknowledged" from "process observed dead". |
| Decision | **Defer (conditional dependency).** Not in Phase 1. Revisit only if a later approved live milestone selects an agent whose *only* supported programmatic interface is ACP. In that case, depend on the schema crate with `default-features = false`, inside one adapter module. | **Reject.** |
| vs in-core | In-core DTOs already exist (P1-08) and cost nothing. | In-core P1-09 on `std::process` (or one chosen runtime) is smaller and keeps stop semantics. |

### 3.2 Claude and Codex ACP adapters

| | `claude-agent-acp` | `codex-acp` |
|---|---|---|
| Source / license | `agentclientprotocol/claude-agent-acp`, TypeScript, Apache-2.0 (README/contribution policy; LICENSE text not read). Uses the Claude Agent SDK *(package.json unverified)*. Release archives bundle the `claude` CLI. | `agentclientprotocol/codex-acp`, **TypeScript** (not Rust), Apache-2.0 per README. npm package includes an `@openai/codex` dependency and starts Codex app-server |
| Runtime overhead | +1 Node or Bun process per session between Jojo and the official binary. That is typically tens of MB RSS plus JS startup, multiplied by concurrency (estimate). | Same: +1 Node process per session, plus the app-server child it starts |
| Integration | Would sit between `PrepareLaunch`/`Start` and the CLI, owning permission prompts and session mapping | Same, for Codex |
| Duplication / risk | Duplicates the provider adapter. Bundling `claude` contradicts "user-installed, unmodified official binary; do not package a provider binary". The adapter can also hide child processes from Jojo's process-tree identity, so stop and evidence become second-hand. | C13 already plans a direct Rust stdio client for the official Codex app-server. codex-acp is a translation layer over that same server. |
| Decision | **Reject** (runtime and code) | **Reject as runtime.** Allow use as a **reference only** for which app-server requests and notifications a real client needs. Protocol truth must still come from pinned official `openai/codex` sources (C12), not from the adapter. |
| vs in-core | In-core adapter: zero extra processes, exact process identity, existing stop policy | Same |

### 3.3 Vigla

- **Source / license:** `Kilbex/Vigla`, Apache-2.0 (`LICENSE`, `NOTICE`, `THIRD_PARTY_NOTICES`). Rust
  workspace with a Tauri 2 + React 19 UI. 8 commits on main, 114 stars at review time: young and unproven.
- **Relevant modules** *(from ARCHITECTURE.md, files not read)*:
  - `crates/orchestrator/src/supervisor/`
  - `mission_worker_dispatch.rs`
  - `mission_workspace/mod.rs` (worktrees, integration, merge, revert)
  - `mission_runtime/` (state machine and replay)
  - `arbiter/`, `audit/`, `recovery/`, `acl/`
  - `crates/adapters/{core,claude,codex}`
  - `crates/orchestrator/resources/vendor_profiles/`
- **Transitive deps:** `tokio` (process, io-util), `sqlx 0.8` (sqlite), `specta`/`tauri-specta` RCs,
  `blake3`, `zstd`, `tracing-appender`, `thiserror`, plus Tauri/WebKit and a JS toolchain.
- **Runtime:** a webview app. `sqlx` alongside `rusqlite` means two SQLite bindings. Both link the
  native `sqlite3` library, and Cargo allows only one such library per build, so the two crates are
  likely incompatible in one workspace. Verify with `cargo tree -i libsqlite3-sys` before relying on this.
- **Duplication:** mission state machine, supervisor, arbiter, recovery and its SQLite repository each
  duplicate an existing Jojo owner.
- **Worth reproducing (algorithm only, no code):**
  1. **Line-delimited stream adapters driven by a versioned per-vendor profile** (`vendor_profiles` +
     `adapters/{claude,codex}`). This maps onto P1-09 parser fingerprints and LaunchSpec argv
     templates. Jojo keeps its own byte-capture-before-normalize rule and synthetic fixtures.
  2. **Orchestrator-only commits plus a post-commit changed-path scope check** (`acl/`, `.vigla/acl.json`
     sentinel). This fits P1-10 independent validation: the changed-path set must stay within the
     approved plan's declared scope, or validation fails.
  3. **Mission revert as a normal `git revert` of the recorded merge commit.** Relevant only to later
     integration phases, not Phase 1.
- **Decision:** **REPRODUCE items 1–2 as algorithms, with attribution in the provenance ledger. Reject
  code, Tauri, sqlx and the runtime.**
- **vs in-core:** Same core code either way. Vigla only shortens design time for two small checks.

### 3.4 Armada (two unrelated projects share the name)

| | `jchristn/Armada` | `mgcrea/armada` |
|---|---|---|
| License | MIT (`LICENSE.md`) | Root MIT for docs/scripts only. `apps/apple/LICENSE` is **source-available, binary redistribution reserved** (not OSI). |
| Stack / runtime | C#/.NET 10 "Admiral" **server**. REST/WebSocket and MCP ports (7890/7891 per README summary), SQLite/Postgres/SQL Server/MySQL, React dashboard, Avalonia desktop app | Swift macOS menu-bar watcher. Opt-in MCP listener on 127.0.0.1. Reads `~/.claude*` and `~/.codex` and **writes** the `hasTrustDialogAccepted` key into `.claude.json` |
| Overlap | Scheduler (`MaxParallelVoyages`), worktree per mission, merge queue, approvals | Session status/usage display only |
| Risk | Full duplicate control plane plus resident server. README summary shows Claude launched with `--dangerously-skip-permissions`, which conflicts with Jojo's permission envelope. | Reading and writing provider-private files violates the provider boundary. The license blocks porting app code. |
| Decision | **Reject** | **Reject** (at most a UI idea reference: per-session status chips; no code) |
| vs in-core | Jojo already has the scheduler (P1-06) and worktree ownership (P1-07). | Jojo derives status from its own captured streams (`UsageObserved`, `ProviderTerminal`), which is authoritative and boundary-safe. |

### 3.5 Duroxide

- **Source / license:** `microsoft/duroxide`, crates.io 0.1.32 (2026-10-11), **MIT**. Self-described
  preview/alpha.
- **Modules:** runtime with orchestration and worker dispatchers, a session manager, a replay
  engine, and `duroxide::providers::sqlite::SqliteProvider`. Tables: `instances`, `executions`,
  `history`, `orchestrator_queue`, `worker_queue`, `sessions`.
- **Transitive deps:** `tokio` with `features = ["full"]`, `tokio-util`, `async-trait`, `tracing`,
  `tracing-subscriber` (fmt/json/env-filter, unconditional), `metrics` (unconditional), `semver`,
  `uuid`. The `sqlite` feature adds `sqlx 0.8` (`runtime-tokio-native-tls`, `macros`, `migrate`,
  `chrono`) and bundled `libsqlite3-sys`. That pulls in native-tls, chrono and sqlx macros, which
  would several-fold the current 36-package lockfile (estimate).
- **Runtime:** background dispatcher tasks with **lock renewal** (`locked_until`, about a 30 s lock
  timeout in the design doc). Orchestration code must be deterministic and replayable.
- **Duplication / risk:**
  - It is a second workflow runtime, a second queue/lease model and a second SQLite database and
    binding (the same sqlx/rusqlite conflict as §3.3).
  - Lease expiry implies takeover. That contradicts the accepted rule that leases are liveness hints
    and that unknown-live work is quarantined, never taken over on timeout.
  - Replay determinism would add a second determinism discipline on top of Jojo's reducer rules.
  - "Activities at most once per logical step" is the same claim Jojo deliberately refuses to make
    for process launch (issued-unknown handling).
- **Decision:** **Reject.** No algorithm needs reproducing. Jojo's append-only facts, receipts,
  epochs and issued-unknown journal (P1-02, P1-06, P1-10) already cover durable progress with
  stricter semantics.
- **vs in-core:** In-core needs zero new crates and zero background tasks.

### 3.6 Issue Orchestrator

- **Source / license:** `issue-orchestrator/issue-orchestrator`, **Apache-2.0** (`LICENSE`, `NOTICE`;
  trademark retained by the author). Python ≥3.11, version 0.10.0, early beta.
- **Structure:** hexagonal `src/issue_orchestrator/{control,observation,domain,ports,adapters,execution}`,
  with boundaries enforced by import-linter contracts in `pyproject.toml`.
- **Deps:** `fastapi`, `uvicorn`, `sse-starlette` (web dashboard server), `textual`, `pexpect` (PTY),
  `transitions`, `httpx`, `keyring`, `PyJWT`, `mcp`, `pystray`, `Pillow`, plus tmux/iTerm terminal adapters.
- **Truth model:** GitHub labels plus observed worktree state.
- **Risk:**
  - Python runtime, a local web server and PTY/tmux control.
  - GitHub as state authority conflicts with SQLite authority and with GitHub being read-only by default.
- **Useful ideas:** all of them already exist in Jojo's design:
  - observe before mutating
  - agent completion treated as untrusted, with validation of the exact commit
  - agents cannot merge
  - bounded rework
  - layered import contracts (Jojo enforces these through `scripts/verify_*` boundary tests)
- **Decision:** **Reject (concept confirmation only).** No code, no algorithm port. If wanted, P1-17
  can cite it as independent prior art for the validation-gate invariants.
- **vs in-core:** No delta.

### 3.7 AgentTrace (several unrelated projects share the name)

| | `luoyuctl/agenttrace` | `Rayfts/AgentTrace` | `Karthikvenugopal/AgentTrace` |
|---|---|---|---|
| License / stack | MIT; Rust TUI/CLI | Apache-2.0; Rust + Tauri/React; SQLite (`.agenttrace/agenttrace.db`) | MIT; Python |
| Function | Parses existing agent session logs (`crates/agenttrace-core/src/parser.rs`) for cost/tokens/latency | Records harness events via `run` (process wrapper) or `import`; optional HTTP server on `127.0.0.1:4319` | Inference-serving workload profiler (vLLM replay, OTel, Grafana) |
| Risk | Its input *is* provider-private session logs, which are forbidden | Second SQLite store plus a server, and wrapping a process duplicates P1-09. 3 commits. | Off-topic |
| Decision | **Reject** | **Reject code. REPRODUCE one concept:** tag each normalized field's provenance as `native` / `derived` / `inferred` / `unavailable`. That fits P1-10 evidence completeness alongside the existing `Supported/Unsupported/Unknown` capabilities. | **Reject** |

Cursor's `agent-trace` spec returned 404 and was not evaluated.

## 4. Benchmarks and resource budgets

These budgets are proposals to adopt as P1-16/P1-17 gates. They are not current measurements: no
helper binary with IPC exists yet. Platform: Apple Silicon, macOS release build, warm disk cache
unless stated otherwise, median of 10 runs and p95 reported. Any candidate that adds a resident
process or a per-session interpreter fails the process-count gate before memory is measured.

| ID | Metric | Budget | Method |
|---|---|---|---|
| B1 | Helper start → handshake-ready (P1-13), empty DB | p95 ≤ 150 ms | timestamped stdio handshake from a harness, `/usr/bin/time -l` |
| B2 | Same, DB with 100k events / 1k attempts | p95 ≤ 400 ms; migration and recovery inspection included | seeded fixture DB |
| B3 | GUI cold launch → first snapshot rendered | p95 ≤ 1.0 s | os_signpost intervals, `xctrace` App Launch |
| B4 | Idle process count | exactly **2** (GUI + helper); 0 listeners | `lsof -p`, `lsof -iTCP -sTCP:LISTEN` |
| B5 | Idle memory, 5 min after launch | helper `phys_footprint` ≤ 20 MB; GUI ≤ 80 MB | `footprint`, `vmmap --summary` |
| B6 | Idle CPU / wakeups | helper avg ≤ 0.1 % over 60 s; no periodic timers when no work is admitted | `powermetrics --samplers tasks`, `top -stats pid,cpu,idlew` |
| B7 | Active overhead per supervised attempt (helper side, excluding the provider tree) | ≤ 4 MB plus configured spool buffers; 4 concurrent fakes ≤ 48 MB helper total | fake-provider fixtures at cap 4 |
| B8 | Flood: 4 fakes × 50 MB/s stdout | helper ≤ 1 core; RSS bounded (no growth after 30 s); stop/close command p99 ≤ 100 ms | P1-09 flood fixture + timestamped control command |
| B9 | Event/log bounds | GUI event backlog ≤ 1,024 frames / 4 MiB, then snapshot-resync; per-attempt raw spool cap with explicit `StreamTruncated` | assert in T09/T13 |
| B10 | Dependency weight | normal-dependency crate count reported per change (baseline 36); helper release binary ≤ 15 MB stripped | `cargo tree -e normal --prefix none \| sort -u`, `ls -l` |

Per-candidate delta gate: a reuse proposal must report the B4–B10 delta against an in-core prototype
of the same function. Any of these rejects it: a positive B4 delta, > +2 MB on B5, any B6 regression,
or a second copy of tokio, sqlite3 or a JSON stack. For the reproduce-only items in §5, the gate is
the same as for any in-core code.

**Runtime choice for P1-09 (recommendation within the existing contract, not a roadmap change).**
At the accepted default cap of one, with low tens of concurrent trees at most, plain `std::process`
with one reader thread per pipe and bounded `sync_channel`s into the existing synchronous writer
meets B6–B8. It adds no dependency and needs no reactor. If Tokio is chosen instead, use a
`current_thread` runtime with only the `process`, `io-util`, `sync` and `time` features, and never
`full`. Either way there must be exactly one async model in the helper, and that choice determines
which future crates are admissible.

## 5. Ranked recommendation

1. **Implement P1-09–P1-17 in the existing Rust core with no new runtime dependency** (baseline;
   wins every comparison above). Keep `rusqlite` as the only SQLite binding and the single writer
   as the only state authority.
2. **REPRODUCE: vendor-profiled line-JSON adapters** (Vigla `adapters/{claude,codex}` +
   `vendor_profiles`) as a design reference for P1-09 parser fingerprints and argv templates.
   Fixtures stay synthetic, with an attribution entry if any structure is adapted.
3. **REPRODUCE: post-commit changed-path scope check** (Vigla `acl/`) as a P1-10 validation fact.
4. **REPRODUCE: field provenance tags** (Rayfts/AgentTrace `native/derived/inferred/unavailable`)
   for P1-10 evidence completeness. Optional and small.
5. **REFERENCE (post-Phase 1): codex-acp's app-server request coverage** as a checklist for the live
   Codex adapter (C13), always validated against pinned official `openai/codex` sources.
6. **DEFER (conditional dependency): `agent-client-protocol-schema`** with `default-features = false`,
   only if a future approved provider is reachable solely via ACP.
7. **CONCEPT CONFIRMATION: Issue Orchestrator** validation-gate invariants, as prior art in P1-17.

## 6. Explicit rejection list

| Rejected | Primary reason (§2 rule) |
|---|---|
| `agent-client-protocol` runtime crate | Second async/connection stack; duplicates P1-09/P1-13 (2, 6) |
| `claude-agent-acp` | Node/Bun process per session; bundles the provider binary (3, 5) |
| `codex-acp` as runtime | Node process per session wrapping app-server that C13 calls directly (3) |
| Vigla code, Tauri UI, `sqlx` store, mission runtime/supervisor/arbiter | Webview, second SQLite binding, duplicate scheduler/state machine (1, 2, 4) |
| `jchristn/Armada` | .NET resident server, network listeners, duplicate scheduler, permission-bypass defaults (2, 3, 5) |
| `mgcrea/armada` | Reads and writes provider-private files; non-OSI app license (5) |
| Duroxide | Second workflow/replay runtime, lease takeover, tokio `full` + sqlx + second DB (1, 2, 6) |
| Issue Orchestrator runtime/code | Python, web server, PTY/tmux, GitHub-label state authority (1, 3) |
| `luoyuctl/agenttrace` | Input is provider-private session logs (5) |
| `Rayfts/AgentTrace` code/server | Second SQLite store, local HTTP server, process-wrapper duplicate (1, 3) |
| `Karthikvenugopal/AgentTrace` | Out of scope (inference-serving profiler) |
| Any reading of `~/.claude*`, `~/.codex`, provider session/auth files | Provider boundary (5) |

## 7. Residual risks

- Vigla and Rayfts/AgentTrace are very young. Reproduced ideas must be justified by Jojo's own tests,
  not by upstream maturity.
- ACP adoption by providers could change the trade-off in §3.1 and §3.2. Re-run this review when a
  live-provider milestone is scoped (U06–U08 remain unresolved).
- The B-series budgets are untested targets. P1-16 should either measure and confirm them or record
  justified revisions. Do not loosen them silently.
- The sqlx/rusqlite native-library conflict is inferred from crate metadata, not built. It is
  irrelevant while both are rejected.
