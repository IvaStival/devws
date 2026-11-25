import unittest

from slugs.slugify import slugify


class AcceptanceTest(unittest.TestCase):
    def test_every_word_is_kept(self):
        self.assertEqual(slugify("Hello Big World"), "hello-big-world")

    def test_a_single_word(self):
        self.assertEqual(slugify("Notes"), "notes")

    def test_spaces_and_case_are_normalised(self):
        self.assertEqual(slugify("  Mixed   CASE title "), "mixed-case-title")
