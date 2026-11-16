# Jojo — Provider-neutral execution contracts

Status: proposed interfaces, not implemented code or a live-provider capability certification. Both official Claude Code and Codex CLIs are required eventually; their protocol details remain release-specific. Phase 1 uses fake adapters and sanitized/synthetic protocol fixtures, never provider inference. [Core model](CORE_STATE_MODEL.md) owns attempts/results; [shutdown](SHUTDOWN_RECOVERY.md) owns application lifetime.

## Boundary and authority

Jojo launches user-installed, unmodified official binaries; the binaries own native subscription sign-in, refresh, credential storage and inference. Jojo does not implement OAuth, read provider Keychain entries/auth caches, proxy HTTP model requests, copy bearer tokens, rotate accounts or substitute an API-key path. Authentication uncertainty blocks execution. Provider-side billing settings cannot be proven safe merely by an absent environment key; the later live milestone must establish an acceptable supported status/user confirmation process. Existing policy qualifications remain in [Stage 1 security](../research/LICENSING_AND_SECURITY.md#official-cli-authentication-boundaries).

Use an explicit executable path verified outside an agent-controlled repository PATH; pin supported version/capability fingerprint, not the Stage 1 default-branch HEAD as an installed release. Omit inherited API-key/model-routing/telemetry overrides that could change billing or data routing; do not modify the official binary or promise that environment filtering disables every configured provider option. Supported config inspection must redact secrets and return mode/categories only. No undocumented token/status endpoint or inference-based auth probe.

A provider may propose files, commands, questions or candidate outputs; it cannot approve a Jojo PRD/ADR/plan, write final SQLite results, release resource claims or decide an integration dependency passed. Tool-permission approval is different from product approval. Child processes receive no Jojo control handles, database handles or approval authority.

## Conceptual operations

These are typed message/trait contracts to implement later, not Rust/Swift source.

| Operation | Request / result contract |
|---|---|
| InspectCapabilities | Approved executable path/version, protocol family, feature flags and supported policy surface. Non-inference local help/status only when later authorized. Report unavailable/unsupported explicitly. |
| CheckReadiness | Provider mode categories `NATIVE_SUBSCRIPTION`, `NOT_SIGNED_IN`, `UNSUPPORTED_MODE`, `UNKNOWN`; binary/config/capability readiness; no credentials. Mode unknown denies launch. Phase 1 supplies fake facts. |
| PrepareLaunch | Frozen LaunchSpec → validated argv/stdin/config references, parser version and launch fingerprint; no side effects or implicit auth. |
| Start | One durable admitted attempt/dispatch token → process identity observations and ordered events. No retry of issued-unknown Start. |
| SupplyInput | Exact live attempt/session/turn and question ID + expected revision; only if capability supports it, otherwise UnsupportedInput. |
| AnswerToolPermission | Exact pending provider request ID, restricted decision and deadline; human answer through trusted UI, never model self-answer. Unsupported CLI mode fails/pause, not permission bypass. |
| RequestInterrupt | Attempt/process identity and stop policy → interrupt-request acknowledgement. May finish a turn but does not assert process exit. |
| ObserveExit / TerminateTree | OS-backed identity/exit/group/known-child observations; provider module cannot fake confirmed death from a terminal message. |
| PrepareContinuation | Exact historical session reference, reviewed repository snapshot and continuation plan → supported continuation LaunchSpec or reason to use fresh context. Creates a new attempt via scheduler. |

The process manager owns spawn, file descriptors, log spooling, process groups, cancellation escalation and exit observation. Adapters own CLI translation and stream semantics. The scheduler alone owns admission/resources. A provider cannot hide another factory-level invocation inside its Start implementation; internal CLI tools/subagents are recorded as part of that invocation's tree/effects.

## Capability model

Capture booleans/enums for structured events, explicit session IDs, exact-session continuation, incremental user input, question IDs, permission response channel, turn interrupt mechanism, process-tree cooperation, output schema, workspace policy, and supported auth-status reporting. Capability values are `SUPPORTED`, `UNSUPPORTED` or `UNKNOWN`, with versioned evidence; UNKNOWN is not a default yes. Also declare whether cwd may change for session continuation and which config/hooks/background tools can execute.

One-shot print/exec modes need not support interactive tool approval or mid-turn input. They may deny prohibited actions or end for user attention. If a required capability is missing, reject that planned execution mode or choose a supported official mode under the same approved permission/auth envelope. Never simulate a protocol capability by feeding terminal keystrokes to a JSON parser.

## LaunchSpec

Required fields: attempt/step/plan IDs; engine epoch and dispatch generation; provider identity; executable/version/schema fingerprint; worktree ID and expected common-dir/base/candidate source vector; prompt/input digest and separately bounded prompt bytes; exact provider session/turn if continuing; role; approved tool/filesystem/network permission envelope; working directory; environment allowlist/config references; timeout/output limits; raw output directory and parser fingerprint; current approval IDs; required outputs/validation plan reference. No token or secret value field exists.

Use argv/stdin documented by the chosen CLI release; no shell string interpolation, shell expansion, arbitrary runtime templates, blind `--last` or unreviewed user PATH resolution. Standard input and control messages are separate. Logs contain hashes/references for prompts/config where sensitive; do not print complete environments. Record repository instruction files, hooks, plugins and global CLI-config fingerprint/known inspection limits as execution inputs: a worktree alone does not prevent their code execution. Optional background/remote effects must be outside the Phase 1/live-pilot permission envelope unless separately approved.

## Event contract

Every normalized event has attempt ID, process generation, adapter/schema version, monotonic per-attempt sequence, raw stream byte reference, observed time and payload. DB event sequence is the writer's global order, not the provider timestamp. Normalize only after bytes are captured; reject stale generations for canonical state changes while retaining diagnostic bytes.

Event kinds: `ProcessStarted`, `SessionIdentified`, `TurnStarted`, `TextDelta`, `ToolStarted`, `ToolFinished`, `QuestionRequested`, `PermissionRequested`, `UsageObserved`, `ProviderTerminal`, `ProviderError`, `InterruptAcknowledged`, `ProcessExited`, `StreamTruncated`, `ProtocolViolation`. ProviderTerminal payload retains success/error/interrupted/unknown, exact turn ID and raw status; it is not an AttemptResult. Usage is advisory and not a cash bill/subscription quota guarantee.

Bound frames/line length, total backlog and parser nesting. Stream NDJSON across chunk/UTF-8 boundaries; maintain separate stdout/stderr offsets. Unknown informational records are preserved, but an unknown terminal/protocol shape cannot produce success. Drain-control separation prevents a stdout flood from starving cancel. Parser overflow/malformed required records request stop and mark protocol/evidence failure; do not silently drop records then claim completeness.

## Official mapping and version cautions

Official pages checked 2026-11-14; implementation must pin and test actual installed releases later. Existing pinned source evidence is preserved in [Optio audit](../research/OPTIO_INTERNALS.md#codex-integration-and-protocol-fidelity), especially official Codex `7924f95421a326bb66426a3ce232b72550948470` `codex-rs/exec/src/exec_events.rs`; do not copy Optio's older display parser as the current contract.

| Provider/mode | Proposed mapping | Important limitation |
|---|---|---|
| Claude programmatic CLI | Supported `-p` structured/stream output, exact recorded session; final `result` mapped to ProviderTerminal | Last applicable result/error matters; exit zero alone is insufficient. Stream input/resume/permissions depend on documented release options. |
| Codex bounded CLI | `codex exec --json`, dotted thread/turn/item events; nested agent messages/tools mapped explicitly | Generic error parsing alone loses lifecycle/session information. `turn.failed` is failure evidence, not successful completion. |
| Codex rich official mode | Separate app-server child via stdio, initialize/initialized handshake, correlated requests and server notifications | This is a distinct mode contract; do not parse its RPC notifications as `exec` JSONL. Feature negotiation/schema matched to installed binary. |

The [Claude programmatic guide](https://code.claude.com/docs/en/headless) documents structured execution and distinguishes SIGINT turn interruption from SIGTERM exit, including unfinished turns and possible missing result. The [Codex non-interactive guide](https://developers.openai.com/codex/noninteractive) documents structured execution and exact session continuation. The [app-server guide](https://developers.openai.com/codex/app-server) documents turn interruption; an interrupt response acknowledges a request, not local process-tree termination.

No guarantee is made that either provider preserves exact in-memory state, exposes arbitrary session metadata without model use, or ends every remote tool job when the CLI exits. Missing continuation metadata is not a reason to read private session/auth formats or launch an inference probe.

## Stop, completion and continuation

Apply adapter-supported graceful turn interruption first within the approved budget, then OS termination as needed. App-server turn completion/interruption is not process exit; an idle CLI/app-server is still owned work during application shutdown. A CLI exit while descendants remain cannot release safety resources. Missing provider result after an intentional stop is valid partial evidence for CANCELLED/INTERRUPTED, never SUCCEEDED. [Core race rules](CORE_STATE_MODEL.md#immutable-result-and-race-rules) decide outcome ordering.

Continuation first validates process absence, repo/common-dir/worktree/head/index/tracked/untracked state, prior effects, approval applicability and session support. Default preserve the old worktree and create a new reviewed continuation worktree/attempt. If provider session history cannot safely use that cwd, offer fresh context rather than guessing. Never use session “last” in a concurrent system. A resume command may replay model reasoning/tool actions; the continuation prompt includes completed/unknown effects and prohibited replays, but prompt instructions are not a correctness guarantee. Jojo must still constrain available effects and require observations/approval where needed.

Auth/rate-limit/permission failures create needs-user or blocked state; no automatic provider/account/API substitution. Transport retry resends only idempotent requests where supported, not Start/continuation or arbitrary tool commands. A remote side effect with unknown outcome blocks repetition even after local exit.

## Deterministic conformance requirements

Fake adapters for both identities expose the same contract with independently configurable capabilities. Test interleaved outputs from two providers, separate sessions, input correlation, unsupported interactive commands, terminal-success/nonzero-exit, exit-zero/no-terminal, interrupted turn/no-result, auth-unknown denial, truncated streams, stale generation, cancel-before-start, stop-before-success and remote-effect-unknown. Use synthetic fixtures or explicitly attributed upstream fixtures; preserve file/SHA/license/modification ledger for reused cases. No tests should need a provider login, subscription, API key, network or model response in Phase 1.
