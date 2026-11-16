# Jojo status and next step

Read this first in a new session. Updated 2026-11-16. Update it whenever a step finishes or a decision changes.

## Where we are

| Step | State |
|---|---|
| Phase 0, Phase 1 (P1-01 … P1-17) | Done |
| Phase 2 (P2-01 … P2-05) | Done. The one live Codex attempt ran; the human accepted its candidate on 2026-11-16. |
| **M0** (close-out of Phase 2) | **Done** |
| **Refactor R-03** (one data-driven gate) | **Done**, with the open points below |
| **Refactor R-01** (typed protocol messages) | **Done** (2026-11-16). Details: [DESIGN_ROADMAP_DIFFERENCES.md](DESIGN_ROADMAP_DIFFERENCES.md), E4 |
| Refactor R-02 (split the long functions) | Optional, not started. Needs its own GO |
| **M1** (Claude Code adapter) | **Done except automatic discovery (D3).** Details: DESIGN_ROADMAP_DIFFERENCES.md, E5 |
| Design system as text | Done (`docs/design/`). No screen is implemented yet |
| Design wave 1, M2 … M6 | Not started |

## Next step: design wave 1

Chosen by the human on 2026-11-16. Build from `docs/design/` (system fonts): app shell, sidebar, status model, then the screens whose data exists today (Recovery Center and launch review, Agent Runs, Source Control local diff and scope, Verification, Approval Inbox for spec and plan, Specifications, Settings, Activity). Write each screen down in `docs/design/` from its canvas board before building it. The other two candidates stay open:


1. **D3, the rest of M1.** Find the Claude Code and Codex executables automatically, record version and digest per run, warn instead of refusing when the binary changes. Needs a decision first: where discovery runs and how the version is learned without running the CLI (E5, "Not done").
2. **Design wave 1.** App shell, sidebar, status model and the screens whose data exists today, built from `docs/design/`.
3. **M2.** Feature Chat lead agent. It can now run on either CLI.

The roadmap order is in [SOURCE_FIRST_REUSE_MAP.md](SOURCE_FIRST_REUSE_MAP.md), section 3. Refactor R-02 stays optional and needs its own GO.

To start a helper that can run Claude Code steps, pass (with `--disposable-root`): `--claude-executable <resolved native binary> --claude-version "2.1.311 (Claude Code)" --claude-sha256 <digest> --claude-home <home> --claude-user <account> --claude-auth native-subscription`. `scripts/live_m1_claude.py` shows the whole flow.

## The three documents

| File | What it holds |
|---|---|
| [SOURCE_FIRST_REUSE_MAP.md](SOURCE_FIRST_REUSE_MAP.md) | Jojo inventory, upstream code per capability (repository, pinned SHA, files, license), roadmap M0–M6 and its order |
| [DESIGN_ROADMAP_DIFFERENCES.md](DESIGN_ROADMAP_DIFFERENCES.md) | Every decision where the Claude Design canvas and the roadmap disagreed (D1–D4), engineering decisions (E1–E3), how to reverse each, and the change log |
| This file | Current state and next step |
| [docs/design/](../design/README.md) | The design system as text, transcribed from the canvas on 2026-11-16: tokens, components, states. Read before any SwiftUI design work |

Design canvas: "Jojo macOS", https://claude.ai/artifact/UuyW4DFxPEciFKZm8ZGM24

## How to verify

```sh
python3 scripts/verify.py              # everything, including 17 GUI scenarios (the app opens on screen; leave the Mac alone)
python3 scripts/verify.py --skip gui   # without the GUI stage; result is passed but complete: false
```

Requirements live in `scripts/gates/current.json`; the result goes to `verification/gate-m1-result.json` and `.log` (earlier results stay in `gate-m0-…` and `gate-r01-…`). Last full run (2026-11-16, after M1): 105 script tests, 420 Rust tests, 39 Swift tests, 17 GUI scenarios, no failures. Rust test names in `required` are written as the test runner prints them, with the module path for unit tests (`protocol::tests::…`).

The twelve P2-04 rehearsal scenarios are run by hand (they passed after R-01; evidence in `~/JojoDisposable/r01-rehearsal/20261116T034931Z`):

