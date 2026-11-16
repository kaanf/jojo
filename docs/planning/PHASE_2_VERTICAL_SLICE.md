# Phase 2 plan — first real Codex execution (vertical slice)

Status: **PROPOSED, not approved.** Planning only, written 2026-11-15. No production code was changed, no Git repository was initialized, nothing was pushed or merged, and Codex was not executed. Passing Phase 1 gates are evidence, not human approval; every authorization in section 7 is still open.

Update 2026-11-15: the human accepted the Phase 1 packet as the baseline, decided B3–B5 as recommended and authorized **P2-01 only**. P2-01 is implemented and reported in [P2-01 verification](../verification/P2-01.md). P2-02 was then authorized separately and is implemented and reported in [P2-02 verification](../verification/P2-02.md). P2-03 ([verification](../verification/P2-03.md)) and P2-04 ([verification](../verification/P2-04.md), [operator checklist](P2_05_OPERATOR_CHECKLIST.md)) were each authorized separately and are implemented and reported, as is P2-04.1 (orphan process safety, [verification](../verification/P2-04-1.md)). P2-05 and every G-LIVE authorization remain open. The text below is the plan as proposed and is otherwise unchanged.

Inputs read: [Phase 1 readiness packet](../verification/PHASE_1_READINESS.md), [P1-17 report](../verification/P1-17.md), `verification/p1-17-result.json`, [stdio protocol](../architecture/STDIO_PROTOCOL.md), [provider contracts](../architecture/PROVIDER_CONTRACTS.md), [open decisions](../architecture/OPEN_DECISIONS.md), [accepted decisions](../architecture/ACCEPTED-DECISIONS-2026-11-14.md), [ADR drafts](../architecture/ADR-DRAFTS.md) (U06–U08), [roadmap](../roadmap/IMPLEMENTATION_ROADMAP.md) Phase 2, and the relevant sections of `crates/` and `macos/JojoApp/Sources`. The Phase 1 test suite was not re-run for this plan.

## 1. Phase 1 readiness assessment

| Check | Finding |
|---|---|
| P1-17 gate | `verification/p1-17-result.json`: `passed: true`, `human_readiness_review: GO`, 339 Rust / 55 boundary / 27 Swift tests, 9 GUI scenarios, 0 failures. `phase_2: NOT APPROVED`. |
| Human approval of Phase 1 | **Not recorded.** The packet (section 10) requests it; no acceptance record exists after `ACCEPTED-DECISIONS-2026-11-14.md`. |
| Accepted decisions | U01–U05 only. |
| Unresolved | U06 (real CLI version/permission/auth/billing-mode assurance), U07 (first provider and sample repository), U08 (live SCM/Figma/deployment/distribution/strong isolation). D01–D10 remain drafts except the clauses U01–U05 accepted. |
| What Phase 1 proves | Deterministic contracts with fixture processes, offline. It proves nothing about real Codex streams, sessions, auth or version drift. |

Phase 1 is a sound base for this slice. The work it leaves is mostly **wiring**: the engine has the pieces, but no production path connects them.

## 2. Blocking decisions

Needed before any implementation (gate G0):

| ID | Decision | Recommendation |
|---|---|---|
| B1 | Approve the Phase 1 readiness packet | Approve, or return findings. |
| B2 | Authorize Phase 2 implementation, milestone by milestone | Authorize P2-01 only; each later milestone needs its own GO. |
| B3 | U07: first provider and sample | Codex first; one generated disposable repository (section 6). |
| B4 | D04, D07, D08 clauses this slice relies on | Accept: unmodified user-installed CLI with native login; trusted repo, owned worktree, one coding run, no permission bypass; one sample feature with a deterministic test. |
| B5 | Launch-envelope amendment (finding F3) | Allow exactly one non-secret path variable (`CODEX_HOME` or `HOME`) in the environment allowlist for the Codex provider, and record provider inference traffic as a separate, explicit permission from agent tool network access (which stays denied). |

Needed before the live attempt only (gate G-LIVE, section 7): U06, Codex home choice, usage limit, and the single-attempt authorization.

Deferred, not needed for this slice: U08, D10 (no remote exists), D11–D20, the Claude Design migration, Claude Code parity, packaging.

## 3. Installed Codex CLI — observed facts

Collected with `--version` and `--help` only. No login, status, doctor, network or inference command was run, and no file under `~/.codex` was read.

