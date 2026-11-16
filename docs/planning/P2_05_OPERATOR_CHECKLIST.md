# Operator checklist — first real Codex attempt (P2-05)

Status: **prepared, not authorized.** Written with P2-04 on 2026-11-15. Nothing in this document permits a live run. P2-05 and every G-LIVE decision in the [Phase 2 plan](PHASE_2_VERTICAL_SLICE.md), section 7, need their own explicit human authorization first.

Every step below was rehearsed offline with a fake Codex executable ([P2-04 verification](../verification/P2-04.md), [P2-04.1 verification](../verification/P2-04-1.md)). The rehearsal says nothing about how the real CLI behaves; section 8 lists what is still unknown.

One attempt means one Start. A failed, interrupted or unknown outcome with complete evidence is an acceptable result. Treating such an outcome as success is not.

## 1. Authorizations to have in writing before anything else

| Item | Recorded value |
|---|---|
| GO for P2-05 (one live attempt) | |
| U06: sign-in confirmed by the human with `codex login status`; mode is a native subscription, not an API key | |
| Codex home: a dedicated directory the human signed into, or the existing one | |
| Model (or default) and time budget for the one attempt | |
| Provider network access for the one attempt | |
| Disposable root (new, private, outside every project) | |

If any row is empty, stop here.

## 2. Preflight by the human, outside Jojo

Jojo never runs these and never reads the Codex home.

1. `codex --version` prints exactly `codex-cli 0.169.0`. The adapter refuses any other version; a different version returns the work to the P2-01 offline gate.
2. Locate the native Mach-O binary, not the launcher script (plan section 3, finding F2). Check with `file <path>`: it must say `Mach-O 64-bit executable`. The path must be absolute, with no symbolic link in it.
3. Record `shasum -a 256 <native binary>`.
4. `codex login status` shows a subscription sign-in. An API-key mode blocks the attempt.
5. `python3 scripts/verify_p2_04_1.py` passed on this exact tree: `verification/p2-04.1-result.json` has `passed: true`, and no file listed in its `input_sha256` has changed since.
6. Build what will run: `cargo build --offline --locked -p jojo-helper` and `swift build --package-path macos/JojoApp`.
7. No other Jojo helper or app is running.

## 3. Roots

Use new directories. Never reuse a rehearsal root or an earlier data root: a data root that holds a finished or interrupted attempt refuses every further Start.

```sh
mkdir -m 700 <disposable root>          # private, at least three levels deep, outside every project
python3 scripts/p2_04_sample.py repository --disposable-root <disposable root>
```

Expected: `"commit": "45147111b537d1965241c4d2b40e32ad9b740ea9"`, branch `main`, files `slugify.py` and `test_slugify.py`. Any other commit means the sample files changed: stop.

Choose a new `<data root>` and `<worktree root>` (the helper creates them). They must not be inside the Codex home, and the Codex home must not be inside them.

## 4. Import and approve, stage by stage

The app has no control for registration or import. They are protocol commands sent by the sample tool to a helper it starts itself. Only one helper can own a data root, so **the app must be quit before each import** (otherwise the import fails: the second helper exits with status 3).

```sh
python3 scripts/p2_04_sample.py import --helper target/debug/jojo-helper \
  --data-root <data root> --worktree-root <worktree root> \
  --disposable-root <disposable root> --repository <disposable root>/slugify-sample
```

Start the app with the same roots and the Codex configuration from section 2:

```sh
macos/JojoApp/.build/out/Products/Debug/JojoApp \
  --helper "$PWD/target/debug/jojo-helper" \
  --data-root <data root> --worktree-root <worktree root> --disposable-root <disposable root> \
  --codex-executable <native binary> --codex-version "codex-cli 0.169.0" \
  --codex-sha256 <digest from section 2> --codex-home <codex home> --codex-auth native-subscription \
  --trace <data root>/../live-trace.jsonl
```

Use absolute paths. `--codex-auth native-subscription` is your attestation of step 2.4, not something Jojo checks.

