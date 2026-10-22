"""Measure which parts of the owned terminal window actually resize it when dragged.

The probe owns every window it touches: it launches the terminal, resolves the HWND
from that PID, and drives the mouse with SetCursorPos plus real button state. Each
target is pressed, dragged by a known distance, and released, so the resulting
geometry change is attributable to that specific target.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

u = c.WinDLL('user32', use_last_error=True)
u.SetProcessDPIAware()
u.IsWindowVisible.argtypes = [w.HWND]
u.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
u.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
u.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
u.ClientToScreen.argtypes = [w.HWND, c.POINTER(w.POINT)]
u.SendMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
u.SendMessageW.restype = w.LPARAM
u.SetForegroundWindow.argtypes = [w.HWND]
u.SetCursorPos.argtypes = [c.c_int, c.c_int]
u.GetCursorPos.argtypes = [c.POINTER(w.POINT)]

HT = {0: 'HTNOWHERE', 1: 'HTCLIENT', 2: 'HTCAPTION', 3: 'HTSYSMENU', 4: 'HTGROWBOX',
      8: 'HTMINBUTTON', 9: 'HTMAXBUTTON', 10: 'HTLEFT', 11: 'HTRIGHT', 12: 'HTTOP',
      13: 'HTTOPLEFT', 14: 'HTTOPRIGHT', 15: 'HTBOTTOM', 16: 'HTBOTTOMLEFT',
      17: 'HTBOTTOMRIGHT', 20: 'HTCLOSE'}


class Mouse(c.Structure):
    _fields_ = [('x', w.LONG), ('y', w.LONG), ('data', w.DWORD), ('flags', w.DWORD),
                ('time', w.DWORD), ('extra', c.c_size_t)]


class Payload(c.Union):
    _fields_ = [('mouse', Mouse)]


class Input(c.Structure):
    _fields_ = [('type', w.DWORD), ('payload', Payload)]


u.SendInput.argtypes = [w.UINT, c.POINTER(Input), c.c_int]
u.SendInput.restype = w.UINT


def window_for(pid):
    matches = []
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)

    def visit(hwnd, _):
        actual = w.DWORD()
        u.GetWindowThreadProcessId(hwnd, c.byref(actual))
        if actual.value == pid and u.IsWindowVisible(hwnd):
            matches.append(hwnd)
        return True

    callback = callback_type(visit)
    u.EnumWindows(callback, 0)
    return matches[0] if matches else None


def hit_test(hwnd, x, y):
    lparam = (y & 0xFFFF) << 16 | (x & 0xFFFF)
    return HT.get(int(u.SendMessageW(hwnd, 0x84, 0, lparam)) & 0xFFFFFFFF, '?')


def geometry(hwnd):
    window = w.RECT()
    client = w.RECT()
    origin = w.POINT()
    u.GetWindowRect(hwnd, c.byref(window))
    u.GetClientRect(hwnd, c.byref(client))
    u.ClientToScreen(hwnd, c.byref(origin))
    return {'window': [window.left, window.top, window.right, window.bottom],
            'window_size': [window.right - window.left, window.bottom - window.top],
            'client_size': [client.right - client.left, client.bottom - client.top],
            'client_origin_screen': [origin.x, origin.y]}


def press(flags):
    u.SendInput(1, c.byref(Input(0, Payload(mouse=Mouse(0, 0, 0, flags, 0, 0)))), c.sizeof(Input))


def reset(hwnd):
    """Return the window to a known rect and make sure it is ready for a gesture.

    A completed Windows resize is a modal loop: while it runs the system owns the mouse and
    swallows synthetic input, so without this reset a later target can be measured against a
    window that is still mid-gesture.

    Moving the window also drops the foreground activation, and the first synthetic press
    after that is consumed by the re-activation instead of reaching the border. The window is
    therefore re-activated and settled with a short click in the middle of the client area,
    which never touches a border, before the target is measured.
    """
    u.SetWindowPos(hwnd, 0, 100, 100, 1136, 728, 0x0002 | 0x0004)
    press(0x0004)
    time.sleep(.3)
    u.SetForegroundWindow(hwnd)
    time.sleep(.3)
    x0, y0, cw, ch = client_box(hwnd)
    u.SetCursorPos(x0 + cw // 2, y0 + ch // 2)
    time.sleep(.15)
    press(0x0002)
    time.sleep(.1)
    press(0x0004)
    time.sleep(.3)
    return hwnd


def drag(hwnd, x, y, dx, dy):
    u.SetForegroundWindow(hwnd)
    time.sleep(.25)
    u.SetCursorPos(x, y)
    time.sleep(.25)
    press(0x0002)
    time.sleep(.25)
    for step in range(1, 9):
        u.SetCursorPos(int(x + dx * step / 8), int(y + dy * step / 8))
        time.sleep(.05)
    press(0x0004)
    time.sleep(.4)


def warm_up(hwnd):
    """Make the window the active foreground window before the first measurement.

    The very first synthetic press on a freshly shown window can be consumed by the
    activation itself instead of reaching the border, which would make the first target
    look broken. A short click in the middle of the client area settles the activation
    without touching any border.
    """
    u.SetForegroundWindow(hwnd)
    time.sleep(.4)
    x0, y0, cw, ch = client_box(hwnd)
    u.SetCursorPos(x0 + cw // 2, y0 + ch // 2)
    time.sleep(.15)
    press(0x0002)
    time.sleep(.1)
    press(0x0004)
    time.sleep(.3)


def client_box(hwnd):
    client = w.RECT()
    origin = w.POINT()
    u.GetClientRect(hwnd, c.byref(client))
    u.ClientToScreen(hwnd, c.byref(origin))
    return origin.x, origin.y, client.right - client.left, client.bottom - client.top


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--git', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    git = args.git.resolve(strict=True)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix='git-terminal-edge-') as temporary:
        root = Path(temporary)
        home = root / 'home'
        repo = root / 'repo'
        home.mkdir()
        repo.mkdir()
        env = {k: v for k, v in os.environ.items() if not k.startswith(('GIT_', 'GCM_', 'SSH_'))}
        env.update(HOME=str(home), USERPROFILE=str(home), APPDATA=str(home / 'appdata'),
                   LOCALAPPDATA=str(home / 'local'), XDG_CONFIG_HOME=str(home / '.config'),
                   GIT_CONFIG_GLOBAL=str(home / '.gitconfig'), GIT_NEXT_CONFIG_HOME=str(home / 'git-next'),
                   SSH_ROOT_PATH=str(home / '.ssh'), GIT_TERMINAL_PROMPT='0', GCM_INTERACTIVE='never',
                   LANG='C.UTF-8', LC_ALL='C.UTF-8')
        process = subprocess.Popen([str(binary), '--no-profile', '--git', str(git), '--cwd', str(repo)],
                                   env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            hwnd = None
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline and hwnd is None:
                hwnd = window_for(process.pid)
                if hwnd is None:
                    time.sleep(.1)
            if hwnd is None:
                raise AssertionError('The owned terminal window did not appear')
            time.sleep(1.5)

            warm_up(hwnd)
            cases = []
            # Each target is expressed relative to the drawn client area, because that
            # is the region the user perceives as the window.
            for name, offset, delta in (
                ('client-top-edge', 0, (0, 60)),
                ('client-bottom-edge', 0, (0, -60)),
                ('client-left-edge', 0, (80, 0)),
                ('client-right-edge', 0, (-80, 0)),
                ('top-left-corner', 0, (60, 60)),
                ('bottom-right-corner', 0, (-60, -60)),
                ('inset-2-right', -2, (-80, 0)),
                ('inset-2-bottom', -2, (0, -60)),
            ):
                before = geometry(reset(hwnd))
                left, top = before['client_origin_screen']
                cw, ch = before['client_size']
                if name.startswith('top-left'):
                    x, y = left + offset, top + offset
                elif name.startswith('bottom-right'):
                    x, y = left + cw - 1 + offset, top + ch - 1 + offset
                elif 'right' in name:
                    x, y = left + cw - 1 + offset, top + ch // 2
                elif 'left' in name:
                    x, y = left + offset, top + ch // 2
                elif 'top' in name:
                    x, y = left + cw // 2, top + offset
                else:
                    x, y = left + cw // 2, top + ch - 1 + offset
                code = hit_test(hwnd, x, y)
                # A single retry absorbs the residual race between the synthetic press and
                # the window's own input pump; a genuine defect fails both attempts.
                size_before = before['window_size']
                size_after = size_before
                for attempt in range(2):
                    drag(hwnd, x, y, delta[0], delta[1])
                    after = geometry(hwnd)
                    size_after = after['window_size']
                    if size_after != size_before:
                        break
                cases.append({
                    'target': name,
                    'hit_code': code,
                    'drag': list(delta),
                    'size_before': size_before,
                    'size_after': size_after,
                    'delta': [size_after[0] - size_before[0], size_after[1] - size_before[1]],
                    'moved': [after['window'][0] - before['window'][0], after['window'][1] - before['window'][1]],
                    'resized': size_after != size_before,
                    'moved_only': (size_after == size_before
                                   and after['window'][:2] != before['window'][:2]),
                })
            value = {"status": "pending", "cases": cases}
        finally:
            if process.poll() is None:
                subprocess.run([os.path.join(os.environ['SystemRoot'], 'System32/taskkill.exe'),
                                '/PID', str(process.pid), '/T', '/F'], capture_output=True, timeout=10)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
    output.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')
    for case in value['cases']:
        print('%-24s hit=%-13s drag=%-11s delta=%-11s moved=%-11s resized=%s' % (
            case['target'], case['hit_code'], case['drag'], case['delta'], case['moved'],
            case['resized']))
    ok = all(case['resized'] for case in value['cases'])
    value['status'] = 'pass' if ok else 'fail'
    raise SystemExit(0 if ok else 1)


if __name__ == '__main__':
    main()