- **F1. Version:** `codex-cli 0.169.0`.
- **F2. Executable:** `/opt/homebrew/bin/codex` is a symlink to a Node launcher script (`#!/usr/bin/env node`). The supervised runner spawns with a cleared environment (`crates/jojo-storage/src/process.rs:723`), so that launcher cannot resolve `node`. The native binary is a Mach-O arm64 file at `/opt/homebrew/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex`. The adapter should be configured with that explicit path and bind version plus file digest in the fingerprint. An npm update changes the bytes, which must invalidate the fingerprint.
- **F3. Envelope conflict:** `LaunchSpec::validate` (`crates/jojo-domain/src/provider.rs:137`) rejects any spec with `permissions.network == true` and any environment name other than `LANG`, `LC_ALL`, `TZ`. A real Codex process needs to locate its own login state and reach its provider. As written, the contract cannot launch a signed-in Codex. This is decision B5, not an engineering detail.
- **F4. Non-interactive mode:** `codex exec [PROMPT]`; the prompt is read from stdin when omitted or `-`. This matches the runner, which already pipes `LaunchSpec.prompt` to stdin.
- **F5. Relevant `exec` options present:** `--json` (JSONL events on stdout), `-s/--sandbox read-only|workspace-write|danger-full-access`, `-C/--cd`, `--ephemeral` (no session files), `--ignore-user-config` (auth still uses `CODEX_HOME`), `--ignore-rules`, `--output-schema`, `-o/--output-last-message`, `--color`, `--skip-git-repo-check`, `-m/--model`, `-p/--profile`.
- **F6. Options that must never be passed:** `--dangerously-bypass-approvals-and-sandbox`, `--dangerously-bypass-hook-trust`, `--approve-for-me`, `--trust` (persists trust), `--worktree` (Jojo owns worktrees), `--add-dir`, `--oss`, `--local-provider`, `--enable/--disable`, arbitrary `-c`.
- **F7. Sessions:** `codex exec resume [SESSION_ID]` and `exec fork` exist. `--last` selects implicitly and must not be used.
- **F8. Auth surface:** `codex login status` exists; `codex login --with-api-key` and `--with-access-token` exist and are out of bounds. `codex doctor` performs network checks and is not a planning-time tool.
- **F9. User state exists:** `~/.codex` contains `config.toml`, `hooks.json`, `rules`, `plugins` and session stores (names listed only). Unisolated, these could change the behaviour of a run.

Not verifiable from help text, so **UNKNOWN** until P2-01 fixtures and the live attempt:

- The exact JSONL event shapes of 0.169.0. The existing parser is `jojo.codex.exec-json.synthetic/1` and models documented shapes only.
- Approval behaviour in `exec`: there is no `--ask-for-approval` flag in `exec --help`.
- Whether hooks, plugins or MCP servers load under `--ignore-user-config`.
- Whether the `workspace-write` sandbox blocks writes to a worktree's Git common directory, and what it allows outside the working root.
- Where login state is stored (file or Keychain) and which billing mode is active.

Proposed argv, to be confirmed in P2-01 and not executed here:

```
<native codex> exec --json --color never --sandbox workspace-write --ephemeral
    --ignore-user-config --ignore-rules -C <owned worktree>      (prompt on stdin)
```

## 4. The vertical slice and its gaps

Legend: **PROD** implemented in production code; **TEST** exercised only through test harnesses or the storage API; **MISSING**; **BLOCKED** by a decision or authorization.

