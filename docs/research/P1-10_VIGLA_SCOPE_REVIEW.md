# P1-10 — Vigla changed-files scope review and independent reimplementation

Reviewed the local Git checkout of [Kilbex/Vigla](https://github.com/Kilbex/Vigla) at the existing pin **`bbd19ae2d5a77401502756c550b5dcd4ae59bbc9`** (commit date 2026-07-27T07:03:10-07:00; the reviewed `acl/` and dispatch files last changed in `1e19e88ca56f2b2270c8aecb55e730ddc6582463`, 2026-07-22). Every file below was byte-compared with `git show bbd19ae2d5a77401502756c550b5dcd4ae59bbc9:<path>`; the checkout has no local modifications. Nothing was fetched, built or executed. SHA-256 values are in [REPOSITORY_PINS.json](../../research/REPOSITORY_PINS.json), `Vigla.p1_10_review.sources`.

## Upstream source paths and functions

| Path | Symbols | Role |
| --- | --- | --- |
| [`crates/orchestrator/src/acl/check.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/acl/check.rs) | `check_diff`, `AclViolation` (`summary`, `payload_json`) | Pure gate: every submitted path must be allowed or the violation lists denied paths |
| [`crates/orchestrator/src/acl/file_acl.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/acl/file_acl.rs) | `FileAcl::from_mission_and_task`, `is_unconstrained`, `is_path_allowed`, `intersect` | Effective allow-list: mission ∩ task prefixes; `Path::starts_with` component matching |
| [`crates/orchestrator/src/acl/sentinel.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/acl/sentinel.rs) | `write_sentinel`, `read_sentinel` | Informational `.vigla/acl.json` in the worktree; missing means unconstrained |
| [`crates/orchestrator/src/acl/README.md`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/acl/README.md), `acl/mod.rs` | design notes, re-exports | Gate runs before audit; prefix-only, no globs, no read isolation |
| [`crates/orchestrator/src/mission_worker_dispatch.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/mission_worker_dispatch.rs) | `commit_and_synthesize`, `git_files_in_head_commit`, `WorkerSubmission` | Orchestrator runs `git add -A`, commits on the worker's behalf, lists files with `git diff-tree --no-commit-id --name-only -r HEAD` |
| [`crates/orchestrator/src/mission_supervisor_run/run_task.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/mission_supervisor_run/run_task.rs) | `acl_preflight` | Violation becomes a synthetic `Escalate { bound: Scope }` decision |
| [`crates/orchestrator/src/audit/scope.rs`](https://github.com/Kilbex/Vigla/blob/bbd19ae2d5a77401502756c550b5dcd4ae59bbc9/crates/orchestrator/src/audit/scope.rs) | `score_scope` | Slower scoring backup with the same prefix semantics |

**License.** `crates/orchestrator/Cargo.toml` declares `license.workspace = true`; root `Cargo.toml` declares `Apache-2.0`; root `LICENSE` (Apache 2.0) and `NOTICE` (Copyright 2026 Kilbex and Vigla contributors) hashes are unchanged in the pin record. No reviewed file carries an overriding header.

## What the upstream approach does well

1. The file list comes from the orchestrator's own Git command after the worker exits, not from the worker's narrative. The worker prompt is explicitly told not to run Git.
2. Matching is component-wise (`Path::starts_with`), so `src` does not admit `src2/...`.
3. The gate is a small pure function over a path list and an allow-list. It runs before slower review.
4. An empty task/mission intersection is deny-all, not allow-all.

## Gaps relative to Jojo's evidence rules

| Concern | Vigla at this pin | Consequence for Jojo |
| --- | --- | --- |
| No declared scope | `FileAcl` is *unconstrained*; `score_scope` treats empty scope as all in scope; a missing sentinel reads as unconstrained | Fail-open. Jojo has no unconstrained mode: an empty write scope is an invalid plan |
| Change capture | `git add -A` then `diff-tree HEAD`: ignored files never appear; a parentless commit would list nothing without `--root`; the orchestrator mutates the worktree by committing | Jojo blocks any ignored file (candidate capture refuses), never commits or stages, and diffs the frozen base commit against the working tree plus untracked files |
| Path encoding | `--name-only` without `-z`, lossy UTF-8, trimmed lines | Quoted or whitespace paths can misparse. Jojo uses `--raw -z`, rejects non-UTF-8 and non-normal paths |
| Renames, modes, types | Name-only; mode-only changes and type changes are not distinguished; rename detection follows Git defaults | Jojo uses `--no-renames` (both sides must be in scope), records old/new modes and kinds, and rejects submodule gitlinks and unknown statuses |
| Authority of the scope | Live gate uses the in-memory mission/task scope; the worktree sentinel is informational, and replay readers treat a missing one as unconstrained | Jojo's scope is part of the human-reviewed validation plan whose digest is bound in the approved execution plan |
| Protected files | Only "outside scope" exists | Jojo adds baseline paths (tests/fixtures) that must stay byte-identical; overlap with write scope is refused as ambiguous ownership |
| Binding | The list is not tied to a durable candidate digest | Jojo re-captures the worktree before and after inspection and requires both to reproduce the sealed candidate digest |

## Comparison with Jojo's existing bindings

- **Source bindings (P1-05/P1-07).** The base commit compared against is the frozen, exact source-vector commit recorded with the owned worktree, not a branch tip or `HEAD~1`. If the worker moves `HEAD`, the diff still compares the whole tree against the approved base.
- **Approvals (P1-04).** The write scope, baseline and expected test inventory live in a canonical validation plan; its SHA-256 must equal `ExecutionPlan::validation_digest` of the attempt's approved plan revision. A receipt also records the current development approval digest; revocation or plan change makes validation fail and acceptance stale.
- **Worktree ownership (P1-07/P1-09).** Inspection requires the current epoch, the ownership marker/gitfile/common-dir/branch checks, and no live `worktree:<attempt>` claim. That claim is released only after the database proves `TREE_ABSENT`. Old-epoch worktrees need recovery inspection. Nothing is cleaned, reset, committed or overwritten.

## Reimplemented technique

Only the idea of a post-run, orchestrator-derived changed-path gate with component-wise prefix matching was reimplemented, independently, in `jojo-domain::validation` (`ScopePath`, `parse_raw_diff`, `check_scope`) and `jojo-storage::worktrees::inspect_candidate_changes`. No Vigla source text, test, fixture, sentinel format, commit-on-behalf behaviour, Tokio/serde usage or dependency was copied or added. The local Git argv uses the existing closed P1-07 `git` surface (`diff`, `ls-files`, `rev-parse`), with hooks, fsmonitor, external diff and textconv disabled.