| Round | Import reports | In the app (Review tab: enter the identity, Load Review, read, Approve) | Then |
|---|---|---|---|
| 1 | `stage=PRD awaiting=…020401` | Approve `00000000000000000000000000020401` (PRD) | Quit the app |
| 2 | `stage=ADR awaiting=…020402` | Approve `00000000000000000000000000020402` (ADR) | Quit the app |
| 3 | `stage=PLAN awaiting=…020404,…020403` | Approve `…020404` (validation plan), then `…020403` (plan) | Stay in the app |

What you are approving is in `scripts/samples/slugify/`: `prd.json`, `adr.md`, and the scope and command in `bundle.json` (write scope `slugify.py`, baseline `test_slugify.py`, command `/usr/bin/python3 -m unittest -q`). Read them there. **The Review tab shows identities, the revision and digests, not the document text.** The helper refuses an import whose content differs from what was imported before, and each approval binds the displayed revision and digests. Every revision must be 1. The live bundle contains no `fixture:` line; the rehearsal directive exists only in rehearsal imports.

A fourth import, after the run, must report `stage=COMPLETE awaiting=`.

## 5. Start, exactly once

1. Recovery tab: Inspect Again. It must list no interrupted or unknown work. Anything listed: stop.
2. Start After Review. The header shows `Admission: Open`. This starts nothing.
3. Start tab: enter the five identities (PRD `…020401`, ADR `…020402`, plan `…020403`, validation `…020404`, step `00000000000000000000000000000001`). Press Start **once**.
4. The status line reads `Start accepted for attempt <identity> (launched)`. Write the attempt identity down.

If Start is refused, the status line gives the helper's reason and nothing was started. Do not work around a refusal.

If the outcome of Start is shown as unknown, do not press Start again. The app reconciles by receipt; a second Start for this step is refused by the helper in any case.

## 6. During the run

1. Candidate tab: enter the attempt identity, Load. `No sealed result yet` until the run settles.
2. In a terminal: `ps -axo pid,ppid,pgid,lstart,command | grep -F "<native binary>"`. Exactly one process tree, started by the helper. Record its PID and start time. Beside it, in the same process group, there is one `jojo-helper --agent-guard <that PID> /bin/ps` process: the guard. Record any process of the tree whose PGID differs from the agent's PID: the guard cannot reach those.
3. Do not touch the owned worktree, the sample repository or the data root.

Quitting while the run is active asks first. Cancel keeps the run going. Confirm Quit interrupts the run: the helper stops the agent, seals an `Interrupted` result and exits; that ends the attempt.

## 7. After the run settles

1. Outcome and reason, and what the provider reported, are shown as the provider's statement. `Succeeded` is not a validation result.
2. Changed files: exactly `Modified slugify.py`, marked ALLOWED, write scope PASS. Read the diff.
3. Run Validation, once. The result is UNKNOWN until the helper records a verdict. The report must show the approved command and `network: denied by OS sandbox`.
4. Decide:
   - Accept Candidate only for outcome `Succeeded`, validation PASS, scope PASS and a diff you have read.
   - Reject Candidate in every other case.
   Accepting records a decision. Nothing is merged, pushed or published.
5. Quit with Cmd-Q. The app must report a clean shutdown.

Evidence to check and keep (acceptance criteria of plan section 6):

| Check | How |
|---|---|
| No process from the run remains | the `ps` command of section 6 prints nothing |
| Source repository untouched | `git -C <repository> status --porcelain` is empty; `HEAD` is still `4514711…` on `main`; the only new ref is `refs/heads/jojo/<attempt>` at that commit; `git remote` is empty |
| Exactly one launch | one attempt identity; the Runs tab lists one `scheduler_attempt` row |
| Validation ran elsewhere | one directory under `<data root>/validation/`, not the owned worktree |
| Raw output kept | spools and evidence under `<data root>` |
| App trace | the `--trace` file |

Keep the disposable root, data root, worktree root and trace. Delete nothing, whatever the outcome.

## 8. Stop conditions

Any one of these halts the work and returns it to review. Do not retry, repair or clean up.

