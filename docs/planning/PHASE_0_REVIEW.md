# Jojo — Phase 0 architecture review

Status: documentation complete for human review; no implementation authorized. Date: 2026-11-14. The latest user requirements are authoritative. Stage 1 reports remain unchanged research/proposal records; the refinements in this Phase 0 set take precedence where those proposals conflict. No ADR draft is marked accepted.

## Inputs read and their location

All seven Stage 1 reports were located and read before choosing these recommendations. This is a refinement of the audited design, not a new upstream audit.

| Report | Review baseline SHA-256 |
|---|---|
| [OPEN_DECISIONS.md](../architecture/OPEN_DECISIONS.md) | `d01d0b7406516e4e8dcfa2c4ac360157efe4f3e38882afebebc669043a7ed733` |
| [PROPOSED_ARCHITECTURE.md](../architecture/PROPOSED_ARCHITECTURE.md) | `373f815f13c2acbc58b994ac70e451b9d97982ac29ad6ce1f0faf3060184a680` |
| [COMPONENT_EXTRACTION_MATRIX.md](../research/COMPONENT_EXTRACTION_MATRIX.md) | `298a1946d8a3e5a6aabff3d418273f137b830a9bd1cdf32c695fb2478bd4635c` |
| [LICENSING_AND_SECURITY.md](../research/LICENSING_AND_SECURITY.md) | `7c2ddc668bba3d21337340955ff0a37faf7f770e42096c481f8052a4065375cc` |
| [OPTIO_INTERNALS.md](../research/OPTIO_INTERNALS.md) | `9a76d276d517bfce4b188a98e9e0b803dea8610a9a182fad9a860628f7922768` |
| [REPOSITORY_AUDIT.md](../research/REPOSITORY_AUDIT.md) | `170a83441b69adc9d0e14b3d22418f0977cead9c9ea661d9e17588d2d34a600a` |
| [IMPLEMENTATION_ROADMAP.md](../roadmap/IMPLEMENTATION_ROADMAP.md) | `3bacddc8e174a3061f9aeac95579f7c569e7fe3b4d57466dc527ddf8a2146831` |

The repository currently contains reports, evidence inventories and upstream research clones, not a working Jojo core. “Supported” below means supported by the design, never an implemented or tested capability.

## Confirmed by the user

Native SwiftUI; Rust modular monolith in a separate local helper; SQLite authoritative state; personal single-user/local-first use; official user-installed Claude Code and Codex with native subscription authentication; no credential extraction/proxy, API-key fallback or automatic paid extra usage; human approval of PRD, ADR and execution plan; future parallel multi-agent execution; independent worktrees/attempts/results; explicit dependencies; no PR-open-only satisfaction; safe confirmed stop-on-close and validated continuation; no background-after-close requirement. Existing `agentic-factory` and upstream sources remain untouched. No remote creation, deployment or inference in this assignment.

These confirmations settle constraints previously offered as choices. They do not accept implementation details, prototype scope or all ten ADR drafts by implication.

## Critical review and required changes

| Finding | Stage 1 evidence | Phase 0 correction |
|---|---|---|
| Background lifetime is no longer an open product choice | D02, proposed IPC options and GUI-independent lifetime | Stop on confirmed quit. Freeze admissions before asking, cancel-close restores admission without cancelling agents. No resident daemon/LaunchAgent required. |
| Deferred concurrency could become a single-agent architecture | D20 and roadmap “advanced concurrency deferred”; “one provider at a time” | Default capacity may be one, but storage, reducer, reservations, dispatch and tests support multiple attempts from the first foundation. Both providers share the scheduler. |
| Graph concepts are good but admission is underspecified | Proposed two DAGs and resource leases | One transactional claim across revisions, all prerequisites and all resources; launch revalidation; no in-memory semaphore authority. |
| A whole-repository exclusive lock held for every run over-serializes independent work | Proposed repository mutation lease wording | Per-worktree exclusivity plus shared repository activity gate; short exclusive administration phases. Default per-repo capacity one is policy, not schema. Separate repositories have separate gates. |
| Lease expiry can be misread as proof of death | Recovery notes warn but do not formalize ownership | DB fencing rejects stale events; OS process absence and worktree validation separately release quarantine. Expiry never replaces an unknown writer. |
| Cancellation, task interruption and provider turn interruption are conflated | Broad start/resume/cancel interface, task-like state sequence | Durable intent, process lifecycle, immutable outcome and independent validation are separate. Shutdown holds steps and interrupts attempts; it does not permanently cancel the feature. |
| “Resume” lacks a safety contract | Session IDs/CLI resume mentioned | Explicit exact session ID only after repository/process/effect inspection; new attempt identity; fresh-context alternative. No `--last`, blind replay or claim of restored memory. |
| Success criteria may reject legitimate cancelled runs or accept weak successes | General terminal-event-plus-exit rule | Terminal success required for success; missing terminal output is expected after some stop paths. Known termination records cancellation/interruption, never success. |
| GUI close has a dispatch race | UI close behavior not specified atomically | Helper-issued close ticket freezes admission, handles queued/starting/unknown work, and revalidates at confirmed shutdown. |
| Process group termination is not universal descendant containment | Optio process/pod distinction | Track identity/group/known descendants; expose unconfirmed termination, including escaped descendants. No success from signal delivery alone. |
| Full event sourcing/daemon/plugin extraction would add complexity | Broad event/log/outbox proposals; XPC and per-run supervisor alternatives | Mutable revisioned projections plus append-only facts/results in SQLite. No full event-sourced rebuild system, distributed workflow engine, plugin platform or always-on supervisor. |
| Same-user processes can reach approval authority | Security report acknowledges trusted-host limitation | Stdio control handle not inherited by agents; model output cannot approve. Do not market this as hostile-code isolation. Repository hooks/config are executable trust inputs. |
| DB, log files and remote effects are not atomic together | Existing outbox/checkpoint caution | Record effect states and launch gaps explicitly; durable log offsets cannot outrun flushed bytes. Unknown effects block reuse; no exactly-once guarantee. |
| Early recovery cannot be left until packaging hardening | Roadmap Phase 7 recovery hardening | Phase 1 must prove shutdown, orphan quarantine, result immutability and fake concurrency before live providers. Advanced transparent reattachment remains later. |