```sh
R=~/JojoDisposable/r01-rehearsal/$(date -u +%Y%m%dT%H%M%SZ); mkdir -p $R; chmod 700 $R
cd scripts && python3 gui_p2_04.py --app ../macos/JojoApp/.build/debug/JojoApp --helper ../target/debug/jojo-helper \
  --fixture ../target/debug/jojo-codex-fixture --root $R --out $R/result.json
```

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

1. Standing authorization for runs in registered repositories, instead of a GO per step and per live attempt. The human authorized the M1 probes and one M1 live run on 2026-11-16; both are used up. **No further live provider run is authorized.**
2. Whether a write the provider's own sandbox allows (`/tmp`) stays a stop condition.
3. Parallel capacity for the demo (2 proposed; the design shows 3 overall, 2 per repository).
4. A written record of the weft owner's permission.

## Known gaps

- Automatic executable discovery (D3) is not built; see "Next step".
- The Claude Code stream profile knows only the record kinds seen on 2026-11-16; an unseen kind ends a run as a protocol failure.
- A stop of a running Claude Code process tree was not exercised live.

- The gate once failed compiling `libsqlite3-sys` twice (missing `bindgen.rs`). Root cause unknown; not seen in the four runs after the compile order was changed.
- The twelve P2-04 rehearsal scenarios (`scripts/gui_p2_04.py`) are not in the new gate.
- README and the architecture documents still describe the old rule that escaped descendants are never signalled.
- The Agent Guard cannot reach an escaped descendant whose parent chain is already broken (agent died first, then the helper).
- The test fixture has one escaped child; it does not reproduce Codex's nine-process tree. The next live run is the real evidence.

## Working notes for an assistant

- The project is a local Git repository since 2026-11-16 (branch `main`; the human chose `git init`). Remote `origin` is https://github.com/kaanf/jojo.git, a public repository; the human authorized the first push of `main` on 2026-11-16, and each later push needs the human again. `1f6d981` is the tree before R-01. `research/upstream` and `__pycache__` are excluded in `.git/info/exclude`, not in `.gitignore`, because the historical gate scripts hash `.gitignore`. Nothing is pushed without the human. Older backup, before M0: `~/Projects/jojo-backups/pre-m0-20261115T165918Z.tar.gz`.
- A provider CLI update needs: new probes, the version constant and parser name in `crates/jojo-domain/src/claude.rs` (or `codex.rs`), the captured streams in the parser tests, and the digest given to the helper. `scripts/tests` and the domain tests may not contain the bare words `File` or `Command` outside the allowed crates, even inside captured data.
- Protocol changes: add the request to `SPECS` in `crates/jojo-domain/src/protocol.rs` and, if the app sends it, to `Request` in `Protocol.swift`; `scripts/tests/test_protocol_fields.py` fails until both agree. `crates/jojo-storage/tests/r01_frames.rs` pins 168 answers; when an answer changes on purpose, change that line of `EXPECTED` and say so in the change log.
- The boundary guard treats the bare word `Command` as a process launch anywhere outside the process crates, in type and variant names too (`Class::Mutation` is named that way for this reason).
- The architecture boundary guard (`scripts/check_p1_01_boundaries.py`) forbids `include_str!`, `unsafe`, FFI, network types and filesystem access outside the allowed crates, in tests too. Run `python3 -m unittest discover -s scripts/tests` after touching Rust or scripts.
- Tests run inside the gate's sandbox cannot execute `/bin/ps`. Use the injected inspector (`inspector()`, `JOJO_TEST_PS`), never the free `verify_identity`. A test that passes with plain `cargo test` can still fail in the gate.
- Fourteen historical reports and the nine `research/upstream` pins are hash-checked by the gate. Add new documents; do not edit those.
- The five reference repositories of the reuse map were only cloned to a temporary directory. Clone them again at the pinned SHAs when their files are needed; adding them under `research/upstream` needs the preservation baseline to be extended.
- Live evidence of P2-05: `/Users/kaan/JojoDisposable/p2-05/20261115T133947Z`.
- The human writes in Turkish and is answered in Turkish; project documents are in English.
