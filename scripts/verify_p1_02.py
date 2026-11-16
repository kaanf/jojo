#!/usr/bin/env python3
"""T02: everything T01 verifies, plus the P1-02 persistence/ownership scenarios.

Same conditions as T01: temporary HOME/CARGO_HOME/target, credential-free environment and
an OS sandbox that denies all network access. The only programs launched by the Rust tests
are the test binaries themselves and this workspace's `jojo-helper`.
Run from any directory: python3 /absolute/path/to/scripts/verify_p1_02.py
"""
from pathlib import Path
import importlib.util
import re
import stat

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("verify_p1_01", ROOT / "scripts/verify_p1_01.py")
T01 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(T01)

# Mandatory scenario -> tests that must have run and passed (not merely exist).
SCENARIOS = {
    "1 same-revision concurrent updates: one winner": [
        "concurrent_updates_with_the_same_expected_revision_have_exactly_one_winner",
        "writer::tests::two_connections_racing_on_one_revision_yield_exactly_one_winner_every_round",
    ],
    "2 replay returns original receipt, no repeated effects": [
        "replaying_a_command_returns_the_original_receipt_without_repeating_effects",
        "concurrent_duplicate_commands_commit_one_logical_operation",
        "writer::tests::two_connections_submitting_one_command_run_its_effects_once",
    ],
    "3 command ID reuse with different payload fails": [
        "reusing_a_command_id_with_a_different_request_is_an_explicit_conflict",
    ],
    "4 forced failure leaves no partial state/event/outbox/claim/receipt": [
        "every_kind_of_forced_failure_rolls_back_all_mutations_and_the_receipt",
        "a_panicking_handler_rolls_back_and_stops_the_writer",
        "a_helper_killed_inside_a_transaction_leaves_only_whole_committed_commands",
    ],
    "5 second helper denied while first holds ownership": [
        "a_second_helper_is_denied_while_the_first_owns_the_database",
        "ownership_is_exclusive_across_processes_and_is_freed_by_death_not_by_time",
        "second_helper_is_denied_and_a_replacement_after_sigkill_starts_a_new_closed_epoch",
    ],
    "6 replacement helper starts a new epoch after owner death": [
        "a_replacement_starts_a_new_epoch_with_admission_closed_and_quarantines_old_claims",
        "second_helper_is_denied_and_a_replacement_after_sigkill_starts_a_new_closed_epoch",
    ],
    "7 admission closed on startup and after crash recovery": [
        "a_replacement_starts_a_new_epoch_with_admission_closed_and_quarantines_old_claims",
        "a_clean_shutdown_is_recorded_and_the_next_start_is_still_closed",
        "schema::tests::engine_sessions_start_closed_in_order_and_are_immutable_once_sealed",
    ],
    "8 migration failure preserves usable prior database and backup": [
        "a_failed_migration_preserves_the_prior_database_and_a_recoverable_backup",
        "a_multi_step_upgrade_stops_at_the_last_whole_version",
    ],
    "9 snapshot and event cursor are consistent": [
        "a_commit_during_a_read_is_invisible_to_it_and_is_the_next_event_after_its_cursor",
        "every_snapshot_taken_under_concurrent_writes_matches_the_log_at_its_cursor",
    ],
    "10 stale-epoch mutations rejected": [
        "a_command_addressed_to_an_old_epoch_is_rejected_without_effect",
        "a_superseded_engine_cannot_mutate_state_even_if_the_os_lock_was_bypassed",
    ],
}


def t02(run, tools, temp, test_output):
    passed = set(re.findall(r"^test (\S+) \.\.\. ok$", test_output, flags=re.M))
    for scenario, tests in SCENARIOS.items():
        missing = [name for name in tests if name not in passed]
        if missing:
            raise RuntimeError(f"T02 scenario {scenario!r} did not run and pass: {missing}")
    # Cargo names each test binary on stderr and libtest prints each summary on stdout, in
    # the same order; the captured output is stdout followed by stderr.
    targets = re.findall(r"^\s+Running (.+?) \(", test_output, flags=re.M)
    summaries = re.findall(r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", test_output, flags=re.M)
    if not targets or len(targets) != len(summaries):
        raise RuntimeError(f"{len(targets)} test binaries but {len(summaries)} summaries")
    binaries = [
        {"target": target, "status": status, "passed": int(ok), "failed": int(failed), "ignored": int(ignored)}
        for target, (status, ok, failed, ignored) in zip(targets, summaries)
    ]
    totals = {key: sum(b[key] for b in binaries) for key in ("passed", "failed", "ignored")}
    if totals["failed"] or any(b["status"] != "ok" for b in binaries):
        raise RuntimeError(f"Rust test failures: {totals}")

    # The real helper: own a fresh private data root, shut down cleanly on stdin EOF, twice.
    binary = temp / "target/debug/jojo-helper"
    data, worktrees = temp / "probe-data", temp / "probe-worktrees"
    probe = [binary, "--engine-probe", "--data-root", data, "--worktree-root", worktrees]
    version = run("helper version and SQLite library", [binary, "--version"]).strip()
    run("helper engine probe, first epoch", probe)
    first = run.last_output
    run("helper engine probe, second epoch", probe)
    second = run.last_output
    if "epoch=1 schema=6 admission=closed previous=none" not in first or "clean shutdown recorded" not in first:
        raise RuntimeError(f"unexpected first probe output: {first}")
    if "epoch=2 schema=6 admission=closed previous=clean" not in second:
        raise RuntimeError(f"unexpected second probe output: {second}")
    if stat.S_IMODE(data.stat().st_mode) != 0o700 or not (data / "jojo.sqlite3").exists() or worktrees.exists():
        raise RuntimeError("engine probe did not leave exactly a private data root with the database")
    return {
        "helper_version": version,
        "rust_tests": {"totals": totals, "binaries": binaries},
        "t02_scenarios": SCENARIOS,
        "live_operations": "none: network denied by OS sandbox for every check; no provider CLI, remote or inference call; "
                           "child processes limited to test binaries and the workspace jojo-helper",
    }


if __name__ == "__main__":
    T01.guarded(
        "P1-02",
        scope="SQLite writer, migrations, CAS, command receipts, events, claims/outbox primitives, OS ownership and "
              "engine epochs; no scheduler/approvals/adapters/process manager/IPC/GUI lifecycle",
        extra=t02,
    )
