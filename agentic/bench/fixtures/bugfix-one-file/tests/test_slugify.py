import unittest

from slugs.slugify import slugify


class SlugifyTest(unittest.TestCase):
    def test_a_blank_title_has_an_empty_slug(self):
        self.assertEqual(slugify("   "), "")
