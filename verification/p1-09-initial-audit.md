# P1-09 — Completion audit (2026-11-14)

Status: **INCOMPLETE — NO-GO for P1-10**. This is an audit of interrupted implementation, not a completion certificate. No production source was changed during this audit.

## Findings

1. **Blocking: mandatory verification fails in its prescribed environment.** The shared P1-04 runner uses a temporary credential-free HOME/CARGO_HOME, locked offline Cargo and macOS OS network denial. In that environment `/bin/ps` fails to execute with `Operation not permitted` (exit 71), independently reproduced in `verification/p1-09-sandbox-ps.log`. The Rust process suite eventually finished in 180.07 seconds: **4 passed, 13 failed**, Cargo exit 101. A sample captured the fragmented UTF-8 test in `launch::supervise -> Supervised::tree -> snapshot -> system`, repeatedly invoking process inspection; other T09 tests waited on the serial test mutex. See `verification/p1-09-hang-sample.txt`. Inspection failure must remain UNKNOWN and the supervision path must return a bounded unconfirmed outcome. Do not widen the verification boundary just to obtain a pass.

2. **Blocking: the PID safety acceptance is not established.** `crates/jojo-storage/src/process.rs:371` performs a fresh identity check followed by a separate `/bin/kill` invocation. A process can exit and its PID can be reused between these operations. `ps lstart` has only one-second resolution. The group path after root reaping has a similar check-to-signal gap. The source explicitly documents these races; that does not satisfy the authoritative P1-09 requirement that a reused PID is never killed. The wrong-start-time test proves rejection of an already visible mismatch, not safety across this race. The held unreaped root path has stronger protection and is not the subject of this finding.

3. **Missing upstream audit deliverables.** The Vigla checkout exists at `bbd19ae2d5a77401502756c550b5dcd4ae59bbc9`, and its root LICENSE is Apache-2.0. `stream.rs` claims its capped-line technique was reproduced from Vigla and points to REPOSITORY_PINS.json, but `research/REPOSITORY_PINS.json` contains no Vigla entry. Exact relevant parser/lifecycle/error function references, applicable file-license review and comparison against P1-08 are absent. Checkout presence does not prove the required source inspection was completed. This audit did not fabricate attribution or certify that inspection.

4. **Incomplete milestone handoff.** At audit start there was no `scripts/verify_p1_09.py`, P1-09 result JSON or P1-09 report. Development and focused test logs existed. The shared runner also lacks a P1-09 -> P1-10 gate mapping. The audit launcher corrects only its own emitted gate to NO-GO; it is not a milestone completion runner.

## Implementation found

The interrupted work adds a pure bounded NDJSON framer and synthetic Claude/Codex normalizers, a std::process supervisor with bounded channels and raw spool files, a durable launch/observation journal, schema changes and test executables. Focused pre-existing logs report 17/17 T09 process tests and 7/7 stream tests passing; development Clippy also passed. These logs do not establish completion under mandatory verification. No root Git metadata exists, so a reliable base-commit diff is unavailable; a pre-change hash inventory exists at `verification/p1-09-before.json`.

## Audit verification

Reproduce the audit using `python3 verification/run_p1_09_audit.py`. Results and full output: `verification/p1-09-result.json`, `verification/p1-09.log`, `verification/p1-09-runner.log`. The first four mandatory groups passed: OS network denial, static boundary, boundary regression and formatting. Independent supplemental offline doctests and Clippy passed (`p1-09-audit-doctests.log`, `p1-09-audit-clippy.log`); these supplemental runs used the normal developer environment and are not substitutes for the remaining isolated mandatory checks. Full Rust regression failed (exit 101); the shared runner therefore did not reach isolated doctests, Clippy or Swift build. Boundary regression passed 30 tests. The 7 T09 parser tests passed inside the isolated run. No fixture processes from the audit temporary target remained at completion.

No P1-10 work, provider invocation, push or merge was performed. Fix the blocking runtime/safety findings, finish the upstream ledger and completion runner, then rerun all mandatory groups before changing the recommendation.