| # | Step | Status | Evidence and gap |
|---|---|---|---|
| 1 | Open an isolated, disposable Git repository | TEST + BLOCKED | `register_local_repository` (`worktrees.rs:245`) is called only from tests. No protocol command registers a repository. Creating the sample repository needs B3. |
| 2 | Create a small deterministic task | TEST | PRD/ADR/plan authoring exists as storage API (`artifacts.rs`, `tests/common/scenario.rs:225–290`). No production import path. |
| 3 | Approved scope and execution plan | PROD (decision) / TEST (inputs) | `HumanDecision` and `ReviewBundle` are production commands bound to displayed bytes. The artifacts they would approve cannot be created in production (step 2). Write scope lives in `ValidationPlan.write_scope` (`validation.rs:59`). |
| 4 | Start real Codex through the supervised runner | MISSING + BLOCKED | `Service::launch` (`service.rs:154`) has no production caller; protocol 1.0 has no Start. No reservation → worktree → bind → launch sequence exists outside tests. No Codex adapter: argv translation exists only for the fixture. Blocked by B5 and G-LIVE. |
| 5 | Codex modifies only approved files | PROD (detective), not preventive | Owned worktree and `issue_process_launch` checks are production. Scope is enforced **after** the run by `check_scope`; a violation fails validation, it is not prevented. Preventive limits come only from Codex's own sandbox (unverified, F-unknowns). |
| 6 | Capture output and process result | PROD, parser TEST-shaped | Spools, bounded frames, process group, exit observation and `settle_attempt` are production. The Codex profile is synthetic and maps unknown event kinds to `Informational`. |
| 7 | Independently inspect the Git diff | PROD (engine) / MISSING (display) | `collect_candidate` captures staged, unstaged and untracked content twice; `inspect_candidate_changes` derives paths from the repository, never from the provider. Nothing exposes the diff over the protocol or in the GUI. |
| 8 | Run a deterministic validation command | MISSING | `validate_candidate` records a `ValidatorReport` "supplied by the host's validation runner". No such runner exists in production; tests construct the report by hand. |
| 9 | Record evidence and settle | PROD / TEST | Settlement, blobs and receipts are production. `record_validation` and `accept_candidate` are reachable only through the storage API. |
| 10 | Display through the macOS app | PARTIAL | The app shows connection, admission, run rows, review hashes and recovery subjects, and can cancel and decide. It uses `Snapshot`, `Events`, `ReviewBundle`, `Recovery`, `HumanDecision`, `CancelAttempt`, `Receipt`. It has no Start, no log or event-detail view, no diff, no validation or acceptance view. |

Focus areas:

- **Start and recovery-resolution commands.** Both missing (readiness limitation 1). Recovery resolution is a prerequisite for the live attempt: a crash mid-run would otherwise leave a hold that cannot be resolved without test code.
- **Provider session handling.** The normalizer extracts `thread_id` and `SessionRef`/`Continuation` DTOs exist. The slice should run one fresh, ephemeral session, record the thread ID as an untrusted observation, and reject continuation. `exec resume` stays out of scope.
- **External-action idempotency.** `journal_attempt_effect` / `issue_attempt_effect` give a unique key and an `ISSUED_UNKNOWN` state that is never re-dispatched (`outcomes.rs:707–748`), and Start is specified as "no retry of issued-unknown". The slice has no remote effect. The one external action is the Codex launch itself, which must go through the existing issue journal so a lost acknowledgement never causes a second inference run.
- **Candidate publication.** A candidate is a sealed snapshot file plus a database reference (`candidate_reference`). There is no publish, branch or merge step, and none is needed here. Phase 2 should end at human acceptance of a validated candidate; applying it to the source repository stays a manual, human action.
- **Process ownership and shutdown.** Production: process group, descendant discovery, identity-verified signals, TERM → KILL budgets, emergency stop on EOF, 45-second stalled-worker report. Untested against Codex's real process tree (sandbox helper, shells). `exec` offers no turn-interrupt channel, so the stop plan is signal-only.
- **Approval requirements.** Product approvals (PRD, ADR, plan) are production and revision-bound. Tool-permission approval is a different thing: one-shot `exec` cannot route it to a human, so the slice must run in a mode where a denied action ends the turn rather than being auto-approved.
- **GUI limitations.** Unbundled SwiftPM executable; see step 10.

## 5. Proposed Phase 2 milestones

Each milestone ends at a gate script in the style of Phase 1 (`scripts/verify_p2_NN.py`, result under `verification/`), runs offline under OS network denial except P2-05, and requires an explicit human GO to begin the next one. Invariants that hold throughout: Swift never writes the database or signals processes; no credential, Keychain or `auth.json` read by Jojo; no API-key path; no remote; no name-based kill; unknown outcomes fail closed; the Phase 1 suite stays green and unmodified except where a milestone names a change.

### P2-01 — Codex adapter and launch envelope, offline

