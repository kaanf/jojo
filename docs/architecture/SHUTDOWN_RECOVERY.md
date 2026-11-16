# Jojo — SwiftUI/helper lifecycle, shutdown and recovery

Status: proposed mechanics implementing the user's **confirmed** stop-on-close requirements. No background execution after GUI closure is required. See [ADR D02/D09](ADR-DRAFTS.md), [core states](CORE_STATE_MODEL.md), [parallel claims](PARALLEL_SCHEDULER.md).

## Lifecycle and prototype transport

SwiftUI remains the native GUI, with an AppKit application delegate/close coordinator for macOS termination. It launches one Rust helper over private stdio pipes. Helper stdout contains only bounded protocol envelopes; stderr diagnostics and attempt logs are separate. Helper owns SQLite, concurrent workers and their processes. A GUI task cancellation is not enough to terminate a helper/provider.

Envelope fields: protocol major/minor, client session ID, helper engine epoch, request/idempotency ID, expected aggregate/config revision, type and bounded payload. Responses return committed revision/event sequence or typed error. Commands, queries and events are distinct. Duplicate mutating request IDs return stored receipts; same ID/different payload conflicts. Unknown protocol major disallows mutations. Logging never shares protocol stdout accidentally.

Startup sequence: acquire OS singleton lock → open/migrate DB safely → persist engine epoch with admission closed → handshake capabilities/protocol → inspect recovery → send consistent snapshot with `as_of_event_seq` → stream committed events after that cursor. UI applies each sequence once and refreshes snapshot on a gap. A snapshot and its cursor must come from one consistent read; subscribing before/after it must not lose intervening events. Artifact/log queries are paginated so bulk output cannot starve stop/approval traffic. Disconnect does not mean requests failed: after reopening query command/shutdown receipts instead of reissuing actions blindly.

There is no Phase 1 reconnection to a live helper using lost stdio handles. A replacement UI first checks for an existing helper; never launches a second writer and assumes it owns old children. An old live helper is shown as unavailable/busy while its EOF-triggered stop completes; failure requires inspection, not duplicate launch. Proposed future Unix socket transport would add owned socket/peer validation, authenticated session handshake, takeover restrictions, cursor replay and pending-question replay. It must preserve the same command IDs, epochs and lifecycle rules. No TCP listener, resident LaunchAgent, XPC shim or per-run daemon is required for the prototype.

The UI may inspect owned singleton-owner metadata and OS process identity to show that busy state; it still does not read/write SQLite or signal an unverified PID. If emergency stopping remains unconfirmed after the old UI died, reopening must report the blocked helper and recovery location, not promise reconnection or launch another writer. This limitation is a reason to consider a private socket later, not to introduce background execution now.

## macOS close entry points

All Cmd-Q/menu Quit and last-primary-window close paths go through one coordinator; last-window→quit is the U02 proposed default. Closing auxiliary inspectors need not quit Jojo. Repeated quit requests share one operation; do not stack confirmation dialogs. Window minimization/hiding is not closure.

Before PrepareClose, settle pending draft writes and reconcile outstanding command receipts; unsent UI text is not preserved merely because helper state is durable. A draft-save/receipt failure stays visible and prevents a false preservation claim. Do not resend an uncertain approval or Start command with a new ID while quitting.

