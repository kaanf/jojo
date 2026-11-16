#!/usr/bin/env python3
"""T03: everything T01 and T02 verify, plus the P1-03 artifact/question/PRD scenarios.

Same conditions as T01/T02: temporary HOME/CARGO_HOME/target, credential-free environment
and an OS sandbox that denies all network access.
Run from any directory: python3 /absolute/path/to/scripts/verify_p1_03.py
"""
from pathlib import Path
import importlib.util
import re

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("verify_p1_02", ROOT / "scripts/verify_p1_02.py")
T02 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(T02)

# Brief T03 clause / P1-03 acceptance -> tests that must have run and passed.
SCENARIOS = {
    "1 complete sample passes structural validation": [
        "the_complete_sample_passes_structural_validation",
    ],
    "2 missing alternate/error flow or acceptance ID fails with field path": [
        "a_missing_alternate_flow_error_flow_or_acceptance_id_fails_with_its_field_path",
    ],
    "3 duplicate IDs rejected": [
        "duplicate_ids_are_rejected_with_the_path_of_both_definitions",
    ],
    "4 ambiguous JSON keys rejected": [
        "ambiguous_json_keys_are_rejected",
        "json_that_could_be_read_two_ways_is_not_read_at_all",
    ],
    "5 oversized and deep input rejected": [
        "oversized_and_deep_input_is_rejected_at_the_bound",
        "rejected_input_never_becomes_a_version",
    ],
    "6 answer survives restart": [
        "an_answer_survives_restart_with_its_provenance",
    ],
    "7 rendering round-trip preserves canonical revision/digests": [
        "the_canonical_form_is_independent_of_layout_and_round_trips_exactly",
        "a_rendering_round_trip_preserves_the_canonical_revision_and_digests",
    ],
    "8 provenance text retained": [
        "provenance_text_of_adapted_content_is_retained_verbatim",
    ],
    "9 edits create new versions; old versions unchanged": [
        "an_edit_creates_a_new_version_and_leaves_the_old_version_unchanged",
        "schema::tests::artifact_versions_questions_and_answers_cannot_be_rewritten_or_misordered",
    ],
    "10 exact reviewed bytes identifiable": [
        "a_rendering_round_trip_preserves_the_canonical_revision_and_digests",
        "artifacts::tests::a_version_altered_behind_the_schema_is_refused_on_read",
        "the_rendering_shows_every_field_of_the_document",
    ],
    "11 missing fields/placeholders/unanswered blocking questions prevent review readiness": [
        "placeholders_empty_fields_and_open_blockers_are_findings_with_paths",
        "unknown_missing_and_mistyped_members_are_refused_with_their_paths",
        "incomplete_content_and_unanswered_blocking_questions_prevent_review_readiness",
    ],
    "12 existing version-1 database upgrades with history intact": [
        "a_version_1_database_upgrades_with_a_backup_and_keeps_its_history",
    ],
}


def t03(run, tools, temp, test_output):
    result = T02.t02(run, tools, temp, test_output)
    passed = set(re.findall(r"^test (\S+) \.\.\. ok$", test_output, flags=re.M))
    for scenario, tests in SCENARIOS.items():
        missing = [name for name in tests if name not in passed]
        if missing:
            raise RuntimeError(f"T03 scenario {scenario!r} did not run and pass: {missing}")
    result["t03_scenarios"] = SCENARIOS
    return result


if __name__ == "__main__":
    T02.T01.guarded(
        "P1-03",
        scope="immutable artifact versions with content/rendered digests, persisted questions/answers with provenance, "
              "typed PRD/use-case import, canonical form, rendering and structural review readiness; "
              "no approvals/invalidation (P1-04), graphs, scheduler, adapters, IPC or GUI",
        extra=t03,
    )
