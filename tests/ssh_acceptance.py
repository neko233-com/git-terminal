"""Disposable loopback OpenSSH fixture and a concurrent real SFTP/monitor acceptance batch."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import time


def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], capture_output=True, timeout=kwargs.pop('timeout', 30), **kwargs)


class Fixture:
    def __init__(self, binary, output):
        self.binary = Path(binary).resolve(strict=True)
        self.output = Path(output).resolve()
        self.temporary = tempfile.TemporaryDirectory(prefix='ssh-acceptance-', dir=self.output.parent)
        self.root = Path(self.temporary.name)
        self.name = 'git-terminal-ssh-' + secrets.token_hex(5)
        self.password = secrets.token_urlsafe(24)
        self.started = False
        self.env = {k: v for k, v in os.environ.items() if not k.startswith(('GIT_', 'GCM_', 'SSH_', 'GNUPG'))}
        home = self.root / 'home'
        home.mkdir()
        self.env.update(HOME=str(home), USERPROFILE=str(home), APPDATA=str(home / 'appdata'),
                        LOCALAPPDATA=str(home / 'local'), SSH_ROOT_PATH=str(home / '.ssh'),
                        GIT_CONFIG_GLOBAL=str(home / '.gitconfig'), GCM_INTERACTIVE='never', LANG='C.UTF-8', LC_ALL='C.UTF-8')
        self.config = self.root / 'config'

    def setup(self):
        system = Path(os.environ['SystemRoot']) / 'System32/OpenSSH'
        self.key = self.root / 'fixture-key'
        keygen = run([system / 'ssh-keygen.exe', '-q', '-t', 'ed25519', '-N', '', '-f', self.key])
        assert keygen.returncode == 0, 'Fixture key generation failed'
        owner = subprocess.check_output(['whoami.exe'], text=True).strip()
        permissions = run(['icacls.exe', self.key, '/inheritance:r', '/grant:r', owner + ':F'])
        assert permissions.returncode == 0, 'Private fixture key permissions failed'
        public = self.root / 'public'
        public.mkdir()
        (public / 'authorized_keys').write_bytes(self.key.with_suffix('.pub').read_bytes())
        result = run(['docker', 'run', '-d', '--name', self.name, '--label', 'com.git-terminal.fixture=ssh-acceptance',
                      '--pids-limit', '128', '--cpus', '2', '--memory', '256m',
                      '--mount', f'type=bind,src={public},dst=/fixture,readonly',
                      '--publish', '127.0.0.1::22', 'git-terminal-ssh-fixture:20261021'])
        assert result.returncode == 0, 'Disposable SSH container could not start'
        self.started = True
        port = run(['docker', 'port', self.name, '22/tcp']).stdout.decode().strip().rsplit(':', 1)[1]
        self.port = int(port)
        host_key = run(['docker', 'exec', self.name, 'cat', '/etc/ssh/ssh_host_ed25519_key.pub'])
        assert host_key.returncode == 0
        self.known = self.root / 'known_hosts'
        self.known.write_text(f'[127.0.0.1]:{port} ' + host_key.stdout.decode(), encoding='utf-8')
        self.config.write_text(f'''Host fixture fixture-password
  HostName 127.0.0.1
  Port {port}
  User terminal
  UserKnownHostsFile "{self.known.as_posix()}"
  StrictHostKeyChecking yes
Host fixture
  IdentityFile "{self.key.as_posix()}"
  IdentitiesOnly yes
  BatchMode yes
Host fixture-password
  PubkeyAuthentication no
  PreferredAuthentications password
  BatchMode no
''', encoding='utf-8', newline='\n')
        result = run(['docker', 'exec', '-i', self.name, 'chpasswd'], input=('terminal:' + self.password + '\n').encode())
        assert result.returncode == 0, 'Disposable password authentication setup failed'
        for _ in range(100):
            check = run([system / 'ssh.exe', '-F', self.config, 'fixture', 'true'], env=self.env)
            if check.returncode == 0: break
            time.sleep(.05)
        else: raise AssertionError('Disposable SSH fixture did not become ready')
        result = run(['docker', 'exec', '--user', 'terminal', self.name, 'sh', '-c',
                      "mkdir -p '/home/terminal/files/中文 dir'; printf seed > '/home/terminal/files/中文 dir/space name.txt'; "
                      "ln -s /etc/shadow /home/terminal/files/do-not-follow; : > /home/terminal/files/empty.bin"])
        assert result.returncode == 0
        self.cli('remote', 'add', 'fixture', 'fixture', '--group', 'Acceptance', '--config', self.config)
        self.cli('remote', 'add', 'password fixture', 'fixture-password', '--group', 'Acceptance', '--config', self.config)

    def cli(self, *args, expect=0, env=None):
        result = run([self.binary, *args], env=env or self.env, timeout=30)
        assert (result.returncode == 0) if expect == 0 else (result.returncode != 0), 'Remote CLI returned an unexpected status'
        return result

    def close(self):
        if self.started:
            label = run(['docker', 'inspect', '--format', '{{index .Config.Labels "com.git-terminal.fixture"}}', self.name])
            assert label.stdout.strip() == b'ssh-acceptance', 'Container ownership mismatch; cleanup refused'
            removed = run(['docker', 'rm', '--force', '--volumes', self.name])
            assert removed.returncode == 0, 'Owned container cleanup failed'
        self.temporary.cleanup()


def batch(fixture):
    results = []
    def record(name, action):
        start = time.monotonic()
        try:
            detail = action()
            return dict(case=name, status='pass', seconds=round(time.monotonic() - start, 3), detail=detail)
        except (AssertionError, OSError, ValueError, subprocess.SubprocessError) as error:
            return dict(case=name, status='fail', seconds=round(time.monotonic() - start, 3), error_type=type(error).__name__)

    def host_config():
        env = dict(fixture.env, APPDATA=str(fixture.root / 'host-crud'))
        fixture.cli('remote', 'add', 'test host', 'fixture', '--group', 'Test', '--config', fixture.config, env=env)
        fixture.cli('remote', 'add', 'invalid', '-oProxyCommand=bad', expect=1, env=env)
        fixture.cli('remote', 'import', fixture.config, env=env)
        hosts = json.loads(fixture.cli('remote', 'hosts', env=env).stdout)
        assert len(hosts) == 3
        fixture.cli('remote', 'remove', 'test host', env=env)
        assert len(json.loads(fixture.cli('remote', 'hosts', env=env).stdout)) == 2
        return dict(hosts=2, password_fields_stored=False)

    def transfer():
        source = fixture.root / 'source'
        source.mkdir()
        nested = source / '中文 with space'
        nested.mkdir()
        (nested / "quote's file.txt").write_text('中文🙂\nLF\n', encoding='utf-8', newline='\n')
        (source / 'empty.bin').write_bytes(b'')
        large = source / 'large.bin'
        with large.open('wb') as file:
            for _ in range(16): file.write(os.urandom(1024 * 1024))
        start = time.monotonic()
        fixture.cli('remote', 'put', 'fixture', source, '/home/terminal/roundtrip')
        uploaded = time.monotonic() - start
        start = time.monotonic()
        destination = fixture.root / 'download'
        fixture.cli('remote', 'get', 'fixture', '/home/terminal/roundtrip', destination)
        downloaded = time.monotonic() - start
        for path in source.rglob('*'):
            if path.is_file():
                assert hashlib.file_digest(path.open('rb'), 'sha256').digest() == hashlib.file_digest((destination / path.relative_to(source)).open('rb'), 'sha256').digest()
        fixture.cli('remote', 'put', 'fixture', large, '/home/terminal/roundtrip/large.bin', expect=1)
        fixture.cli('remote', 'put', 'fixture', large, '/home/terminal/roundtrip/large.bin', '--overwrite')
        return dict(bytes=large.stat().st_size, upload_mib_per_second=round(16 / uploaded, 2), download_mib_per_second=round(16 / downloaded, 2), recursive_files=3)

    def monitor():
        result = fixture.cli('remote', 'monitor', 'fixture', '--count', '2')
        samples = [json.loads(line) for line in result.stdout.splitlines()]
        assert len(samples) == 2 and samples[0]['cpus'][0]['used'] is None
        assert 0 <= samples[1]['cpus'][0]['used'] <= 100
        assert samples[1]['memory_total'] > 0 and samples[1]['spaces'] and samples[1]['os']
        assert samples[1]['rx_per_second'] is not None
        assert all(disk['read_per_second'] is not None for disk in samples[1]['disks'])
        return dict(samples=2, cpu_cores=len(samples[1]['cpus']) - 1, agent_installed=False)

    def negative():
        fixture.cli('remote', 'list', 'fixture', '/root', expect=1)
        target = fixture.root / 'must-not-exist'
        fixture.cli('remote', 'get', 'fixture', '/home/terminal/files/do-not-follow', target, expect=1)
        assert not target.exists() and not list(fixture.root.glob('.git-terminal-*.part'))
        bad = fixture.root / 'bad-config'
        text = fixture.config.read_text().replace(fixture.known.as_posix(), (fixture.root / 'empty-known-hosts').as_posix())
        bad.write_text(text, encoding='utf-8')
        env = dict(fixture.env, APPDATA=str(fixture.root / 'negative-hosts'))
        fixture.cli('remote', 'add', 'untrusted', 'fixture', '--config', bad, env=env)
        fixture.cli('remote', 'list', 'untrusted', expect=1, env=env)
        return dict(permission_denied=True, symlink_rejected=True, unknown_host_rejected=True)

    with ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(record, name, action) for name, action in (
            ('host-crud-import-and-option-injection', host_config),
            ('sftp-recursive-unicode-empty-large-file-atomic-replacement', transfer),
            ('linux-two-sample-telemetry', monitor),
            ('permissions-host-key-and-path-boundaries', negative))]
        for future in futures: results.append(future.result())
    return results


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    fixture = Fixture(args.binary, args.output)
    started = time.monotonic()
    try:
        fixture.setup()
        results = batch(fixture)
    finally:
        fixture.close()
    report = dict(status='pass' if all(item['status'] == 'pass' for item in results) else 'fail',
                  seconds=round(time.monotonic() - started, 3), results=results,
                  personal_profile_accessed=False, credentials_logged=False, owned_container_removed=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8', newline='\n')
    print(json.dumps(report))
    raise SystemExit(0 if report['status'] == 'pass' else 1)


if __name__ == '__main__': main()