- **Objective:** turn a frozen `LaunchSpec` into a validated Codex `ProcessLaunch`, and parse Codex output conservatively, without launching Codex.
- **Scope:** production adapter module (argv from section 3 with a fixed allowlist; F6 options unrepresentable); executable by explicit configured path with version and file-digest fingerprint; the B5 envelope amendment in `LaunchSpec::validate` scoped to the Codex provider; a versioned parser profile distinct from `synthetic/1`; capability record with every unverified item set to `UNKNOWN`.
- **Acceptance:** generated argv is byte-exact for a golden spec; a symlink, script launcher, wrong digest, wrong version or missing file is rejected before the issue journal; any inherited `OPENAI_*`, `CODEX_*` token or routing variable is absent from the child environment; continuation specs are rejected.
- **Tests:** golden argv; fixture executable emitting split, malformed and oversized JSONL, `turn.failed`, auth-error text, exit 0 without a terminal record, unknown event kinds and stderr flood; envelope negative cases; all Phase 1 tests.
- **Safety invariants:** no code path can execute the real binary (gate scan plus launch ledger); agent tool network permission remains false.
- **Dependencies:** G0 (B1–B5).
- **GO/NO-GO:** GO when the gate passes and the launch ledger shows only fixture executables. NO-GO if the amendment weakens any existing validation for the Claude profile or the fixture.

### P2-02 — Production orchestration: stdio 1.1 Start and recovery resolution

- **Objective:** make the slice drivable through the helper protocol instead of test code.
- **Scope:** minor-version protocol additions only: register an explicitly named local repository (restricted to a configured disposable root); import one feature bundle (PRD, ADR, plan, validation plan) as unapproved drafts; `Start` for one approved plan step, performing reservation → owned worktree → bind → adapter → existing `Service::launch`; recovery-resolution commands mapping to the existing P1-11 reviewed choices. Minimal SwiftUI controls for Start and recovery.
- **Acceptance:** Start is refused with admission closed, stale or missing approvals, a repository outside the disposable root, or an unresolved recovery hold; a replayed Start with the same command identity never launches twice; a lost Start response is reconciled by `Receipt`; capacity stays one.
- **Tests:** protocol tests for each refusal; crash between Start commit and spawn; duplicate and replayed commands; recovery resolution after a forced kill; Swift tests and GUI scenarios for the new controls; the nine existing GUI scenarios unchanged.
- **Safety invariants:** no arbitrary executable or argv over IPC; importing a bundle never approves it; the helper never starts work without a human Start.
- **Dependencies:** P2-01.
- **GO/NO-GO:** GO when the T15 scenario can be reproduced with the fixture provider using protocol commands only.

### P2-03 — Host validation runner and result review

- **Objective:** run the approved deterministic test command against the sealed candidate and let a human inspect and accept the result.
- **Scope:** a validation runner that materializes base plus candidate in a clean owned directory, runs only the command from the approved validation plan through the supervised process runner with no network and a cleared environment, and builds the `ValidatorReport`; protocol commands for validate, bounded diff and changed-path queries, and candidate acceptance; GUI views for run log, changed paths, scope verdict, validation receipt and Accept.
- **Acceptance:** a change outside `write_scope` or to a baseline path yields a failed scope verdict and cannot be accepted; a provider claim of "tests passed" has no effect; acceptance rechecks approval freshness; the developer worktree is never the validation directory.
- **Tests:** in-scope pass; out-of-scope write; baseline (test file) edit; failing test; hung validation command stopped by budget; candidate tampered after sealing.
- **Safety invariants:** the validator executes only human-approved command bytes; validation never mutates the source repository or the candidate.
- **Dependencies:** P2-02.
- **GO/NO-GO:** GO when all negative cases fail closed with visible reasons in the GUI.

### P2-04 — Offline dress rehearsal