| Condition | How it shows |
|---|---|
| Jojo reads or relays a credential | any prompt for sign-in from Jojo; any Codex home content in the data root or trace |
| An unrequested remote effect | a remote, a push, or any change to the source repository other than the `jojo/<attempt>` ref |
| An unknown outcome treated as success | Accept offered or accepted for anything but `Succeeded` with a passing validation |
| An orphaned process | a process from section 6.2, or an `--agent-guard` process, that still exists a few seconds after the helper is gone |
| A write outside the owned worktree | a changed file anywhere else that the run can be blamed for |
| A second launch for the same Start | more than one agent process tree, attempt or run |

What the rehearsal showed for the interruptions, and what to do:

- **Sign-in error.** The run ends `Failed — provider_error` and the provider's message is in the agent output. Jojo does nothing about sign-in. Reject, quit, return to review. Do not sign in "through" anything and do not start again.
- **The app crashes or is killed.** The helper sees its input close, stops the agent itself and exits. Check with `ps`. Reopen the app on the same roots and follow "Recovery" below.
- **The helper crashes or is killed** (the app shows `Helper: ended`). Since P2-04.1 every agent and validation process group has a guard: a second `jojo-helper --agent-guard <pid> /bin/ps` process inside that group, which notices that the helper is gone, sends TERM to the agent, waits up to 2 seconds and then sends KILL to the whole group, itself included ([P2-04.1 verification](../verification/P2-04-1.md)). In the rehearsal the agent was gone within a fraction of a second.
  1. Check with the `ps` command of section 6.2 that nothing is left. Also `ps -axo pid,pgid,command | grep -F -- "--agent-guard"` must print nothing.
  2. Quit the app (it reports that it cannot verify a stop), reopen it on the same roots and follow "Recovery" below.
  3. **If a process is still there**, the guard did not work or the agent started processes outside its group. This is the stop condition "orphaned process". Recovery then lists the attempt with `live orphan pid:<n> start:<time>` (or a group with live members) and offers only "Keep on hold"; Start is refused; quitting fails visibly and needs "Disconnect and Quit". Jojo will not signal it. Ending that process is your decision and your action: wait for it to exit, or terminate that exact PID yourself after checking that PID and start time match the Recovery line. Never by name. When it is gone, Inspect Again and follow "Recovery".
  4. The guard does not cover a helper that is frozen but not dead. If the app shows the helper as unresponsive rather than ended, treat it as a stop condition and follow step 3.
- **Recovery** (same data root). Recovery lists the attempt with no blockers. "Validate existing output" settles it as `Interrupted`; the candidate can then be loaded, validated and rejected. It cannot be accepted, even with a passing validation, and no new Start is possible on this data root. Do not claim otherwise and do not delete the root to get around it.

## 9. Still unknown after the rehearsal

The fake executable writes documented-shape records and reacts to signals like any small program. None of the following has been observed for the real CLI:

- the exact JSONL event shapes of 0.169.0, and whether the stream ends with a terminal record;
- approval behaviour in `exec`, and whether hooks, plugins or MCP servers load under `--ignore-user-config`;
- what the `workspace-write` sandbox allows outside the working directory and in the Git common directory;
- the real process tree (sandbox helper, shells), whether any of it leaves the agent's process group, and how it reacts to SIGTERM and to closed pipes;
- where sign-in state lives and which billing mode is active.

Each is observed, not assumed, during the attempt, and recorded.

## 10. Record sheet

| Field | Value |
|---|---|
| Date, operator | |
| Tree: `verification/p2-04.1-result.json` hash check | |
| Native binary path, version, SHA-256 | |
| Codex home (path only) | |
| Disposable root, data root, worktree root | |
| Sample commit | |
| Import stages seen (PRD, ADR, PLAN, COMPLETE) | |
| Approved revisions and digests (PRD, ADR, validation plan, plan) | |
| Attempt identity | |
| Agent PID and start time; process check after the run | |
| Outcome, reason, provider statement | |
| Changed files and scope verdict | |
| Validation state, receipt, command shown | |
| Decision (Accept / Reject) and why | |
| Shutdown result | |
| Source repository check | |
| Stop condition reached (which, when) | |
| Deviations from this checklist | |
