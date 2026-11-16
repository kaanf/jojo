# Jojo status and next step

Read this first in a new session. Updated 2026-11-16. Update it whenever a step finishes or a decision changes.

## Where we are

| Step | State |
|---|---|
| Phase 0, Phase 1 (P1-01 … P1-17) | Done |
| Phase 2 (P2-01 … P2-05) | Done. The one live Codex attempt ran; the human accepted its candidate on 2026-11-16. |
| **M0** (close-out of Phase 2) | **Done** |
| **Refactor R-03** (one data-driven gate) | **Done**, with the open points below |
| **Refactor R-01** (typed protocol messages) | **Next. Not started.** |
| Design wave 1, M1 … M6 | Not started |

## Next step: Refactor R-01

Defined in [REFACTOR_PLAN.md](../planning/REFACTOR_PLAN.md), section R-01. In short: one typed definition of each protocol message in `jojo-domain`, `service.rs` dispatching on an enum, the recovery assessment sent as structured fields in addition to the existing text, and the Swift app reading those fields instead of matching debug text. Behaviour must not change; the existing protocol tests are the oracle.

Before starting, ask the human once more whether to `git init` (asked three times, not yet answered). Without it, take a backup as was done before M0.

After R-01: design wave 1 and M1 (Claude Code adapter) in parallel, then M2 (Feature Chat lead agent), M3 (project record and plan runner), M4 (validation, review, repair), M5 (push and draft PR). The order and the reasons are in [SOURCE_FIRST_REUSE_MAP.md](SOURCE_FIRST_REUSE_MAP.md), section 3.

## The three documents

| File | What it holds |
|---|---|
| [SOURCE_FIRST_REUSE_MAP.md](SOURCE_FIRST_REUSE_MAP.md) | Jojo inventory, upstream code per capability (repository, pinned SHA, files, license), roadmap M0–M6 and its order |
| [DESIGN_ROADMAP_DIFFERENCES.md](DESIGN_ROADMAP_DIFFERENCES.md) | Every decision where the Claude Design canvas and the roadmap disagreed (D1–D4), engineering decisions (E1–E3), how to reverse each, and the change log |
| This file | Current state and next step |

Design canvas: "Jojo macOS", https://claude.ai/artifact/UuyW4DFxPEciFKZm8ZGM24

## How to verify

```sh
python3 scripts/verify.py              # everything, including 17 GUI scenarios (the app opens on screen; leave the Mac alone)
python3 scripts/verify.py --skip gui   # without the GUI stage; result is passed but complete: false
```

Requirements live in `scripts/gates/current.json`; the result goes to `verification/gate-m0-result.json` and `.log`. Last full run (2026-11-16): 97 script tests, 396 Rust tests, 35 Swift tests, 17 GUI scenarios, no failures.

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

1. `git init` for the Jojo project.
2. Standing authorization for runs in registered repositories, instead of a GO per step and per live attempt. **No further live provider run is authorized.**
3. Whether a write the provider's own sandbox allows (`/tmp`) stays a stop condition.
4. Parallel capacity for the demo (2 proposed; the design shows 3 overall, 2 per repository).
5. A written record of the weft owner's permission.

## Known gaps

- The gate once failed compiling `libsqlite3-sys` twice (missing `bindgen.rs`). Root cause unknown; not seen in the four runs after the compile order was changed.
- The twelve P2-04 rehearsal scenarios (`scripts/gui_p2_04.py`) are not in the new gate.
- README and the architecture documents still describe the old rule that escaped descendants are never signalled.
- The Agent Guard cannot reach an escaped descendant whose parent chain is already broken (agent died first, then the helper).
- The test fixture has one escaped child; it does not reproduce Codex's nine-process tree. The next live run is the real evidence.

## Working notes for an assistant

- The project is not a Git repository. Backup taken before M0: `~/Projects/jojo-backups/pre-m0-20261115T165918Z.tar.gz`.
- The architecture boundary guard (`scripts/check_p1_01_boundaries.py`) forbids `include_str!`, `unsafe`, FFI, network types and filesystem access outside the allowed crates, in tests too. Run `python3 -m unittest discover -s scripts/tests` after touching Rust or scripts.
- Tests run inside the gate's sandbox cannot execute `/bin/ps`. Use the injected inspector (`inspector()`, `JOJO_TEST_PS`), never the free `verify_identity`. A test that passes with plain `cargo test` can still fail in the gate.
- Fourteen historical reports and the nine `research/upstream` pins are hash-checked by the gate. Add new documents; do not edit those.
- The five reference repositories of the reuse map were only cloned to a temporary directory. Clone them again at the pinned SHAs when their files are needed; adding them under `research/upstream` needs the preservation baseline to be extended.
- Live evidence of P2-05: `/Users/kaan/JojoDisposable/p2-05/20261115T133947Z`.
- The human writes in Turkish and is answered in Turkish; project documents are in English.
