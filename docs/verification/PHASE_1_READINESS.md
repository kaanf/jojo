# Phase 1 human readiness packet

Status: **Phase 1 foundation complete and ready for human review**, conditional on `verification/p1-17-result.json` recording `passed: true` and `human_readiness_review: GO`. **Phase 2 is not approved.** Nothing in this packet accepts a draft decision, starts live-provider work or authorizes the Claude Design UI migration. Each requires a separate, explicit human decision.

Machine-readable companion: `verification/p1-17-manifest.json`, written by the P1-17 gate. It holds platform, tool, schema and configuration identifiers; per-milestone result and report digests; the invariant-coverage map with pass state; the link check; Stage 1 and upstream checks; the process-launch ledger; the adapter scan; dependency licenses; and fixture and source digests.

## 1. What Phase 1 proves, and what it does not

Phase 1 proves **deterministic foundation contracts with fakes and local tools**:

- helper-owned SQLite authority;
- revision-bound human approvals;
- typed dependencies;
- configurable parallel scheduling with transactional claims;
- owned Git worktrees;
- supervised fixture processes with bounded streams;
- immutable results and independent validation;
- startup inspection, quarantine and reviewed continuation;
- safe close, cancel-close and visible failure;
- a bounded stdio protocol;
- a native SwiftUI/AppKit review and lifecycle shell;
- an end-to-end multi-agent scenario;
- a crash, durability and permission campaign.

It does **not** prove:

- real Claude Code or Codex compatibility;
- subscription, billing or authentication assurance;
- deployed-backend correctness;
- AI output quality;
- same-user hostile-code containment, which is **explicitly unsupported**.

All tests run offline under OS network denial with a temporary credential-free HOME. No provider CLI, model, credential, GitHub, Figma or deployment was used.

## 2. Evidence index

| Milestone | Deliverable | Record | Final gate |
|---|---|---|---|
| P1-01–P1-03 | Workspace boundaries; SQLite writer, receipts, epochs; artifacts and PRD schema | `verification/p1-0{1,2,3}-result.json`, `docs/verification/P1-0{1,2,3}.md` | passed |
| P1-04–P1-08 | Approvals and invalidation; plan graphs; scheduler; worktrees; provider DTOs and fakes | `verification/p1-0{4..8}-result.json` | passed, GO chain |
| P1-09–P1-13 | Process manager; results and validation; recovery; shutdown coordinator; stdio protocol | `verification/p1-{09..13}-result.json` | passed, GO chain |
| P1-14 | Native SwiftUI review/quit/recovery shell | `verification/p1-14-result.json`, `p1-14-gui.json` | 322 Rust, 55 boundary, 27 Swift, 9 GUI |
| P1-15 | Offline multi-agent feature scenario (T15) | `verification/p1-15-result.json`, `docs/verification/P1-15.md` | 329 Rust (+7 T15), 55, 27, 9 |
| P1-16 | Crash, resource, durability and permission campaign (T16) | `verification/p1-16-result.json`, `docs/verification/P1-16.md` | 339 Rust (+10 T16), 55, 27, 9 |
| P1-17 | This packet, provenance checks, final re-run of the whole suite | `verification/p1-17-result.json`, `p1-17-manifest.json`, `docs/verification/P1-17.md` | 339 Rust, 55, 27, 9 plus T17 |

The P1-17 gate re-runs the complete suite on the final tree. Evidence for failures and forced stops is retained: every gate keeps its full log (`verification/p1-NN.log`) and runner log, and earlier results are never overwritten by later gates.

## 3. Identifiers

Recorded by the gate in the manifest (`platform`, `tools`, `schema`, `configuration`). At packet authoring time:

| Item | Value |
|---|---|
| Platform | macOS 27.0 (26A428), arm64 |
| Toolchain | rustc and cargo 1.95.0, pinned by `rust-toolchain.toml`; Apple Swift 6.4; Apple Git 2.54.0 |
| Database | 10 migrations; system SQLite through `rusqlite =0.37.0` |
| Protocol | stdio 1.0; provider DTO schema 1; dependency schema 1 |
| Shipped configuration | global, per-provider and per-repository slots of one (accepted U01–U05) |

## 4. Invariant coverage

The manifest's `coverage` maps each requirement to named tests, and the gate fails unless every one passed in its run.

Requirements covered:

- human PRD/ADR/plan approval bound to the displayed bytes;
- multiple Claude/Codex processes with a default cap of one;
- worktree isolation and per-repository resources;
- typed dependencies, where an open PR never completes;
- immutable evidence and deterministic settlement;
- safe shutdown and verified receipts;
- recovery admission restrictions and no silent background execution;
- quarantined claims across fake reboot and clock jumps;
- exact continuation authorization and never-repeated effects;
- preserved work, validated resume and restore integrity;
- fail-closed unknown outcomes;
- one supervised helper and the channel boundary.

Strict Rust/Swift separation and "no process signalling from Swift" are enforced by the boundary guard and its 55 regression tests. GUI quit and recovery behaviour is covered by the 27 Swift tests and 9 runtime GUI scenarios.

## 5. Provenance ledger