- **Objective:** execute all ten steps of section 4 through the app and production commands, with a fake Codex executable that emits recorded-shape JSONL and edits the sample file.
- **Scope:** the sample-repository generator script and bundle (section 6); a rehearsal gate; an operator checklist for the live attempt. No new product features.
- **Acceptance:** happy path ends in an accepted candidate; and each of these ends safely and visibly: out-of-scope write, `turn.failed`, auth error, kill mid-run, app quit mid-run, helper crash mid-run followed by GUI recovery resolution.
- **Tests:** the rehearsal scenarios as GUI/runtime scenarios; orphan check after each (no process from the attempt's tree remains).
- **Safety invariants:** the real Codex binary is still never executed; the rehearsal repository lives only under the disposable root.
- **Dependencies:** P2-03.
- **GO/NO-GO:** GO for requesting G-LIVE only if every stop condition from readiness packet section 9.5 was exercised and observed.

### P2-05 — One authorized live Codex attempt

- **Objective:** one real Codex run on the sample task, with evidence preserved for human inspection.
- **Scope:** no code changes other than fixes found by the attempt, each returning to the relevant offline gate. Human-performed preflight, one Start, validation, acceptance or rejection, and an updated readiness packet.
- **Acceptance:** section 6.
- **Tests:** the attempt itself; afterwards, the real JSONL is reduced to sanitized fixtures and added to the P2-01 parser tests.
- **Safety invariants:** stop conditions of section 7 halt the work immediately; no second attempt without a new authorization.
- **Dependencies:** P2-04 and G-LIVE.
- **GO/NO-GO:** this milestone is itself the gate for any further live work.

Out of scope for Phase 2: Claude Code, session continuation, parallelism above one, remotes and PRs, packaging and signing, the Claude Design migration, hostile-code isolation.

## 6. First real Codex execution — acceptance criteria

Sample (recommended default; the human may substitute under B3): a generated repository under a dedicated disposable root outside every existing project, with one commit containing `slugify.py` (stub) and `test_slugify.py` (failing tests). Task: implement `slugify()` so the tests pass. Write scope: `slugify.py`. Baseline (must not change): `test_slugify.py`. Validation command: `/usr/bin/python3 -m unittest -q`, offline.

The attempt passes only if all of the following are recorded as evidence:

1. The executable path, version and digest match the approved fingerprint; argv equals the approved argv.
2. The run was started by one human Start in the app, against plan, PRD and ADR revisions approved in the app.
3. Exactly one Codex process tree was launched; after exit, no process from it remains.
4. Raw stdout and stderr spools are stored as verified blobs; the stream ended with a terminal record, or the outcome is recorded as unknown.
5. The repository-derived diff touches only `slugify.py`; the source repository's branch, index and working tree are unchanged; no file outside the owned worktree and Codex's own home changed as far as Jojo can observe.
6. The validation command ran in a clean directory on the sealed candidate and produced a receipt bound to the candidate digest.
7. The result, changed paths, validation receipt and acceptance decision are visible in the macOS app.
8. Jojo read no credential, relayed no authentication and made no network connection of its own.

A failed, interrupted or unknown outcome with complete evidence is an acceptable result of the first attempt. Treating such an outcome as success is not.

## 7. Human authorization boundaries

Nothing below is implied by this plan, by passing gates, or by approval of an earlier item.

| Boundary | Who | When |
|---|---|---|
| Approve Phase 1 packet; decisions B2–B5 | Human | G0, before P2-01 |
| GO for each milestone P2-02 … P2-04 | Human | After the previous gate |
| Initialize Git in the Jojo project, push, merge | Human | Not requested by this plan |
| Create the disposable sample repository | Human authorizes the generator and its root | P2-04 |
| U06: confirm sign-in and billing mode | Human runs `codex login status` personally and records the mode; an API-key mode blocks | G-LIVE |
| Codex home | Human chooses: a dedicated `CODEX_HOME` they sign into themselves (recommended; isolates F9 state), or the existing `~/.codex` with `--ignore-user-config --ignore-rules` | G-LIVE |
| Usage limit and model | Human sets one attempt, a time budget and a model or the default | G-LIVE |
| Provider network access for the one attempt | Human | G-LIVE |
| Start of the live run; acceptance of its candidate | Human, in the app | P2-05 |
| Any second live attempt, any other repository, any remote action | Human, new authorization | After P2-05 |

Stop conditions during P2-05 (any one halts and returns to review): Jojo reads or relays a credential; any unrequested remote effect; an unknown outcome treated as success; an orphaned process; a write outside the owned worktree attributable to the run; a second launch for the same Start.

## 8. Recommendation

Approve the Phase 1 packet and decisions B2–B5, then authorize **P2-01 — Codex adapter and launch envelope, offline** as the first and only implementation milestone. It resolves the two findings that would otherwise stop a real launch at the first step (the Node launcher under a cleared environment, and a launch contract that forbids what a signed-in Codex needs), and it does so without executing Codex, touching the network or using credentials. Keep P2-05 unauthorized until the offline rehearsal in P2-04 has passed.
