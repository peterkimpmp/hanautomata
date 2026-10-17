import unittest
from unittest.mock import patch
from package import validate_targets


class UniversalNoticesTests(unittest.TestCase):
    @patch('package.cargo', side_effect=['libc v0.2.190\nserde v1.0.229\n', 'serde v1.0.229\nlibc v0.2.190\n'])
    def test_identical_runtime_dependencies(self, cargo):
        self.assertEqual(validate_targets('aarch64-apple-darwin', ['x86_64-apple-darwin']),
                         ['aarch64-apple-darwin', 'x86_64-apple-darwin'])

    @patch('package.cargo', side_effect=['libc v0.2.190\n', 'libc v0.2.190\nextra v1.0.0\n'])
    def test_new_target_cannot_silently_drop_a_license(self, cargo):
        with self.assertRaisesRegex(ValueError, 'different runtime dependencies'):
            validate_targets('arm', ['intel'])


if __name__ == '__main__':
    unittest.main()
