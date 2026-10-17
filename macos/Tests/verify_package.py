#!/usr/bin/env python3
"""Audit the extracted distributable, not the build directory. Never registers or starts the input method."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import plistlib
import subprocess
import tempfile
import zipfile


def run(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def verify(archive, proofreading):
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    expected, filename = Path(str(archive) + '.sha256').read_text().strip().split(None, 1)
    assert (digest, archive.name) == (expected, filename), 'archive checksum/name mismatch'
    with zipfile.ZipFile(archive) as z:
        for item in z.infolist():
            name = PurePosixPath(item.filename)
            assert not name.is_absolute() and '..' not in name.parts, 'unsafe archive path'
    with tempfile.TemporaryDirectory(prefix='hanautomata-zip-audit-') as temp:
        run('ditto', '-x', '-k', str(archive), temp)
        root = Path(temp)
        for name in ['LICENSE', 'NOTICE.md', 'THIRD-PARTY-NOTICES.md', 'PRIVACY.md', 'Install.command', 'README.txt']:
            assert (root / name).is_file(), 'missing ' + name
        app = root / 'Hanautomata.app'
        with (app / 'Contents/Info.plist').open('rb') as stream:
            info = plistlib.load(stream)
        version = info['CFBundleShortVersionString']
        assert archive.name == 'Hanautomata-' + version + '-macos.zip'
        assert info['CFBundleIdentifier'] == 'io.github.peterkimpmp.inputmethod.Hanautomata'
        run('codesign', '--verify', '--deep', '--strict', str(app))
        assert set(run('lipo', '-archs', str(app / 'Contents/MacOS/Hanautomata')).split()) == {'arm64', 'x86_64'}
        payload_count = 0
        if proofreading:
            payload = app / 'Contents/Resources/proofreading'
            helper = payload / 'hanautomata-proofread'
            assert set(run('lipo', '-archs', str(helper)).split()) == {'arm64', 'x86_64'}
            manifest = json.loads((payload / 'MANIFEST.json').read_text())
            assert set(manifest['targets']) == {'aarch64-apple-darwin', 'x86_64-apple-darwin'}
            files = {p.relative_to(payload).as_posix() for p in payload.rglob('*') if p.is_file() and p.name != 'MANIFEST.json'}
            assert files == set(manifest['files']), 'payload missing/unlisted files'
            for name, metadata in manifest['files'].items():
                data = (payload / name).read_bytes()
                assert len(data) == metadata['bytes'] and hashlib.sha256(data).hexdigest() == metadata['sha256'], name
            payload_count = len(files)
            result = run(str(helper), '--dictionary', str(payload / 'data'), '--rules', input='되요')
            assert '돼요' in result, 'extracted checker failed known correction'
    print(json.dumps(dict(version=version, sha256=digest, architectures=['arm64', 'x86_64'],
                          signature='verified (trust/notarization not implied)', proofreading_files=payload_count), ensure_ascii=False))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('--with-proofreading', action='store_true')
    args = parser.parse_args()
    verify(args.archive.resolve(), args.with_proofreading)
