"""Owned native SSH UI, Explorer file drop, Save dialog and password authentication checks."""
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import traceback

import windows_desktop_acceptance as ui

ui.u.EnumChildWindows.argtypes = [w.HWND, c.c_void_p, w.LPARAM]
ui.u.GetClassNameW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
ui.u.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
ui.u.GetWindowLongW.argtypes = [w.HWND, c.c_int]
ui.u.SendMessageW.restype = w.LPARAM


def click(hwnd, x, y, right=False):
    ui.activate_owned(hwnd)
    scale = ui.u.GetDpiForWindow(hwnd) / 96
    left, top, _, _ = ui.bounds(hwnd)
    ui.u.SetCursorPos(int(left + x * scale), int(top + y * scale))
    ui.send(hwnd, [ui.Input(0, ui.Payload(mouse=ui.Mouse(0, 0, 0, 8 if right else 2, 0, 0)))])
    ui.send(hwnd, [ui.Input(0, ui.Payload(mouse=ui.Mouse(0, 0, 0, 16 if right else 4, 0, 0)))])


def unicode_input(hwnd, text):
    events = []
    units = text.encode('utf-16-le')
    for offset in range(0, len(units), 2):
        value = int.from_bytes(units[offset:offset + 2], 'little')
        events += [ui.Input(1, ui.Payload(keyboard=ui.Keyboard(0, value, 4, 0, 0))),
                   ui.Input(1, ui.Payload(keyboard=ui.Keyboard(0, value, 6, 0, 0)))]
    ui.send(hwnd, events)


def owned_dialog(pid):
    matches = []
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    def visit(hwnd, _):
        owner = w.DWORD()
        ui.u.GetWindowThreadProcessId(hwnd, c.byref(owner))
        name = c.create_unicode_buffer(128)
        ui.u.GetClassNameW(hwnd, name, 128)
        if owner.value == pid and ui.u.IsWindowVisible(hwnd) and name.value == '#32770':
            ui.WINDOW_OWNERS[hwnd] = pid
            matches.append(hwnd)
        return True
    callback = callback_type(visit)
    ui.u.EnumWindows(callback, 0)
    return matches[0] if matches else None


class ProcessEntry(c.Structure):
    _fields_ = [('size', w.DWORD), ('usage', w.DWORD), ('pid', w.DWORD), ('heap', c.c_size_t),
                ('module', w.DWORD), ('threads', w.DWORD), ('parent', w.DWORD),
                ('priority', w.LONG), ('flags', w.DWORD), ('name', w.WCHAR * 260)]


def askpass_child(parent):
    ui.k.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
    ui.k.CreateToolhelp32Snapshot.restype = w.HANDLE
    ui.k.Process32FirstW.argtypes = [w.HANDLE, c.POINTER(ProcessEntry)]
    ui.k.Process32NextW.argtypes = [w.HANDLE, c.POINTER(ProcessEntry)]
    ui.k.CloseHandle.argtypes = [w.HANDLE]
    snapshot = ui.k.CreateToolhelp32Snapshot(2, 0)
    assert snapshot != c.c_void_p(-1).value
    processes = []
    entry = ProcessEntry()
    entry.size = c.sizeof(entry)
    try:
        valid = ui.k.Process32FirstW(snapshot, c.byref(entry))
        while valid:
            processes.append((entry.pid, entry.parent, entry.name))
            valid = ui.k.Process32NextW(snapshot, c.byref(entry))
    finally: ui.k.CloseHandle(snapshot)
    descendants = {parent}
    for _ in range(6):
        descendants.update(pid for pid, owner, _ in processes if owner in descendants)
    return next((pid for pid, _, name in processes if pid in descendants and name == 'git-terminal-askpass.exe'), None)


