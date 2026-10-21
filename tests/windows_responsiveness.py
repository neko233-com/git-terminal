"""Measure real prompt recovery and hidden-tab VT replies in disposable Windows sessions."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

import windows_desktop_acceptance as ui


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--git', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--deadline', type=float, default=3)
    args = parser.parse_args()
    results = []
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='git-terminal-response-') as temporary:
        root = Path(temporary)
        home = root / 'home'
        home.mkdir()
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(('GIT_', 'GCM_', 'SSH_', 'GNUPG'))}
        env.update(HOME=str(home), USERPROFILE=str(home), APPDATA=str(home / 'appdata'),
                   LOCALAPPDATA=str(home / 'local'), XDG_CONFIG_HOME=str(home / '.config'),
                   GIT_CONFIG_GLOBAL=str(home / '.gitconfig'), SSH_ROOT_PATH=str(home / '.ssh'),
                   GCM_INTERACTIVE='never', LANG='C.UTF-8', LC_ALL='C.UTF-8')
        for profile in ('bash', 'powershell', 'cmd'):
            process = None
            try:
                start = time.monotonic()
                process = subprocess.Popen([str(args.binary.resolve()), '--git', str(args.git.resolve()),
                                            '--cwd', str(home), '--shell', profile], env=env,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                hwnd = ui.wait_for(lambda: ui.window_for(process.pid), 5)
                ui.u.SetWindowPos(hwnd, None, 40, 40, 1120, 720, 4)
                ui.activate_owned(hwnd)
                if profile == 'bash':
                    ui.wait_for(lambda: ui.prompt_ready(hwnd), args.deadline)
                else:
                    time.sleep(.9 if profile == "powershell" else .1)
                path = root / (profile + '-ready')
                quote = lambda p: "'" + str(p).replace('\\', '/').replace("'", "'\\''") + "'"
                def mark(p):
                    if profile == 'bash': return "printf ready > " + quote(p)
                    if profile == 'powershell': return "[IO.File]::WriteAllText('" + str(p) + "','ready')"
                    return 'echo ready>"' + str(p) + '"'
                def completed(p):
                    ui.wait_for(lambda: p.exists() and p.read_text().strip() == 'ready', args.deadline)
                execute = lambda source: (ui.type_text(hwnd, source), ui.shortcut(hwnd, 0x0D))
                execute(mark(path))
                completed(path)
                startup = time.monotonic() - start
                timings = []
                for index in range(5):
                    path = root / f'{profile}-{index}'
                    then = time.monotonic()
                    execute("git")
                    execute(mark(path))
                    completed(path)
                    timings.append(round((time.monotonic() - then) * 1000, 1))
                if profile == 'bash':
                    # Simulate the drive-qualified PWD seen after Windows shell integration.
                    path = root / 'drive-root-ready'
                    execute('PWD=' + quote(home) + '; true')
                    execute(mark(path))
                    completed(path)
                    path = root / 'hidden-reply'
                    # The foreground tab changes before the original asks for its cursor.
                    execute("sleep .3; printf '\\033[6n'; IFS= read -rs -d R -t 1 answer; "
                               "[[ $answer == *'['*';'* ]] && " + mark(path))
                    ui.shortcut(hwnd, 0x11, 0x54)
                    completed(path)
                    ui.shortcut(hwnd, 0x11, 0x57)
                    # Large output must not delay the next command indefinitely.
                    path = root / 'burst-ready'
                    then = time.monotonic()
                    execute("printf '%080d\\n' {1..25000}")
                    # A slow producer is allowed to finish; the UI must remain usable.
                    ui.shortcut(hwnd, 0x11, 0x54)
                    foreground = root / 'during-burst-ready'
                    execute(mark(foreground))
                    completed(foreground)
                    burst_tab = round((time.monotonic() - then) * 1000, 1)
                    ui.shortcut(hwnd, 0x11, 0x57)
                    execute(mark(path))
                    ui.wait_for(lambda: path.exists() and path.read_text().strip() == 'ready', 10)
                    burst = round((time.monotonic() - then) * 1000, 1)
                    path = root / 'after-burst-ready'
                    then = time.monotonic()
                    execute(mark(path))
                    completed(path)
                    burst_recovery = round((time.monotonic() - then) * 1000, 1)
                else:
                    burst = burst_tab = burst_recovery = None
                ui.screenshot(hwnd, args.output.with_name(args.output.stem + '-' + profile + '.png'))
                result = dict(profile=profile, status='pass', startup_command_ms=round(startup * 1000, 1),
                              help_then_next_command_ms=timings, burst_2mb_completion_ms=burst,
                              burst_new_tab_command_ms=burst_tab, burst_recovery_command_ms=burst_recovery)
            except (AssertionError, OSError, ValueError, subprocess.SubprocessError) as error:
                result = dict(profile=profile, status='fail', reason=str(error)[:180])
                if process and ui.window_for(process.pid):
                    ui.screenshot(ui.window_for(process.pid), args.output.with_name(args.output.stem + '-' + profile + '-failure.png'))
            finally:
                if process and process.poll() is None:
                    hwnd = ui.window_for(process.pid)
                    if hwnd: ui.u.PostMessageW(hwnd, 0x10, 0, 0)
                    try: process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        subprocess.run(['taskkill.exe', '/PID', str(process.pid), '/T', '/F'], capture_output=True)
                        process.wait(timeout=3)
            results.append(result)
            print(json.dumps(result), flush=True)
    report = dict(status='pass' if all(r['status'] == 'pass' for r in results) else 'fail',
                  binary_sha256=hashlib.file_digest(args.binary.open('rb'), 'sha256').hexdigest(),
                  results=results, personal_profile_accessed=False, terminal_contents_logged=False)
    args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8', newline='\n')
    raise SystemExit(0 if report['status'] == 'pass' else 1)


if __name__ == '__main__':
    main()
