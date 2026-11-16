"""Acceptance tests for slugify(). Baseline: the task must not change this file."""
import unittest

from slugify import slugify


class SlugifyTest(unittest.TestCase):
    def test_lower_cases_and_joins_words_with_hyphens(self):
        self.assertEqual(slugify("Hello World"), "hello-world")

    def test_collapses_punctuation_and_whitespace_into_one_hyphen(self):
        self.assertEqual(slugify("Rock & Roll:  the   sequel!"), "rock-roll-the-sequel")

    def test_strips_leading_and_trailing_separators(self):
        self.assertEqual(slugify("  --Already-Slugged--  "), "already-slugged")

    def test_keeps_digits(self):
        self.assertEqual(slugify("Version 2.0 (beta 3)"), "version-2-0-beta-3")

    def test_removes_accents(self):
        self.assertEqual(slugify("Crème Brûlée à la carte"), "creme-brulee-a-la-carte")

    def test_drops_characters_without_an_ascii_form(self):
        self.assertEqual(slugify("日本語 text"), "text")

    def test_empty_and_separator_only_input_gives_an_empty_slug(self):
        self.assertEqual(slugify(""), "")
        self.assertEqual(slugify(" -_- "), "")

    def test_rejects_values_that_are_not_strings(self):
        with self.assertRaises(TypeError):
            slugify(None)
        with self.assertRaises(TypeError):
            slugify(42)


if __name__ == "__main__":
    unittest.main()
