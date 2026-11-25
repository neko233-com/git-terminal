"""Replace only the terminal component of an accepted git-next MSI distribution.

The core and MSI ProductVersion retain their accepted identity. Windows Installer
calls this a small update; reinstall/recache the package instead of inventing a
new Git engine version. All unchanged payload bytes are verified and retained.

The terminal component is the Go 1.27 MyGo build: one git-terminal.exe plus its
ghostty-vt.dll. There is no second helper executable, no libunwind and no Rust
toolchain in this package.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import struct

# Files that belong beside the executable in bin/. Everything else in the
# terminal package is a notice under LICENSES/git-terminal/.
EXECUTABLE = 'git-terminal.exe'
NATIVE_LIBRARY = 'ghostty-vt.dll'


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def safe(root, name):
    part = PurePosixPath(name)
    if part.is_absolute() or '..' in part.parts or any(':' in p or '\\' in p for p in part.parts):
        raise ValueError('Unsafe package name')
    path = root / name
    if path.is_symlink() or not path.resolve().is_relative_to(root):
        raise ValueError('Unsafe package path')
    return path


def mapped(name):
    if name in (EXECUTABLE, NATIVE_LIBRARY) or name.startswith('shell-integration/'):
        return 'bin/' + name
    return 'LICENSES/git-terminal/' + name


def main():
    parser = argparse.ArgumentParser(__doc__)
    for name in ('base', 'terminal', 'acceptance', 'destination'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    base = args.base.resolve(strict=True)
    terminal = args.terminal.resolve(strict=True)
    output = args.destination.resolve()
    if output.exists():
        parser.error('Use a new output directory')
    source = base / 'git-next-distribution.json'
    original = json.loads(source.read_text())
    identity = json.loads((terminal / 'git-terminal-package.json').read_text())
    gate = json.loads(args.acceptance.read_text())
    if not (original['product'] == 'git-next' and original['release_eligible'] is True
            and original['complete_git_parity'] is True and original['upstream_git_core_deployed'] is False
            and identity['product'] == 'git-terminal' and identity['renderer'] == 'mygo'
            and identity['platform'] == original['platform'] and identity['source_built'] is True
            and gate['status'] == 'pass' and gate['verification_seconds'] < 60):
        parser.error('Accepted base distribution, matching Go terminal and passed Go gate are required')
    for name, record in original['files'].items():
        path = safe(base, name)
        if sha(path) != record['sha256'] or path.stat().st_size != record['bytes']:
            parser.error('Base payload changed: ' + name)
    for name, digest in identity['files'].items():
        if sha(safe(terminal, name)) != digest:
            parser.error('Terminal package changed: ' + name)
    # Windows x64 owns native runtime acceptance; ARM64 is cross-built.
    if identity['platform'] == 'windows-x64':
        if sha(terminal / EXECUTABLE) != gate['binary_sha256']:
            parser.error('Go gate does not identify these exact terminal bytes')
        if sha(terminal / NATIVE_LIBRARY) != gate['native_library_sha256']:
            parser.error('Go gate does not identify this exact Ghostty VT library')
    for name, digest in gate['shell_integration_sha256'].items():
        if sha(terminal / 'shell-integration' / name) != digest:
            parser.error('Accepted shell integration changed')
    report = copy.deepcopy(original)
    records = report['files']
    # A small MSI update must retain every old component, including old notices.
    replaced = set(records) & {mapped(name) for name in [*identity['files'], 'git-terminal-package.json']}
    output.mkdir(parents=True)
    for name in records.keys() - replaced:
        target = safe(output, name)
        target.parent.mkdir(parents=True, exist_ok=True)
        # Hard links keep immutable base payloads from consuming another full copy.
        os.link(base / name, target)
    for name in replaced:
        del records[name]
    for name in [*identity['files'], 'git-terminal-package.json']:
        native = name in (EXECUTABLE, NATIVE_LIBRARY)
        relative = mapped(name)
        target = safe(output, relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(terminal / name, target)
        records[relative] = dict(sha256=sha(target), bytes=target.stat().st_size,
                                 kind='source-built-git-terminal' if native else 'git-terminal-notice',
                                 windows_attributes=0)
    expected = 0x8664 if identity['platform'] == 'windows-x64' else 0xaa64
    data = (output / 'bin' / EXECUTABLE).read_bytes()
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    if data[:2] != b'MZ' or data[pe:pe + 4] != b'PE\0\0' or struct.unpack_from('<H', data, pe + 4)[0] != expected:
        parser.error('Terminal PE architecture mismatch')
    report['git_terminal'] = {key: identity[key] for key in ('product', 'version', 'source_commit', 'platform', 'renderer')}
    report['git_terminal']['native_library'] = dict(name=NATIVE_LIBRARY, sha256=sha(terminal / NATIVE_LIBRARY))
    report['terminal_update_acceptance'] = dict(
        base_manifest_sha256=sha(source), go_acceptance_sha256=sha(args.acceptance),
        terminal_package_sha256=sha(terminal / 'git-terminal-package.json'),
        core_and_other_payload_unchanged=True, native_runtime_platform='windows-x64',
        arm64_runtime_executed=False,
        installer_operation='Windows Installer small update; original source basename, REINSTALL=ALL REINSTALLMODE=vamus',
        core_version=original['version'], terminal_version=identity['version'])
    for name, record in records.items():
        if sha(output / name) != record['sha256']:
            raise ValueError('Staged payload changed')
    (output / 'git-next-distribution.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8', newline='\n')
    print(json.dumps(dict(status='component-accepted-awaiting-msi-lifecycle', platform=identity['platform'],
                          core=original['version'], terminal=identity['version'], renderer='mygo', files=len(records))))


if __name__ == '__main__':
    main()
