# Jojo

Personal, local-first AI Software Factory for macOS. **P1-01–P1-17 are implemented** (offline fixtures; see milestone verification).

The existing architecture/research reports remain historical records. Explicit U01–U05
acceptance is recorded separately in
[ACCEPTED-DECISIONS-2026-11-14.md](docs/architecture/ACCEPTED-DECISIONS-2026-11-14.md).
U06–U08 remain unresolved. Phase 1 was accepted as the baseline on 2026-11-15 ([readiness packet](docs/verification/PHASE_1_READINESS.md)). Of [Phase 2](docs/planning/PHASE_2_VERTICAL_SLICE.md), **P2-01** (offline Codex adapter, [verification](docs/verification/P2-01.md)) and **P2-02** (protocol 1.1 Start and recovery resolution, [verification](docs/verification/P2-02.md)) are authorized and implemented; no live Codex execution has been authorized or performed.

## Foundation boundaries

| Location | Purpose |
|---|---|
| `crates/jojo-domain` | Pure typed IDs, attempt bindings/state vocabulary, injected clock/ID traits, policy data and lexical root configuration; strict bounded JSON and the typed PRD/use-case schema (import, canonical form, rendering, structural validation) |
| `crates/jojo-helper` | Separate executable; validates configuration, owns the database for `--engine-probe`, or serves the bounded stdio protocol (`--serve`, P1-13); no implicit runtime |
| `crates/jojo-storage` | SQLite authority: migrations, single writer, revision CAS, command receipts, events, claim/outbox primitives, OS ownership lock and engine epochs; immutable artifact versions with content/rendered digests, clarification questions/answers and review readiness |
| `crates/jojo-test-support` | Dev-only deterministic clock, namespaced ID fixtures and a complete sample PRD, not linked into the helper |
| `macos/JojoApp` | Dependency-free Swift package: native SwiftUI/AppKit review and lifecycle shell over the helper protocol (P1-14); unbundled SwiftPM executable |
| `scripts` | Offline milestone verification through P1-17 and P2-02, and negative tests for the architecture boundary guard |
| `verification` | Preservation baseline, actual verification results, source hashes and tool output |

The Rust core includes revision-bound approvals, typed dependency evaluation, resource
scheduling, owned local Git worktrees, provider-neutral contracts and fixture process
supervision. Cargo dependencies are explicitly pinned and allowlisted; workspace code
forbids unsafe/FFI. SQLite uses rusqlite with the macOS system library. Swift has no
remote package dependency. No live provider/auth adapter, network client,
background daemon or build script is implemented. Defaults of one slot are configurable;
tests also exercise concurrent mixed-provider attempts.

The helper supports only `--help`, `--version`, `--check-config` `--engine-probe` and `--serve`:

```sh
cargo build --offline --locked --workspace --all-targets
cargo test --offline --locked --workspace --all-targets
cargo run --offline --locked -p jojo-helper -- --check-config \
  --data-root /absolute/test/data --worktree-root /absolute/test/worktrees
```

Defaults are `$HOME/Library/Application Support/Jojo` and separate sibling
`$HOME/Library/Application Support/JojoWorktrees`. Tests can override both without HOME.
Paths must be absolute, traversal-free and non-overlapping. **`--check-config` creates no
directory**; `--engine-probe` creates only the private (0700) data root and its database.
Configuration checks are lexical; storage and owned-worktree operations enforce their
own ownership checks. Recovery/continuation and clean application shutdown remain later
milestones; uncertain work and evidence are retained.

## Reproduce T01 without network or credentials

Prerequisites must already be installed: macOS/Xcode with SwiftUI and Swift 6+, Rust/Cargo
1.95.0 (rustfmt and Clippy), Python 3.11+, Git and `sandbox-exec`. The pinned Rust version
is in `rust-toolchain.toml`; no toolchain installation is performed. Homebrew Rust works
without rustup. Cargo's lockfile pins the workspace and its allowlisted dependencies. Swift has no remote
package dependencies. Reproducibility means locked inputs and deterministic checks, not
byte-identical binaries across different Apple SDKs/machines.

```sh
python3 scripts/verify_p1_01.py
```

The verifier uses temporary HOME/CARGO_HOME/target/Swift caches and an explicit environment
with no inherited provider variables. Every build/test/helper/Git subprocess runs under a
macOS profile denying network access. A denied numeric-loopback connection verifies the
profile is effective; it sends no API request. Cargo additionally uses offline/locked mode,
and Swift automatic resolution is disabled. The script never installs packages, discovers
provider CLIs, reads provider credentials or uses the real application data root. It fails
rather than falling back if required tooling/network denial is unavailable.

