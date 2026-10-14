#!/usr/bin/env python3
"""Exercise only an explicitly supplied development driver and synthetic temporary data."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time

if sys.flags.optimize:
    raise RuntimeError('This verification harness requires Python assertions; do not use -O or PYTHONOPTIMIZE.')


class Driver:
    def __init__(self, exe, path):
        self.process = subprocess.Popen(
            [str(exe), '--development-stdio', '--store', str(path)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding='utf-8', creationflags=subprocess.CREATE_NO_WINDOW,
        )
        self.lines = queue.Queue()
        self.reader = threading.Thread(target=self.read_lines, daemon=True)
        self.reader.start()

    def read_lines(self):
        try:
            while True:
                line = self.process.stdout.readline(2_097_153)
                if not line:
                    break
                self.lines.put(line)
                if len(line) > 2_097_152:
                    break
        finally:
            self.lines.put(None)

    def raw(self, line):
        self.process.stdin.write(line + '\n')
        self.process.stdin.flush()
        response = self.lines.get(timeout=5)
        assert response is not None, 'driver ended before replying'
        assert len(response) <= 2_097_152, 'unbounded driver response'
        return json.loads(response)

    def call(self, command, **fields):
        return self.raw(json.dumps({'command': command, **fields}, ensure_ascii=False))

    def close(self, expected=None):
        if not self.process.stdin.closed:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                pass
        try:
            code = self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()  # only the child created by this fixture
            self.process.wait(timeout=5)
            raise AssertionError('owned driver did not exit')
        self.reader.join(timeout=2)
        assert not self.reader.is_alive(), 'response reader did not exit'
        error = self.process.stderr.read(4096)
        self.process.stdout.close()
        self.process.stderr.close()
        if expected is not None:
            assert code == expected, (code, error)
        return code, error


def request(lease, sequence, generation, operation):
    return dict(store_id=lease['version']['store_id'], client_id=lease['client_id'],
                lease_revision=lease['lease_revision'], sequence=sequence,
                generation=generation, operation=operation)


def choice(raw, korean=True):
    return dict(operation='learn', raw=raw, keys=raw, korean=korean, context='')


def key(lease):
    return {name: lease[name] for name in ('client_id', 'lease_revision')}


def exercise(exe, root, passed):
    path = root / '학습 데이터.db'
    children = []

    def start():
        driver = Driver(exe, path)
        children.append(driver)
        return driver

    def error(response, code):
        assert response == {'ok': False, 'error': code}, response

    try:
        owner = start()
        assert owner.call('snapshot')['snapshot']['entries'] == []
        error(owner.call('register', extra='reject'), 'invalid_request')
        passed.append('strict commands, empty dedicated Unicode-path database')
        alice = owner.call('register')['lease']
        bob = owner.call('register')['lease']
        a1 = request(alice, 1, 1, choice('hello'))
        b1 = request(bob, 1, 1, choice('hello', False))
        assert owner.call('apply', request=a1)['ok']
        assert owner.call('apply', request=b1)['ok']
        entry = owner.call('snapshot')['snapshot']['entries'][0]
        assert (entry['korean_weight'], entry['english_weight']) == (0.85, 1.0)
        disk = path.read_bytes()
        assert owner.call('apply', request=a1)['reply']['duplicate']
        assert path.read_bytes() == disk
        passed.append('independent clients and byte-preserving duplicate reply')

        contender = start()
        _, diagnostic = contender.close(2)
        assert 'Busy' in diagnostic
        passed.append('separate process denied while first owner is alive')
        reset = request(bob, 2, 1, {'operation': 'reset'})
        version = owner.call('apply', request=reset)['reply']['version']
        error(owner.call('apply', request=request(alice, 2, 1, choice('world'))), 'stale_generation')
        owner.close(0)
        owner = start()
        assert owner.call('apply', request=a1)['reply']['duplicate']
        assert owner.call('snapshot')['snapshot']['entries'] == []
        assert owner.call('apply', request=reset)['reply']['duplicate']
        resumed = owner.call('resume', key=key(alice))['lease']
        assert resumed['next_sequence'] == 2
        assert resumed['lease_revision'] == alice['lease_revision']
        passed.append('restart, reset retry and old duplicate never resurrect words')

        a2 = request(alice, 2, version['generation'], choice('world'))
        assert owner.call('apply', request=a2)['ok']
        error(owner.call('apply', request=a1), 'superseded')
        forget = request(bob, 3, version['generation'], dict(operation='forget', raw='world', keys='world'))
        assert owner.call('apply', request=forget)['ok']
        assert owner.call('apply', request=request(alice, 3, version['generation'], choice('hello')))['ok']
        error(owner.call('apply', request=request(alice, 4, version['generation'], choice('world'))), 'stale_generation')
        entries = owner.call('snapshot')['snapshot']['entries']
        assert [entry['raw'] for entry in entries] == ['hello']
        passed.append('word-specific deletion fence preserves unrelated queued choices')

        assert len(owner.call('clients')['clients']) == 2
        assert owner.call('reconcile', keep=[key(alice)])['retired'] == 1
        error(owner.call('apply', request=forget), 'unknown_client')
        error(owner.call('resume', key=key(bob)), 'expired_lease')
        assert owner.call('snapshot')['snapshot']['entries'] == entries
        passed.append('explicit stale-client reconciliation preserves learning')

        disk = path.read_bytes()
        error(owner.raw('x' * 4097), 'invalid_request')
        owner.close(2)
        assert path.read_bytes() == disk
        passed.append('oversized frame terminates stream without a write')
        owner = start()
        assert owner.call('snapshot')['snapshot']['entries'] == entries
        owner.close(0)
        path.write_bytes(b'foreign-or-corrupt-file')
        invalid = start()
        _, diagnostic = invalid.close(2)
        assert 'InvalidState' in diagnostic
        assert path.read_bytes() == b'foreign-or-corrupt-file'
        passed.append('corrupt or foreign store preserved and refused')
    finally:
        original = sys.exc_info()[1]
        cleanup_errors = []
        for child in children:
            try:
                if child.process.poll() is None:
                    child.close()
            except Exception as exc:
                cleanup_errors.append(exc)
                try:
                    if child.process.poll() is None:
                        child.process.kill()
                        child.process.wait(timeout=5)
                except Exception as fallback:
                    cleanup_errors.append(fallback)
        if cleanup_errors:
            if original is not None:
                original.add_note('Owned-child cleanup errors: ' + repr(cleanup_errors))
            else:
                raise cleanup_errors[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    assert os.name == 'nt', 'Windows subprocess fixture only'
    exe = args.exe.resolve(strict=True)
    assert not args.output.exists(), 'use a new report path'
    root = Path(tempfile.mkdtemp(prefix='hanautomata-learning-driver-한글 ')).resolve()
    report = dict(exe=str(exe), sha256=hashlib.sha256(exe.read_bytes()).hexdigest(),
                  synthetic_data_only=True, real_apps_approved=0, passed=[], result='fail')
    started = time.monotonic()
    failure = None
    try:
        exercise(exe, root, report['passed'])
        # The fresh direct child of TEMP is verified before removing individual fixture files.
        assert root.parent == Path(tempfile.gettempdir()).resolve()
        assert root.name.startswith('hanautomata-learning-driver-한글 ')
        for item in root.iterdir():
            assert item.parent == root and item.is_file() and not item.is_symlink()
            assert item.name == '학습 데이터.db' or item.name == '학습 데이터.db.owner.lock'
            item.unlink()
        root.rmdir()
        report.update(result='pass', cleanup_verified=not root.exists())
    except Exception as exc:
        failure = exc
        report.update(error=repr(exc), preserved_fixture=str(root))
    report['elapsed_seconds'] = round(time.monotonic() - started, 3)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump(report, stream, ensure_ascii=False, indent=2)
    print(json.dumps(report, ensure_ascii=False))
    if failure is not None:
        raise failure


if __name__ == '__main__':
    main()
