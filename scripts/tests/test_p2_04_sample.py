"""P2-04 sample generator and bundle: deterministic, confined and never approving."""
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("p2_04_sample", ROOT / "scripts/p2_04_sample.py")
SAMPLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SAMPLE)


class SampleRepositoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="jojo-p2-04-sample-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve() / "disposable"
        self.root.mkdir(mode=0o700)

    def test_same_inputs_give_the_same_single_commit(self):
        first = SAMPLE.generate_repository(self.root, "one")
        second = SAMPLE.generate_repository(self.root, "two")
        self.assertEqual(first["commit"], SAMPLE.SAMPLE_COMMIT)
        self.assertEqual(second["commit"], SAMPLE.SAMPLE_COMMIT)
        state = SAMPLE.repository_state(first["repository"])
        self.assertEqual(state, {"head": SAMPLE.SAMPLE_COMMIT, "branch": "main", "remotes": "", "status": "",
                                 "refs": f"refs/heads/main {SAMPLE.SAMPLE_COMMIT}"})
        self.assertEqual(SAMPLE.git(first["repository"], "rev-list", "--count", "HEAD"), "1")
        self.assertEqual(SAMPLE.git(first["repository"], "ls-files"), "slugify.py\ntest_slugify.py")

    def test_the_repository_holds_exactly_the_sample_files_and_no_hooks(self):
        repository = Path(SAMPLE.generate_repository(self.root)["repository"])
        self.assertEqual(sorted(p.name for p in repository.iterdir()), [".git", "slugify.py", "test_slugify.py"])
        for name in SAMPLE.FILES:
            self.assertEqual((repository / name).read_bytes(), (SAMPLE.SAMPLE / name).read_bytes())
        self.assertFalse((repository / ".git/hooks").exists())

    def test_an_existing_repository_is_never_overwritten(self):
        SAMPLE.generate_repository(self.root)
        with self.assertRaises(FileExistsError):
            SAMPLE.generate_repository(self.root)

    def test_a_root_that_is_not_private_is_refused(self):
        self.root.chmod(0o755)
        with self.assertRaises(ValueError):
            SAMPLE.generate_repository(self.root)

    def test_a_root_inside_a_project_is_refused(self):
        with self.assertRaises(ValueError):
            SAMPLE.check_disposable_root(ROOT / "scripts")
        (self.root / ".git").mkdir()
        inner = self.root / "inner"
        inner.mkdir(mode=0o700)
        with self.assertRaises(ValueError):
            SAMPLE.generate_repository(inner)

    def test_a_missing_relative_or_shallow_root_and_a_nested_name_are_refused(self):
        for root in [self.root / "missing", Path("relative"), Path("/tmp")]:
            with self.assertRaises(ValueError):
                SAMPLE.check_disposable_root(root)
        with self.assertRaises(ValueError):
            SAMPLE.generate_repository(self.root, "a/b")


class SampleBundleTests(unittest.TestCase):
    def test_validation_plan_is_the_approved_scope_and_command_in_canonical_form(self):
        self.assertEqual(
            SAMPLE.validation_plan(),
            '["jojo-validation-plan-v2",[["00000000000000000000000000000001","1",["slugify.py"],["test_slugify.py"],'
            '["unittest"],["/usr/bin/python3","-m","unittest","-q"]]]]')

    def test_payload_has_exactly_the_import_fields_and_names_no_executable(self):
        payload = SAMPLE.import_payload("ab" * 16, SAMPLE.SAMPLE_COMMIT)
        self.assertEqual(sorted(payload), ["adr", "adr_id", "base", "feature", "plan_id", "prd", "prd_id", "provider",
                                           "repository", "step", "validation", "validation_id"])
        self.assertEqual(payload["provider"], "Codex")
        self.assertEqual(len({payload[k] for k in ["feature", "prd_id", "adr_id", "plan_id", "validation_id"]}), 5)
        self.assertEqual(json.loads(payload["prd"])["title"], "Slugify")

    def test_the_live_bundle_has_no_fixture_directive_and_the_rehearsal_bundle_has_one(self):
        live = SAMPLE.import_payload("ab" * 16, SAMPLE.SAMPLE_COMMIT)
        self.assertNotIn("fixture:", live["adr"] + live["prd"])
        self.assertNotIn("Rehearsal", live["adr"])
        rehearsal = SAMPLE.import_payload("ab" * 16, SAMPLE.SAMPLE_COMMIT, "implement-slugify")
        self.assertEqual([l for l in rehearsal["adr"].splitlines() if l.startswith("fixture: ")],
                         ["fixture: implement-slugify"])
        self.assertTrue(rehearsal["adr"].startswith(live["adr"]))

    def test_the_stub_fails_its_tests_and_the_tests_name_only_the_standard_library(self):
        stub = (SAMPLE.SAMPLE / "slugify.py").read_text()
        self.assertIn("raise NotImplementedError", stub)
        tests = (SAMPLE.SAMPLE / "test_slugify.py").read_text()
        self.assertEqual(sorted(l for l in tests.splitlines() if l.startswith(("import ", "from "))),
                         ["from slugify import slugify", "import unittest"])
        self.assertEqual(tests.count("    def test_"), 8)


if __name__ == "__main__":
    unittest.main()
