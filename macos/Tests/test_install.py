"""Installer integration tests in a disposable directory; never touch the user's input sources."""
import pathlib
import os
import plistlib
import shutil
import subprocess
import tempfile
import unittest

INSTALL = pathlib.Path(__file__).resolve().parents[1] / 'Packaging/Install.command'


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='hanautomata-install-test-')
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        self.source = self.root / 'source/Hanautomata.app'
        self.dest = self.root / 'Input Methods'
        self.backups = self.root / 'Backups'
        contents = self.source / 'Contents'
        (contents / 'MacOS').mkdir(parents=True)
        shutil.copy('/usr/bin/true', contents / 'MacOS/Hanautomata')
        with (contents / 'Info.plist').open('wb') as stream:
            plistlib.dump(dict(CFBundleIdentifier='io.github.peterkimpmp.inputmethod.Hanautomata',
                              CFBundleExecutable='Hanautomata', CFBundleVersion='1',
                              CFBundlePackageType='APPL', CFBundleShortVersionString='0.6.1'), stream)
        self.sign()

    def sign(self):
        subprocess.run(['codesign', '--force', '--sign', '-', str(self.source)], check=True, capture_output=True)

    def install(self, env=None, activate=False):
        args = ['/bin/sh', str(INSTALL), '--app', str(self.source), '--destination', str(self.dest), '--backups', str(self.backups)]
        if not activate:
            args.append('--no-activate')
        return subprocess.run(args, text=True, capture_output=True, env=env)

    def test_first_install_and_backup_upgrade(self):
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        original = (self.dest / 'Hanautomata.app/Contents/Info.plist').read_bytes()
        (self.source / 'Contents/Resources').mkdir()
        (self.source / 'Contents/Resources/new.txt').write_text('new build')
        self.sign()
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.dest / 'Hanautomata.app/Contents/Resources/new.txt').is_file())
        saved = list(self.backups.glob('*/Hanautomata.app/Contents/Info.plist'))
        self.assertEqual(len(saved), 1)
        self.assertEqual(saved[0].read_bytes(), original)
        self.assertFalse((saved[0].parent / 'Resources/new.txt').exists())

    def test_corrupt_source_keeps_installed_copy(self):
        self.assertEqual(self.install().returncode, 0)
        (self.source / 'Contents/Info.plist').write_text('broken seal')
        self.assertNotEqual(self.install().returncode, 0)
        self.assertTrue((self.dest / 'Hanautomata.app/Contents/MacOS/Hanautomata').is_file())
        self.assertFalse(self.backups.exists())

    def test_missing_proofreading_refuses_feature_loss(self):
        self.assertEqual(self.install().returncode, 0)
        payload = self.dest / 'Hanautomata.app/Contents/Resources/proofreading'
        payload.mkdir(parents=True)
        (payload / 'keep.txt').write_text('existing optional payload')
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('proofreading', result.stderr)
        self.assertEqual((payload / 'keep.txt').read_text(), 'existing optional payload')

    def test_failed_publish_restores_backup(self):
        self.assertEqual(self.install().returncode, 0)
        original = (self.dest / 'Hanautomata.app/Contents/Info.plist').read_bytes()
        stubs = self.root / 'stubs'
        stubs.mkdir()
        move = stubs / 'mv'
        move.write_text('#!/bin/sh\ncase "$1" in */.hanautomata-stage.*/Hanautomata.app) exit 73;; esac\nexec /bin/mv "$@"\n')
        move.chmod(0o755)
        result = self.install(dict(os.environ, PATH=str(stubs) + os.pathsep + os.environ['PATH']))
        self.assertEqual(result.returncode, 73, result.stderr)
        self.assertEqual((self.dest / 'Hanautomata.app/Contents/Info.plist').read_bytes(), original)
        self.assertIn('restored', result.stderr)
        self.assertFalse(list(self.root.glob('.hanautomata-stage.*')))

    def test_concurrent_install_cannot_move_current_bundle(self):
        self.assertEqual(self.install().returncode, 0)
        (self.dest / '.hanautomata-install.lock').mkdir()
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('installation', result.stderr)
        self.assertFalse(self.backups.exists())

    def test_incomplete_proofreading_is_not_a_preserved_feature(self):
        self.assertEqual(self.install().returncode, 0)
        (self.dest / 'Hanautomata.app/Contents/Resources/proofreading').mkdir(parents=True)
        (self.source / 'Contents/Resources/proofreading').mkdir(parents=True)
        self.sign()
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('proofreading', result.stderr)
        self.assertFalse(self.backups.exists())

    def test_preservation_rechecks_current_app_after_acquiring_lock(self):
        self.assertEqual(self.install().returncode, 0)
        stubs = self.root / 'stubs'
        stubs.mkdir()
        make = stubs / 'mkdir'
        make.write_text('#!/bin/sh\ncase "$1" in */.hanautomata-install.lock)\n'
                        ' /bin/mkdir -p "${1%/.hanautomata-install.lock}/Hanautomata.app/Contents/Resources/proofreading";;\n'
                        'esac\nexec /bin/mkdir "$@"\n')
        make.chmod(0o755)
        result = self.install(dict(os.environ, PATH=str(stubs) + os.pathsep + os.environ['PATH']))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('proofreading', result.stderr)
        self.assertTrue((self.dest / 'Hanautomata.app/Contents/Resources/proofreading').is_dir())

    def test_interrupted_activation_does_not_undo_verified_install(self):
        self.assertEqual(self.install().returncode, 0)
        (self.source / 'Contents/Resources').mkdir()
        (self.source / 'Contents/Resources/new.txt').write_text('new build')
        self.sign()
        stubs = self.root / 'stubs'
        stubs.mkdir()
        for name, body in [('pgrep', 'exit 1'), ('open', 'kill -TERM "$PPID"\nexit 0')]:
            stub = stubs / name
            stub.write_text('#!/bin/sh\n' + body + '\n')
            stub.chmod(0o755)
        result = self.install(dict(os.environ, PATH=str(stubs) + os.pathsep + os.environ['PATH']), activate=True)
        self.assertEqual(result.returncode, 130, result.stderr)
        self.assertTrue((self.dest / 'Hanautomata.app/Contents/Resources/new.txt').is_file())
        self.assertFalse((self.dest / '.hanautomata-install.lock').exists())
        self.assertEqual(len(list(self.backups.glob('*/Hanautomata.app'))), 1)


if __name__ == '__main__':
    unittest.main()