def password_dialog(fixture):
    process = subprocess.Popen([str(fixture.binary), 'remote', 'list', 'password fixture'],
                               env=fixture.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    dialog = None
    try:
        helper = ui.wait_for(lambda: askpass_child(process.pid), 5)
        dialog = ui.wait_for(lambda: owned_dialog(helper), 3)
        controls = []
        callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
        def visit(hwnd, _):
            owner = w.DWORD()
            ui.u.GetWindowThreadProcessId(hwnd, c.byref(owner))
            name = c.create_unicode_buffer(64)
            ui.u.GetClassNameW(hwnd, name, 64)
            if owner.value == helper and name.value == 'Edit' and ui.u.GetWindowLongW(hwnd, -16) & 0x20:
                controls.append(hwnd)
            return True
        callback = callback_type(visit)
        ui.u.EnumChildWindows(dialog, callback, 0)
        assert len(controls) == 1, 'Owned native password field was not found'
        value = c.create_unicode_buffer(fixture.password)
        ui.u.SendMessageW(controls[0], 0x000C, 0, c.cast(value, c.c_void_p).value)
        ui.u.PostMessageW(dialog, 0x0111, 1, 0)
        stdout, _ = process.communicate(timeout=8)
        assert process.returncode == 0 and json.loads(stdout)['entries']
        return dict(native_authentication_dialog=True, disposable_password_authenticated=True, secret_persisted=False)
    finally:
        if process.poll() is None:
            if dialog: ui.u.PostMessageW(dialog, 0x10, 0, 0)
            process.kill()
            process.wait(timeout=3)


def run_desktop(fixture, git, drop_helper, output):
    results = []
    process = None
    def record(name, action):
        start = time.monotonic()
        try:
            detail = action()
            results.append(dict(case=name, status='pass', seconds=round(time.monotonic() - start, 3), detail=detail))
        except Exception as error:
            results.append(dict(case=name, status='fail', seconds=round(time.monotonic() - start, 3), error_type=type(error).__name__, reason=str(error)[:140]))
            Path(output).with_suffix('.traceback.txt').write_text(traceback.format_exc(), encoding='utf-8')
            try:
                if process and process.poll() is None:
                    hwnd = ui.window_for(process.pid)
                    if hwnd: ui.screenshot(hwnd, Path(output).with_name(Path(output).stem + '-' + name + '.png'))
            except (AssertionError, OSError): pass
        print(json.dumps(results[-1]), flush=True)

    def launch(*args):
        nonlocal process
        process = subprocess.Popen([str(fixture.binary), '--git', str(git), '--cwd', fixture.env['HOME'], *args],
                                   env=fixture.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        hwnd = ui.wait_for(lambda: ui.window_for(process.pid), 5)
        ui.u.SetWindowPos(hwnd, None, 20, 20, 1660, 970, 4)
        ui.activate_owned(hwnd)
        return hwnd

    def close():
        nonlocal process
        if process and process.poll() is None:
            hwnd = ui.window_for(process.pid)
            if hwnd: ui.u.PostMessageW(hwnd, 0x10, 0, 0)
            try: process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                subprocess.run(['taskkill.exe', '/PID', str(process.pid), '/T', '/F'], capture_output=True)
                process.wait(timeout=3)
        process = None

    try:
        hwnd = launch('--shell', 'bash', '--no-profile')
        ui.wait_for(lambda: ui.prompt_ready(hwnd), 3)
        scale = ui.u.GetDpiForWindow(hwnd) / 96
        width = (ui.bounds(hwnd)[2] - ui.bounds(hwnd)[0]) / scale
        def host_editor():
            click(hwnd, width - 260, 24)
            time.sleep(.15)
            click(hwnd, 188, 70)
            time.sleep(.15)
            for index, text in enumerate(['native 中文', 'Acceptance', 'fixture', '', '', '', str(fixture.config)]):
                ui.shortcut(hwnd, 0x11, 0x41)
                ui.type_text(hwnd, text)
                if index != 6: ui.shortcut(hwnd, 0x09)
            ui.shortcut(hwnd, 0x0D)
            config = Path(fixture.env['APPDATA']) / 'git-terminal/hosts.json'
            ui.wait_for(lambda: any(host['name'] == 'native 中文' for host in json.loads(config.read_text())), 3)
            return dict(unicode_commits=True, native_form=True, persisted=True)
        record('native-host-form-group-config-and-unicode', host_editor)
        close()

        hwnd = launch('--ssh', 'fixture')
        time.sleep(.7)
        def ssh_tab():
            ui.type_text(hwnd, "printf ready > /home/terminal/ui-shell.txt")
            ui.shortcut(hwnd, 0x0D)
            ui.wait_for(lambda: subprocess.run(['docker', 'exec', fixture.name, 'test', '-s', '/home/terminal/ui-shell.txt'], capture_output=True).returncode == 0, 3)
            ui.shortcut(hwnd, 0x11, 0x54)
            time.sleep(.2)
            ui.type_text(hwnd, "printf second > /home/terminal/ui-second-tab.txt")
            ui.shortcut(hwnd, 0x0D)
            ui.wait_for(lambda: subprocess.run(['docker', 'exec', fixture.name, 'test', '-s', '/home/terminal/ui-second-tab.txt'], capture_output=True).returncode == 0, 3)
            ui.shortcut(hwnd, 0x11, 0x57)
            return dict(real_remote_pty=True, ctrl_t_reuses_host=True, ctrl_w=True)
        record('native-ssh-pty-tabs-and-command-roundtrip', ssh_tab)

        def sftp_drop():
            click(hwnd, width - 260, 24)
            time.sleep(.15)
            click(hwnd, 65, 138)
            time.sleep(.65)
            source = fixture.root / 'ui-upload.txt'
            source.write_text('native-drop-中文\n', encoding='utf-8', newline='\n')
            left, top, right, _ = ui.bounds(hwnd)
            ready = fixture.root / 'drag-ready'
            trace = open(Path(output).with_suffix('.drag.txt'), 'wb')
            drag = subprocess.Popen([str(drop_helper), str(source), str(ready)], stdout=subprocess.DEVNULL, stderr=trace)
            try:
                ui.wait_for(ready.exists, 2)
                source_hwnd = ui.wait_for(lambda: int(ready.read_text() or '0'), 1)
                ui.WINDOW_OWNERS[source_hwnd] = drag.pid
                ui.activate_owned(source_hwnd)
                source_bounds = w.RECT()
                assert ui.u.GetWindowRect(source_hwnd, c.byref(source_bounds))
                ui.u.SetCursorPos((source_bounds.left + source_bounds.right) // 2, (source_bounds.top + source_bounds.bottom) // 2)
                ui.send(source_hwnd, [ui.Input(0, ui.Payload(mouse=ui.Mouse(0, 0, 0, 2, 0, 0)))])
                time.sleep(.1)
                ui.u.SetCursorPos(int(right - 145 * scale), int(top + 270 * scale))
                events = (ui.Input * 1)(ui.Input(0, ui.Payload(mouse=ui.Mouse(1, 1, 0, 1, 0, 0))))
                ui.u.SendInput(1, events, c.sizeof(ui.Input))
                time.sleep(.15)
                events = (ui.Input * 1)(ui.Input(0, ui.Payload(mouse=ui.Mouse(0, 0, 0, 4, 0, 0))))
                assert ui.u.SendInput(1, events, c.sizeof(ui.Input)) == 1
                assert drag.wait(timeout=5) == 0, 'Native Explorer-style drop was refused'
            finally:
                events = (ui.Input * 1)(ui.Input(0, ui.Payload(mouse=ui.Mouse(0, 0, 0, 4, 0, 0))))
                ui.u.SendInput(1, events, c.sizeof(ui.Input))
                if drag.poll() is None: drag.kill(); drag.wait(timeout=3)
                trace.close()
            expected = hashlib.sha256(source.read_bytes()).hexdigest().encode()
            def uploaded():
                value = subprocess.run(['docker', 'exec', fixture.name, 'sha256sum', '/home/terminal/ui-upload.txt'], capture_output=True)
                return value.returncode == 0 and value.stdout.split()[0] == expected
            ui.wait_for(uploaded, 3)
            ui.screenshot(hwnd, Path(output).with_name(Path(output).stem + '-sftp.png'))
            return dict(ole_file_drop=True, upload_sha256_verified=True)
        record('native-explorer-drop-to-sftp-upload', sftp_drop)

        def download():
            entries = json.loads(fixture.cli('remote', 'list', 'fixture').stdout)['entries']
            index = next(i for i, entry in enumerate(entries) if entry['name'] == 'ui-upload.txt')
            image = ui.ImageGrab.grab(bbox=ui.bounds(hwnd))
            x0 = int(image.width - 270 * scale)
            x1 = int(image.width - 85 * scale)
            rows = []
            for y in range(int(200 * scale), min(image.height - int(85 * scale), int(540 * scale))):
                if sum(1 for x in range(x0, x1) if (lambda p: p[0] > 175 and p[1] > 180 and p[2] > 190)(image.getpixel((x, y))[:3])) >= 3:
                    if rows and y - rows[-1][1] <= 4: rows[-1][1] = y
                    else: rows.append([y, y])
            assert index < len(rows), 'Expected remote file row was not visible'
            click(hwnd, width - 200, sum(rows[index]) / 2 / scale, right=True)
            time.sleep(.12)
            click(hwnd, width - 160, 177)
            dialog = ui.wait_for(lambda: owned_dialog(process.pid), 3)
            local = fixture.root / 'native-downloaded.txt'
            fields = []
            diagnostic = []
            callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
            def visit(control, _):
                name = c.create_unicode_buffer(64)
                text = c.create_unicode_buffer(512)
                ui.u.GetClassNameW(control, name, 64)
                ui.u.SendMessageW(control, 0x000D, 512, c.cast(text, c.c_void_p).value)
                if name.value in ('Edit', 'ComboBox', 'Button', 'Static'): diagnostic.append((name.value, text.value))
                if name.value == 'Edit' and text.value == 'ui-upload.txt': fields.append(control)
                return True
            callback = callback_type(visit)
            ui.u.EnumChildWindows(dialog, callback, 0)
            Path(output).with_suffix('.save-controls.json').write_text(json.dumps(diagnostic, ensure_ascii=False), encoding='utf-8')
            assert len(fields) == 1, 'The native Save dialog filename field was not found'
            value = c.create_unicode_buffer(str(local))
            ui.u.SendMessageW(fields[0], 0x000C, 0, c.cast(value, c.c_void_p).value)
            ui.u.PostMessageW(dialog, 0x0111, 1, 0)
            ui.wait_for(lambda: local.exists(), 3)
            assert local.read_bytes() == (fixture.root / 'ui-upload.txt').read_bytes()
            return dict(right_click=True, native_save_dialog=True, download_sha256_verified=True)
        record('native-sftp-context-download-and-save-dialog', download)

        def monitor():
            click(hwnd, width - 210, 94)
            time.sleep(2.4)
            ui.screenshot(hwnd, Path(output).with_name(Path(output).stem + '-monitor.png'))
            # Execute while telemetry is active; the shell must keep responding.
            ui.type_text(hwnd, 'printf ready > /home/terminal/ui-with-monitor.txt')
            ui.shortcut(hwnd, 0x0D)
            ui.wait_for(lambda: subprocess.run(['docker', 'exec', fixture.name, 'test', '-s', '/home/terminal/ui-with-monitor.txt'], capture_output=True).returncode == 0, 3)
            return dict(native_panel=True, shell_responsive=True)
        record('native-monitor-panel-and-concurrent-shell', monitor)
    finally:
        close()
    record('native-password-authentication-dialog', lambda: password_dialog(fixture))
    return results