- **Changed files:** each milestone result lists its changed files (`files_changed`, P1-14 onwards) against a recorded pre-edit baseline. P1-15 and P1-16 changed no production Rust or Swift source. They added tests, test-only fixture scenarios, gates and reports. P1-17 changes only its gate, this packet, its report and README status lines.
- **Reused content:** none. Phase 1 sources declare original fixtures. The P1-03, P1-08, P1-09 and P1-10 reports record that no upstream code, fixtures or template text were copied or adapted, so no third-party notices are owed for Phase 1 content.
- **Dependencies:** 32 locked crates, all with license metadata recorded in the manifest: MIT, Apache-2.0, Unlicense, Zlib, BSL-1.0 and Unicode-3.0 variants. No network, TLS, async-runtime or inference crate is present.
- **Stage 1 inputs:** the 14 original research and architecture documents match the P1-01 preservation baseline. All 10 upstream research checkouts are at their pinned commits, clean, with unchanged license files.
- **Process launches:** every launch site in Rust, Swift and Python is enumerated and classified in the manifest. In tests, provider executables are only the workspace `jojo-process-fixture`, or a synthetic path in DTO-only tests that never launch. Signals target exact pid or process-group identities; no name-based kill tool appears anywhere.
- **Remote and inference adapters:** none. The only URL in Rust or Swift sources is the deliberately non-routable `https://example.invalid/not-contacted`, which a T07 test uses to prove remotes are never touched.

## 6. Known limitations

1. Protocol 1.0 has no Start or recovery-resolution command. Recovery choices are displayed in the UI, and tests drive admission through the storage API after a reviewed recovery start.
2. SHUTDOWN_FAILED offers only Stay or an explicit unclean Disconnect and Quit; no retry or escalation is exposed.
3. The app is an unbundled SwiftPM executable: no signed `.app`, helper embedding or packaging.
4. The dependency observer in T15 is a test fixture that records only after a PASSED validation receipt and human acceptance. Selecting a validated output for a downstream step needs a human-reviewed plan revision.
5. Disk failures are injected and sleep/reboot are simulated; no volume is filled and no physical reboot occurs.
6. Backup/restore is a cold copy of a stopped helper's data root; there is no online backup command.
7. The database file mode follows the umask; its privacy rests on the 0700 data root.
8. Same-user hostile-code isolation is unsupported.

## 7. Residual risks for human assessment

- **Provider reality gap.** Real CLI streams, session semantics, auth prompts and version drift are untested. Fixtures model documented shapes only.
- **Operational recovery UX.** Recovery needs a human to inspect and start. With no Start command in the protocol, a real user cannot yet resolve holds from the GUI.
- **Durability under real resource exhaustion.** Injected faults cover the code paths, not file-system-specific ENOSPC behaviour or WAL growth.
- **Single-machine assumptions.** Ownership relies on one OS user and local `ps`, `kill` and `sysctl` semantics. Process identity across a real reboot is reasoned about, not exercised.
- **Draft decisions.** D01–D10 remain drafts and U06–U08 remain unresolved. Passing tests did not accept any of them.

## 8. SwiftUI frontend and a separate Claude Design migration

The P1-14 native shell is unchanged through P1-15 to P1-17 and re-verified in every gate (27 Swift tests, 9 runtime GUI scenarios). It is a suitable, stable baseline for a **separate** Claude Design migration because:

- its behaviour is pinned by tests;
- it owns no database writes and sends no signals;
- every quit path goes through one `QuitCoordinator`;
- the protocol it consumes is bounded and versioned.

A migration should keep `JojoCore` (transport, client and quit coordinator) intact, restyle only the view layer, and keep the T14 Swift and GUI suites as its regression gate. The migration itself was not started and needs its own approval.

## 9. Proposal for later live-provider work (not approved)

This follows the roadmap's Phase 2 ("one provider, isolated work and safe recovery").

1. **Preconditions (human decisions):**
   - approve this packet;
   - resolve or explicitly defer U06–U08;
   - choose the first provider (roadmap recommends Codex for its auditable structured contract);
   - set a usage limit;
   - name one throwaway local repository.
2. **Adapter, offline first:**
   - a Codex `exec --json` adapter behind the existing P1-08 DTOs and P1-09 supervised launcher;
   - executable discovered by explicit configured path, never PATH;
   - version and capability capture;
   - unknown fields fail closed.
   - Extend the fake fixtures with recorded-shape cases (split, malformed and oversized JSONL; `turn.failed`; auth-error text; missing executable) before any real launch.
3. **Protocol additions needed for real use:**
   - a reviewed `Start` command and recovery-resolution commands in stdio 1.x, gated by the existing approval, scheduler and recovery checks;
   - SwiftUI controls for them, styled separately from the migration.
4. **One authorized live attempt:**
   - cap one;
   - one approved feature plan in the throwaway repository;
   - no network permission beyond the provider's own;
   - no push or PR;
   - its candidate validated by the independent validator;
   - evidence preserved and inspected by a human.
5. **Stop conditions:**
   - any authentication relay or credential read by Jojo;
   - any unrequested remote effect;
   - an unknown outcome treated as success;
   - an orphaned process.

   Any one of these halts the work and returns to review.
6. **Exit:**
   - deterministic adapter tests;
   - one preserved, inspectable real attempt record;
   - an updated readiness packet.

Claude Code follows in a later phase on the same contract.

## 10. Requested human decision

Review sections 6–9, then either approve Phase 1 completion and separately authorize any next stage, or return findings. Until then the repository stays at Phase 1. It remains not a Git repository, and nothing has been pushed or merged.
