import unittest
from write_notices import runtime_keys


class NoticeTests(unittest.TestCase):
    def test_last_dependency_with_trailing_newline_is_not_lost(self):
        tree = "local v1.0.0 (/tmp/local)\nhunspell-sys v0.3.1\nhunspell-sys v0.3.1 (*)\ntinyvec v1.13.3\n"
        self.assertEqual(runtime_keys(tree), {("hunspell-sys", "0.3.1"), ("tinyvec", "1.13.3")})


if __name__ == "__main__":
    unittest.main()
