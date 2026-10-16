"""Failure-path tests for the data acquisition boundary; no downloads."""
import tempfile
import unittest
from pathlib import Path
from prepare_dictionary import prepare


class PreparationTests(unittest.TestCase):
    def test_bad_archive_leaves_existing_data_untouched(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            output = root / "data"
            output.mkdir()
            (output / "ko.aff").write_bytes(b"keep this")
            archive = root / "wrong.zip"
            archive.write_bytes(b"not the pinned release")
            with self.assertRaises(ValueError):
                prepare(output, archive)
            self.assertEqual((output / "ko.aff").read_bytes(), b"keep this")
            self.assertEqual(list(output.iterdir()), [output / "ko.aff"])

    def test_missing_archive_does_not_create_output(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            with self.assertRaises(OSError):
                prepare(root / "output", root / "missing.zip")
            self.assertFalse((root / "output").exists())


if __name__ == "__main__":
    unittest.main()
