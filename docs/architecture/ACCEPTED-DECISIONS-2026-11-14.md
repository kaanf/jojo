# Jojo — Accepted U01–U05 decisions

Status: **ACCEPTED by explicit user instruction in “Jojo — Phase 1 / P1-01 Implementation”.**
Recorded 2026-11-14. This is an append-only acceptance record, not an edit of historical
[ADR drafts](ADR-DRAFTS.md) or the [Phase 1 brief](../planning/PHASE_1_IMPLEMENTATION_BRIEF.md).
The user's latest instruction supersedes their historical “awaiting approval” wording
for the exact decisions below. No inference or application-feature approval is implied.

| Decision | Explicitly accepted recommendation | Relevant draft clauses |
|---|---|---|
| U01 | Phase 1 is an offline deterministic foundation with fake providers. **Only P1-01 may be implemented in this turn**; P1-02+ require the user's next instruction. | D01/D05/D08 foundation direction |
| U02 | Closing the last primary window triggers application quit; auxiliary windows close independently. | D02 |
| U03 | Private macOS Application Support data root and separate owned worktree root; never automatically delete interrupted or uncertain work. | D03 |
| U04 | Conservative revision/hash invalidation; stop affected work, preserve history and require re-review. | D05/D06 |
| U05 | Trusted local repositories; initial agent capacity one, configurable parallelism; deterministic fairness; controlled interrupt → TERM → KILL with verified stop and visible failure. | D07/D09 |

Accepted recommended policy details remain those in the normative contracts: global,
per-provider and initial per-repository slots one; round-robin/aging/drain fairness;
10-second graceful interruption, 20 additional seconds for TERM and 5 seconds for
KILL verification. These are policy inputs, not an implemented scheduler/stop engine
or a guarantee that arbitrary child processes terminate. U05 does not authorize any
process launch, signal or provider work during P1-01.

U06–U08 remain **UNRESOLVED / NOT AUTHORIZED**: real CLI compatibility/auth/billing-mode
assurance, first live provider/sample, live Figma/SCM/deployment/distribution or stronger
isolation decisions. No general “D01–D10 accepted” label is applied to unrelated details.
Already-confirmed SwiftUI/Rust-helper/SQLite/native-auth/no-remote constraints remain binding.

P1-01 chooses the routine path names `~/Library/Application Support/Jojo` (data) and
`~/Library/Application Support/JojoWorktrees` (separate worktree root), with explicit
absolute overrides for tests. It only represents these paths. Directory creation,
0700 permissions, ownership/symlink validation, retention enforcement, SQLite, leases,
approval evaluation, process supervision, IPC and actual window behavior remain later tasks.
No accepted invariant is weakened by those scope limits.
