# Refactor plan — code structure

Status: **PROPOSED, not authorized.** Written 2026-11-15 after P2-04.1. Planning only: no code was changed for this document. Each milestone below needs its own explicit human GO, like every other milestone.

Recommended timing: **after P2-05.** The tree that passed the P2-04.1 gate is the one the operator checklist binds the live attempt to; a refactor before the attempt would mean a new gate on a tree that has never been rehearsed. Nothing here is needed for a single supervised attempt.

## Why

The architecture boundaries are sound and enforced (crate layering, one process launch surface, no unsafe code, no network code, 87 boundary tests). The weaknesses are inside those boundaries. Measured on the P2-04.1 tree:

| Finding | Evidence |
|---|---|
| Very long functions on the most critical paths | `handle` 296 lines and `slice_command` 274 (`crates/jojo-helper/src/service.rs`); `supervise` 283 (`crates/jojo-storage/src/launch.rs`); `spawn_guarded` 289 (`crates/jojo-storage/src/process.rs`) |
| The protocol is text-shaped | message types matched as strings; payload field sets written by hand, once in Rust and once in Swift; the recovery assessment reaches the app as Rust debug output (`Assessment { blockers: [...] }`) and is matched there by substring |
| Gate scripts are fragile | 22 `scripts/verify_*.py` files that import each other, change each other's module variables and are each valid only at their own milestone; three of the four P2-04.1 gate runs failed for harness or test reasons, none for a product defect |
| Large files, flat namespace | `schema.rs` 1,701 lines, `worktrees.rs` 1,447, `service.rs` 1,440, `process.rs` 1,334; `jojo-storage` re-exports everything from one flat namespace |
| Inline SQL in very long lines | lines of 450–650 characters in `schema.rs`, `approvals.rs`, `scheduler.rs` |
| Test hooks and production settings share a type | `LaunchHooks` is documented as test barriers but also carries the process inspector and the guard executable |

## Rules for every milestone

- **Behaviour does not change.** No protocol frame, database schema, approval, evidence, recovery or process-safety rule is altered. A change that would alter one is out of scope and goes back to the human.
- **Existing tests are not edited to make a refactor pass**, except mechanical updates a milestone names in advance (for example a renamed import).
- **The full gate passes on the final tree**, including the OS-level guard tests and the twelve rehearsal scenarios.
- One milestone at a time; each ends at its own gate and waits for a GO.
- No real Codex execution, no network, no credential access, no Git initialization, push or merge.

## Milestones

### R-01 — Typed protocol messages

- **Objective:** one definition of each message and payload, instead of strings and hand-written field lists.
- **Scope:** in `jojo-domain`, an enum of request types and typed payload structs with the exact field sets the helper accepts today, parsed once at the boundary. `service.rs` dispatches on the enum. The recovery assessment is sent as structured fields (blockers, process-absent flag, offered choices) **in addition to** the existing text, so that 1.1 clients keep working; the Swift app reads the structured fields. This is the one additive wire change, and it needs to be named in the authorization.
- **Acceptance:** every frame accepted or refused before is accepted or refused identically (the existing protocol tests are the oracle); the app no longer matches on debug text; the Swift and Rust field lists are checked against each other by a test.
- **Risk:** medium. Touches the helper's whole request path.
- **Depends on:** nothing.

### R-02 — Split the long functions

- **Objective:** make the supervision and request paths reviewable in pieces.
- **Scope:** `supervise` (message handling, stop decision, stop escalation, completion as separate functions over one explicit state struct); `spawn_guarded` (spool preparation, spawn, guard arming and verification, reader threads, reaper as separate functions); `handle` and `slice_command` (one function per command, which R-01 makes natural). Production settings (`inspector`, `guard`) move out of `LaunchHooks` into a launch configuration type; `LaunchHooks` keeps only test barriers and faults. Pure moves: no reordering of effects, no new behaviour.
- **Acceptance:** all 394 Rust tests and the rehearsal pass unchanged; no function in the four files above is longer than about 80 lines; the boundary guard's process-surface rules still hold without being loosened.
- **Risk:** medium to high. `supervise` and the reaper thread are ordering-sensitive; the existing OS-level tests are the safety net, and the milestone should be done in small steps with the guard tests run after each.
- **Depends on:** R-01 for the helper part; the storage part is independent.

### R-03 — One data-driven gate

- **Objective:** a gate that stays valid as the tree moves, and fails for product reasons only.
- **Scope:** one `scripts/verify.py` reading a per-milestone description (previous result, allowed changed files, expected inventories, coverage map, artifact prefix) from data files, replacing the import chain and the patched module variables. The earlier `verify_*.py` scripts and their results stay as historical records and are not run again. Harness hygiene carried over from P2-04.1: no path-spelling comparisons, and the gate refuses to start (or holds the machine awake) when the system may sleep during GUI scenarios.
- **Acceptance:** run on the P2-04.1 tree, the new gate reproduces the P2-04.1 result (same counts, same scenarios, same conditions); a deliberately broken tree fails it for each check class.
- **Risk:** low for the product, since no production source changes. The risk is a gate that checks less than before, which is what the reproduction requirement guards against.
- **Depends on:** nothing. It can be done first, and doing it first makes R-01 and R-02 cheaper to verify.

### Not planned

Splitting the large files further, moving SQL out of long inline lines and giving `jojo-storage` a structured public namespace are worthwhile but cosmetic next to the three above. They can follow R-02 if wanted, as their own milestone.

## Recommended order

R-03, then R-01, then R-02. The gate first, because every later step is verified by it; the protocol types second, because they shape how `service.rs` is split; the long functions last, because they carry the most risk and benefit most from the other two.