New failure modes covered: cap reductions while jobs run; cancellation during reservation/spawn; late success after stop intent; duplicate commands; stale callbacks from old engine epochs; approval changes during execution; symlink/alias repository collisions; full disk on finalization; truncated NDJSON/UTF-8; lost IPC during close; GUI/helper SIGKILL; crash after side effect before acknowledgement; PID reuse; Mac sleep/clock changes; missing session history; output flood blocking control; child CLI exit while tool descendants remain.

Provider documentation checked without running CLIs: [Claude programmatic execution](https://code.claude.com/docs/en/headless), [Codex non-interactive interface](https://developers.openai.com/codex/noninteractive), [Codex app-server](https://developers.openai.com/codex/app-server). These support different protocol/termination capabilities, not one universal resumability promise. Provider policy qualifications and license findings remain as documented in Stage 1; this review does not grant new authentication rights.

## Deliverables and authority

| Document | Owns |
|---|---|
| [ADR-DRAFTS.md](../architecture/ADR-DRAFTS.md) | D01–D10 recommendations, confirmation boundary and unresolved approval items U01–U08 |
| [CORE_STATE_MODEL.md](../architecture/CORE_STATE_MODEL.md) | Domain identities, revisions, approvals, attempts, results and persistence invariants |
| [PARALLEL_SCHEDULER.md](../architecture/PARALLEL_SCHEDULER.md) | Resource types, fairness, transactional admission, cancellation and crash rules |
| [PROVIDER_CONTRACTS.md](../architecture/PROVIDER_CONTRACTS.md) | Provider-neutral capabilities/events and official CLI boundary |
| [SHUTDOWN_RECOVERY.md](../architecture/SHUTDOWN_RECOVERY.md) | IPC, close state machine, stop acknowledgement and reopening/continuation |
| [PHASE_1_IMPLEMENTATION_BRIEF.md](PHASE_1_IMPLEMENTATION_BRIEF.md) | Bounded foundation backlog, prerequisites, acceptance and deterministic test oracle |

No Stage 1 extraction decision is silently expanded: C01/C03/C08 remain selective PORT candidates with source/fixture attribution; storage/scheduler/process/IPC remain REWRITE; official tool engines remain EXTERNAL TOOL; credential relay, permission-bypass defaults and hosted infrastructure remain DISCARD. No source copying is needed to approve this foundation. Retain original file/commit/license/modification provenance for any later adaptation.

## Readiness verdict

Phase 1 is ready to be reviewed and approved as a **foundation implemented against fake providers**, not approved to execute now. U01–U05 require a decision before implementation scope is considered approved; U06–U08 block later live-provider or packaging stages, not the deterministic foundation. No application PRD or execution plan is treated as accepted merely because this brief exists.

Recommended next implementation step after explicit approval: P1-01, create the minimal Rust domain/helper workspace and deterministic test harness, preserving these reports and research clones. Then implement SQLite revisions/approval invariants before worktree/process dispatch. Do not start with a real Claude/Codex coding run or GUI polish.

## Validation of this documentation

Review checks: seven Stage 1 inputs read; D01–D10 mapped; confirmed decisions distinguished; all requested failure paths and task prerequisites specified; local links and input hashes checked. Deterministic tests listed in the brief are future implementation acceptance requirements; they have not been executed. No production code, provider launch, credential access, remote repository or deployment was created in Phase 0.
