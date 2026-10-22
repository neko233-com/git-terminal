"""One isolated, parallel acceptance run; GUI input is serialized in one worker."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

from ssh_acceptance import Fixture, batch
from ssh_desktop_acceptance import run_desktop

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--git', type=Path, required=True)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--drop-helper', type=Path, required=True)
    parser.add_argument('--test-build-log', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.binary = args.binary.resolve(strict=True)
    args.git = args.git.resolve(strict=True)
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    scripts = args.binary.parent / 'shell-integration'
    assert scripts.resolve().is_relative_to(ROOT / 'target'), 'Only a workspace build may be staged by this harness'
    scripts.mkdir(exist_ok=True)
    for source in (ROOT / 'shell-integration').iterdir():
        if source.is_file(): shutil.copyfile(source, scripts / source.name)
    binaries = [ROOT / name for name in re.findall(r'\(([^\r\n]+\.exe)\)', args.test_build_log.read_text())]
    assert len(binaries) == 4 and all(path.is_file() for path in binaries)
    fixture = Fixture(args.binary, args.output)
    reports = []
    def process(name, command, output, env=None):
        started = time.monotonic()
        result = subprocess.run([str(arg) for arg in command], cwd=ROOT, env=env, capture_output=True, timeout=75)
        if result.returncode != 0:
            output.with_suffix('.stderr.txt').write_bytes(result.stderr[-16384:])
        report = json.loads(output.read_text()) if output.is_file() else dict(status='pass' if result.returncode == 0 else 'fail')
        return dict(case=name, status='pass' if result.returncode == 0 and report['status'] == 'pass' else 'fail',
                    seconds=round(time.monotonic() - started, 3), report=report)
    def units():
        env = dict(fixture.env, GIT_TERMINAL_TEST_GIT=str(args.git),
                   GIT_TERMINAL_TEST_SSH_CONFIG=str(fixture.config),
                   GIT_TERMINAL_TEST_TRANSFER_ROOT=str(fixture.root / 'cancel-tests'))
        results = []
        for binary in binaries:
            result = subprocess.run([str(binary), '--include-ignored', '--test-threads=4'], env=env, capture_output=True, timeout=50)
            (args.output.parent / (binary.stem + '.test.log')).write_bytes(result.stdout + result.stderr)
            results.append(dict(binary=binary.stem, status='pass' if result.returncode == 0 else 'fail'))
        return dict(case='rust-pty-protocol-lifecycle-cancel-and-unit-tests', status='pass' if all(r['status'] == 'pass' for r in results) else 'fail', results=results)
    def desktop():
        reports = []
        for name, script in [('responsiveness', 'windows_responsiveness.py'), ('desktop-regression', 'windows_desktop_acceptance.py')]:
            output = args.output.with_name(args.output.stem + '-' + name + '.json')
            reports.append(process(name, [sys.executable, ROOT / 'tests' / script, '--binary', args.binary, '--git', args.git, '--output', output], output))
        # The borderless window has no native resize frame, so the resize behaviour is
        # measured directly: the probe drags each edge and corner with the real mouse and
        # checks that the window geometry actually changed.
        resize_output = args.output.with_name(args.output.stem + '-resize.json')
        reports.append(process('window-resize-drag', [sys.executable, ROOT / 'tests' / 'resize_edge_probe.py',
                                                      '--binary', args.binary, '--git', args.git, '--output', resize_output], resize_output))
        surface_output = args.output.with_name(args.output.stem + '-resize-surface.json')
        reports.append(process('window-resize-surface', [sys.executable, ROOT / 'tests' / 'resize_surface_map.py',
                                                         '--binary', args.binary, '--git', args.git, '--output', surface_output], surface_output))
        result = run_desktop(fixture, args.git, args.drop_helper.resolve(strict=True), args.output.with_name(args.output.stem + '-ssh-ui.json'))
        report = dict(status='pass' if all(r['status'] == 'pass' for r in result) else 'fail', results=result)
        args.output.with_name(args.output.stem + '-ssh-ui.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8', newline='\n')
        reports.append(dict(case='ssh-sftp-native-ui', **report))
        return dict(case='serialized-native-desktop', status='pass' if all(r['status'] == 'pass' for r in reports) else 'fail', results=reports)
    started = time.monotonic()
    try:
        fixture.setup()
        def backend():
            result = batch(fixture)
            return dict(case='ssh-sftp-monitor-real-protocol', status='pass' if all(r['status'] == 'pass' for r in result) else 'fail', results=result)
        def common_git():
            output = args.output.with_name(args.output.stem + '-common-git.json')
            return process('common-git-cli-sha1-sha256', [sys.executable, ROOT.parent / 'git-next/tests/native_common_cli_acceptance.py', args.git,
                           '--reference', args.reference.resolve(strict=True), '--output', output, '--jobs', '4'], output)
        launched = time.monotonic()
        with ThreadPoolExecutor(max_workers=4) as pool:
            futures = [(name, pool.submit(action)) for name, action in [('backend', backend), ('units', units), ('desktop', desktop), ('common-git', common_git)]]
            for name, future in futures:
                try: reports.append(future.result())
                except Exception as error: reports.append(dict(case=name, status='fail', error_type=type(error).__name__))
        verification_seconds = time.monotonic() - launched
    finally:
        fixture.close()
    report = dict(status='pass' if all(item['status'] == 'pass' for item in reports) else 'fail',
                  seconds=round(time.monotonic() - started, 3), verification_seconds=round(verification_seconds, 3),
                  binary_sha256=hashlib.file_digest(args.binary.open('rb'), 'sha256').hexdigest(),
                  shell_integration_sha256={p.name: hashlib.file_digest(p.open('rb'), 'sha256').hexdigest() for p in scripts.iterdir() if p.is_file()},
                  results=reports, personal_profile_accessed=False, credentials_logged=False, owned_container_removed=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8', newline='\n')
    print(json.dumps(dict(status=report['status'], seconds=report['seconds'], verification_seconds=report['verification_seconds'], groups=[{'case':r['case'],'status':r['status']} for r in reports])))
    raise SystemExit(0 if report['status'] == 'pass' else 1)


if __name__ == '__main__': main()