Checks include Rust formatting/build/tests/doc-tests/Clippy, the dependency/launch boundary
guard and its negative tests, configuration-only helper behavior, SwiftUI package build,
and unchanged hashes for all 14 pre-existing reports plus pins/status of nine research
checkouts. Results are written to `verification/p1-01-result.json` and `verification/p1-01.log`;
a failed rerun replaces the pass status with a failure. The preservation baseline was captured
before implementation. Build/test outputs stay temporary; ordinary manual Cargo builds use
ignored `target/`.

The static guard is a small allowlist/regression check, not hostile-code containment or a
formal proof of arbitrary Rust/Swift behavior. No old upstream source/fixtures were extracted.
New implementation is original; toolchain/standard-library licensing remains upstream-owned.
Distribution/licensing choices beyond personal use remain U08.

## Reproduce T02

```sh
python3 scripts/verify_p1_02.py
```

Runs every T01 check above against the current sources plus the P1-02 scenarios (same
sandbox, no network), and writes `verification/p1-02-result.json` and `p1-02.log`.
`python3 scripts/verify_p1_01.py` now runs the T01 regression only and writes
`verification/p1-01-regression-result.json`; `p1-01-result.json` remains the historical
record of the P1-01 source state.

## Reproduce T03

```sh
python3 scripts/verify_p1_03.py
```

Runs every T01/T02 check against the current sources plus the P1-03 scenarios and writes
`verification/p1-03-result.json` and `p1-03.log`. Schema version 2 is now the latest, so
`verify_p1_02.py` expects `schema=2` from the helper probe; `p1-02-result.json` remains the
historical record of the P1-02 source state.

See the [P1-01](docs/verification/P1-01.md), [P1-02](docs/verification/P1-02.md) and
[P1-03](docs/verification/P1-03.md) verification reports for actual results, guarantees
and remaining limitations. Historical milestone reports describe their original source state; the current completion gate is P1-09.


## Reproduce P1-09 (current gate)

```sh
python3 scripts/verify_p1_09.py
```

Runs the entire Rust workspace, boundary tests, formatting, doctests, Clippy and Swift
build under OS network denial, locked offline Cargo and a credential-free temporary
HOME. On macOS the verifier prepares a non-setuid, ad-hoc-signed private copy of the
host `/bin/ps`; the original setuid executable cannot run in that sandbox. The copy
reports real OS processes inside the same network-denied boundary and is deleted
with the temporary test environment. `codesign` is a verification prerequisite only.

P1-09 adds a durable launch journal, explicit fixture process launches, bounded raw
stdout/stderr spools, synthetic Claude/Codex NDJSON normalization, process identity
observations and capability-specific INT/TERM/KILL stopping. The group leader stays
unreaped until group members stop, preventing PID/PGID recycling during signals.
Snapshot-only escaped PIDs are never signalled; unknown stops retain ownership claims.
No live provider, private provider directory, extra runtime dependency or result
finalization is involved.

See [P1-09 verification](docs/verification/P1-09.md),
[Vigla source review](docs/research/P1-09_VIGLA_SOURCE_REVIEW.md) and
[repository pins](research/REPOSITORY_PINS.json).
The machine gate is `verification/p1-09-result.json`; verbose output is `p1-09.log`.
P1-10 requires a separate instruction and has not started.

P1-13 adds the bounded `--serve` stdio protocol and helper worker/close coordination.
See [protocol](docs/architecture/STDIO_PROTOCOL.md) and [verification](docs/verification/P1-13.md).

P1-14 adds the native SwiftUI review and lifecycle shell ([verification](docs/verification/P1-14.md)).
P1-15 adds the offline multi-agent feature scenario (`python3 scripts/verify_p1_15.py`; [verification](docs/verification/P1-15.md)).
P1-16 adds the crash, resource, durability and permission campaign (`python3 scripts/verify_p1_16.py`; [verification](docs/verification/P1-16.md)).
P1-17 adds the provenance checks and the [Phase 1 readiness packet](docs/verification/PHASE_1_READINESS.md) (`python3 scripts/verify_p1_17.py`; [verification](docs/verification/P1-17.md)).
P2-01 adds the offline Codex adapter, the Codex-only launch-envelope amendment and a fail-closed parser profile, exercised with fixture executables only (`python3 scripts/verify_p2_01.py`; [verification](docs/verification/P2-01.md)).
P2-02 adds helper protocol 1.1: disposable-root repository registration, feature-bundle import as unapproved drafts, one human Start routed only through the Codex adapter, recovery resolution, and minimal Start/Recovery controls in the app (`python3 scripts/verify_p2_02.py`; [protocol](docs/architecture/STDIO_PROTOCOL.md#protocol-11-p2-02); [verification](docs/verification/P2-02.md)).
