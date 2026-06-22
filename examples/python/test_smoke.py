import os
import sys
import unittest

from l10n import RELEASES_URL, Translator

TEST_DIR = os.path.dirname(os.path.abspath(__file__))
EXAMPLES_DIR = os.path.join(TEST_DIR, "..")
DIST_LOCALES = os.path.join(EXAMPLES_DIR, "dist", "locales")


class TestL10n4cSmoke(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        verify_hex = os.environ.get("L10N4X_VERIFY_PUBLIC_KEY")
        if not verify_hex:
            raise unittest.SkipTest("L10N4X_VERIFY_PUBLIC_KEY not set")
        cls.verify_hex = verify_hex
        cls.pak_dir = os.environ.get("L10N4X_PAK_DIR", DIST_LOCALES)

    def setUp(self):
        self.tr = Translator()
        self.tr.set_verify_key(bytes.fromhex(self.verify_hex))
        self.tr.set_fallback_locale("es")
        self.tr.load_pak_directory(self.pak_dir)

    def tearDown(self):
        self.tr.clear()

    def test_spanish_welcome(self):
        result = self.tr.translate("es", "common.welcome")
        self.assertIn("Bienvenido", result)

    def test_english_welcome_buffered(self):
        result = self.tr.translate_buffered("en", "common.welcome")
        self.assertIn("Welcome", result)

    def test_english_greet_with_params(self):
        result = self.tr.translate("en", "common.greet", params={"name": "World"})
        self.assertIn("World", result)

    def test_fallback_to_spanish(self):
        result = self.tr.translate("xx", "common.welcome")
        self.assertIn("Bienvenido", result)

    def test_missing_key_returns_key(self):
        result = self.tr.translate("en", "nonexistent.key")
        self.assertEqual(result, "nonexistent.key")


if __name__ == "__main__":
    unittest.main()