Use the application termination decision hook to defer quitting and eventually reply after asynchronous shutdown. Do not begin this in `applicationWillTerminate`, which is too late and is not guaranteed for sudden termination. Apple documents [termination deferral](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationshouldterminate%28_%3A%29?language=objc%2Cobjc) and [the subsequent reply](https://developer.apple.com/documentation/appkit/nsapplication/reply%28toapplicationshouldterminate%3A%29?changes=latest_major&language=o_8). This AppKit lifecycle hook does not replace SwiftUI views or imply a guaranteed OS shutdown deadline.

## What counts as active work

Active means an in-flight operation can still execute/write or its process stop is unconfirmed: reserved/starting launch dispatch, running/awaiting-input provider, test/Git worker, live descendants, settling I/O, or unknown-live execution. Pending unissued queue items alone are not active; they remain preserved for reopening. A retained worktree or known-dead historical interrupted attempt alone is not active. An unresolved remote effect with no local live process is a recovery issue to preserve, not proof a local process must keep running; no such remote adapter exists in Phase 1.

The helper is the authority on active work. UI counters are never used to decide it is safe to terminate. Close admission freeze prevents a queued job becoming active after the no-active check.

## Close protocol and state machine

| State | Trigger | Action / next state |
|---|---|---|
| OPEN | PrepareClose(request ID) | Atomically set CLOSE_CHECK admission gate, create ticket and active snapshot. Running jobs continue; no new launch may cross issue boundary. |
| CLOSE_CHECK | No active work | Proceed to FINALIZING without a user confirmation; queued work preserved. |
| CLOSE_CHECK | Active/potentially live work | Return ticket + CONFIRM_REQUIRED and affected IDs/revisions; UI shows “Stop active work and close?” with Cancel/Stop and Close. Enter AWAITING_CONFIRMATION. |
| AWAITING_CONFIRMATION | CancelClose(ticket) | Remove only this close gate; leave user pauses/cancels intact; return OPEN. No stop has been sent and running work continues. |
| AWAITING_CONFIRMATION | ConfirmClose(ticket) | Persist shutdown ID/target set and interrupt-for-shutdown intent; gate becomes STOPPING. No launches/automatic retries. |
| STOPPING | All local processes/descendants gone and workers settled | Drain/seal evidence and journal partial states; FINALIZING. |
| STOPPING / FINALIZING | Stop, inspection, durability or helper error | SHUTDOWN_FAILED; show exact uncertainty/failure, retain claims and admission hold. Do not send clean acknowledgement. |
| SHUTDOWN_FAILED | Retry stop / approved escalation | Continue STOPPING for unresolved targets only; operations idempotent. |
| SHUTDOWN_FAILED | Stay in Jojo | Cancel OS termination, keep affected work interrupted/stopping/held; never automatically restart jobs already signalled. |
| FINALIZING | Durable closure checks pass | Persist clean shutdown receipt; emit ShutdownReady(ticket, shutdown ID, event sequence, outcome summary); exit helper after channel flush. |
| Ready + helper exit observed | UI receives valid receipt and verifies helper exit | Complete OS termination; no retained background helper by design. |

PrepareClose and ConfirmClose are distinct; confirmation is never inferred from elapsed time or a preselected button. Tickets bind the helper epoch and close operation. Confirm rechecks current targets and freeze state; already-finished jobs remain finished, starting/unknown targets cannot vanish from tracking. No-active close must use the same finalization checks, but must not show an active-work confirmation.

The CLOSE_CHECK gate is a layer over RUN/HOLD intent, not a permanent task state change. CancelClose restores only that layer. User commands during confirmation may pause/cancel work but cannot start new work until ticket resolution. If all work finishes while the dialog is open, the UI can update the count; it must not interpret that as the user's confirmation. Existing paused/cancelled intent is not undone by cancelling close.

After ConfirmClose, a request to stay open cannot undo signals or restore terminated memory. Leave work held and offer recovery/continuation when verified. ConfirmClose records interruption for shutdown, not permanent feature/task cancellation.

The durable closure receipt distinguishes `process_stop_confirmed`, `persistence_sealed` and remaining recovery item IDs. A clean local shutdown can preserve known-dead interrupted work or an unknown remote-effect receipt; it does not mean every task succeeded or every remote operation was cancelled. Unconfirmed local writers or failed persistence never satisfy clean shutdown.

## Stopping concurrent process trees

1. Commit admission stop and target identities before sending supported interrupts. Control handling outranks retry backoff and provider cooldown. Mark each attempt STOP_REQUESTED with shutdown reason/sequence.
2. Deliver capability-specific graceful interrupt if supported; keep stdout/stderr draining concurrently. Suggested budget is 10 seconds, then TERM with 20 additional seconds. Timers are injected policy inputs. Awaiting a provider permission response does not excuse indefinite shutdown.
3. If graceful termination is unconfirmed, expose its failure. Under approved U05 escalation, issue KILL only to verified owned identities/groups/known descendants; allow 5 seconds for verification. Always record forced termination. “Graceful stop failed; forced stop confirmed” is distinct from graceful success.
4. Verify root exit and the relevant owned descendants, including tools still alive after CLI exit. Record boot ID, PID/start identity, PGID, executable/cwd facts and inspection errors. Successful signal delivery or `kill(pid,0)` alone is insufficient. Reused PID is not ours to kill. New children discovered during stopping join the target set.
5. If any owned/potentially owned writer cannot be confirmed stopped, retain quarantine and report failure. Process groups are not complete hostile-code containment; escaped/uncertain descendants must not be silently ignored. Do not kill all `claude`/`codex` processes by name or touch another terminal's agents.
6. Classify/seal the attempt only once actual local process absence is established. Missing provider terminal result after stop yields partial interruption evidence, never fabricated success. Candidate code/logs stay available.

These actions may proceed in parallel per target with bounded control workers; do not stop one agent serially while all others wait unboundedly. No scheduler/provider slot is released merely because a stop request was issued. Graceful interrupt acknowledgement means a turn was asked to stop, not that every descendant stopped. Some remote tools can outlive their CLI; record unknown remote effects and block replay rather than claiming a local kill cancelled them.

If log spooling, blob flush or DB commit fails, attempt best-effort stop but do not promise preservation beyond the last committed/flushed boundary. Show disk/evidence failure and leave the close unclean. Unknown status after helper death cannot be turned into ShutdownReady by the GUI. OS force quit, power failure and system logout deadlines may bypass confirmation/cleanup; reopening must handle them as unclean recovery, not guaranteed graceful shutdown.

## Reopening and validated continuation

Every startup starts paused with admission closed until recovery inspection and a user Start/Continue action. Display all interrupted, failed-stop and issued-unknown attempts, their worktree paths, last durable logs/evidence and precise blockers. Queued jobs are preserved; do not automatically replay last session's Start commands.

Recovery inspection must validate:

| Subject | Required check |
|---|---|
| Helper ownership | Old helper singleton identity/lock; no second writer if old helper lives |
| Process | Boot/process-start identity, executable/cwd/group/known descendants; distinguish confirmed absent, matching live, reused PID and unknown inspection |
| Repository | Canonical path/Git common-dir identity, ownership markers, branch/HEAD/base, repository identity changes/aliases |
| Worktree snapshot | Index/staged and unstaged diff, untracked paths, symlink targets without following outside roots, submodule pointers; ignored/secret files are not copied automatically |
| Inputs | Current PRD/ADR/plan/permissions/source bindings and validation plan; stale approval blocks execution |
| Effects | Launch issue journal and any possible Git/SCM/deploy operation; inspect stable target IDs/digests, never infer “not done” from missing local acknowledgement |
| Evidence | Last durable byte offsets, partial trailing records, hashes, absent/truncated files and sealed result identity |
| Provider continuation | Exact recorded session and supported resume/cwd semantics; no `--last`, credential/session-cache scraping or inference probe |

A matching live orphan is stopped and inspected before continuation in the prototype; no transparent stream takeover is promised. Unknown identity or inaccessible repo remains blocked with resource holds. Expired lease alone is not absence. If no reliable process identity was recorded in the launch gap, quarantine the known repo/worktree/slot and require enough OS/filesystem evidence to resolve it; a human checkbox “assume stopped” is not a safe replacement.

Offer reviewed choices only when their preconditions hold:

- **Validate existing output:** no new inference; run an approved validation against a frozen candidate snapshot. This can accept useful code from an interrupted run without changing its immutable result.
- **Continue with supported history:** create a new attempt and new owned worktree from the reviewed snapshot; reference exact prior session only if official mode safely supports that cwd and current inputs. State plainly that this reuses history, not terminated memory.
- **Continue in fresh context:** new attempt with approved PRD/ADR/plan, candidate diff and completed/unknown-effect summary. Unknown effects must first be reconciled or excluded by the approved action scope.
- **Keep on hold / cancel task:** retain existing state/worktree/evidence; no implicit clean/reset/delete.

A continuation plan records predecessor result, snapshot/source vector, intended remaining work, reconciled effect IDs, current approval applicability and chosen session policy. If remaining work changes the accepted plan materially, obtain new plan approval. Forking dirty snapshots must handle staged/untracked/modes/symlinks safely and report omissions; if faithful materialization cannot be established, block rather than quietly losing changes. In-place reuse is not the default. Never blindly retry PR creation, a migration, a deployment or an already-committed operation because a provider resumed.

## Failure/race acceptance table

| Case | Required behavior |
|---|---|
| Idle close with queued-only work | No confirmation, no new dispatch, durable queue retained |
| Active close cancelled | Existing jobs continue, no signals sent, fair queue state preserved |
| Active close confirmed with mixed providers | Every target stopped/verified or explicitly failed; no detached background job claimed as clean |
| Job completes during confirmation | Preserve sealed success, recheck other targets; no new launch |
| Stop intent races success finalization | Writer-order rules from core model; one immutable result, preserved candidate output |
| Close races reservation/spawn | Admission gate plus issued-unknown tracking; no untracked replacement |
| Graceful timeout then confirmed KILL | Visible graceful failure/escalation receipt; interruption, not success |
| KILL/inspection fails or descendant escapes | SHUTDOWN_FAILED, UI stays available, resources quarantined |
| GUI crashes/stdio EOF | Helper closes admission and starts emergency stop; no confirmation possible, mark unclean session |
| Helper crashes/IPC breaks | UI shows unknown, never assumes stopped; reopen inspection/OS evidence before new helper/admission |
| Logs/DB disk full | Best-effort stop, no clean preservation claim, actionable failure |
| Mac wakes/reboots or PID reused | Reobserve, do not signal unrelated PID or steal resources |
| Exact session missing or cwd mismatch | Offer fresh-context/hold, never automatic fallback execution |

These are future deterministic implementation tests, not capabilities already tested on this Mac. The [Phase 1 brief](../planning/PHASE_1_IMPLEMENTATION_BRIEF.md) makes them release gates before live-provider execution.
