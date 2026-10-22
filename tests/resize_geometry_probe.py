"""Instrument the owned terminal window's resize behaviour frame by frame.

The probe owns every window it touches: it launches the terminal, resolves the HWND
from that PID, and drives the mouse with real input. It records the window/client
geometry, the reported hit-test codes, and the geometry after each synthetic drag
step so the resize path is observed rather than assumed.
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
u.GetForegroundWindow.restype = w.HWND
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
u.GetWindowLongPtrW.restype = c.c_ssize_t
u.GetWindowLongPtrW.argtypes = [w.HWND, c.c_int]

GWL_STYLE = -16
WS_OVERLAPPED = 0x00000000
WS_CAPTION = 0x00C00000
WS_THICKFRAME = 0x00040000
WS_MINIMIZEBOX = 0x00020000
WS_MAXIMIZEBOX = 0x00010000
WS_SYSMENU = 0x00080000
WS_POPUP = 0x80000000

HT = {0: 'HTNOWHERE', 1: 'HTCLIENT', 2: 'HTCAPTION', 3: 'HTSYSMENU', 4: 'HTGROWBOX',
      8: 'HTMINBUTTON', 9: 'HTMAXBUTTON', 10: 'HTLEFT', 11: 'HTRIGHT', 12: 'HTTOP',
      13: 'HTTOPLEFT', 14: 'HTTOPRIGHT', 15: 'HTBOTTOM', 16: 'HTBOTTOMLEFT',
      17: 'HTBOTTOMRIGHT', 20: 'HTCLOSE'}
HT_NAME = {v: k for k, v in HT.items()}


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


def geometry(hwnd):
    window = w.RECT()
    client = w.RECT()
    origin = w.POINT()
    u.GetWindowRect(hwnd, c.byref(window))
    u.GetClientRect(hwnd, c.byref(client))
    u.ClientToScreen(hwnd, c.byref(origin))
    return {
        'window': [window.left, window.top, window.right, window.bottom],
        'client': [client.left, client.top, client.right, client.bottom],
        'client_origin_screen': [origin.x, origin.y],
        'window_size': [window.right - window.left, window.bottom - window.top],
        'client_size': [client.right - client.left, client.bottom - client.top],
    }


def hit_test(hwnd, x, y):
    lparam = (y & 0xFFFF) << 16 | (x & 0xFFFF)
    code = int(u.SendMessageW(hwnd, 0x84, 0, lparam)) & 0xFFFFFFFF
    return HT_NAME.get(code, str(code))


def style_of(hwnd):
    style = int(u.GetWindowLongPtrW(hwnd, GWL_STYLE))
    flags = []
    for name, value in (('WS_POPUP', WS_POPUP), ('WS_CAPTION', WS_CAPTION),
                        ('WS_THICKFRAME', WS_THICKFRAME), ('WS_SYSMENU', WS_SYSMENU),
                        ('WS_MINIMIZEBOX', WS_MINIMIZEBOX), ('WS_MAXIMIZEBOX', WS_MAXIMIZEBOX)):
        if style & value:
            flags.append(name)
    return {'raw': style, 'flags': flags}


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

    with tempfile.TemporaryDirectory(prefix='git-terminal-geom-') as temporary:
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

            before = geometry(hwnd)
            style = style_of(hwnd)
            # Probe the visible edge of the drawn content: the client-area boundary
            # is where a user perceives the window edge.
            left, top = before['client_origin_screen']
            cw, ch = before['client_size']
            probes = {
                'client_top_left': (left, top),
                'client_top_left+3': (left + 3, top + 3),
                'client_left_edge': (left, top + ch // 2),
                'client_left_edge+2': (left + 2, top + ch // 2),
                'client_left_edge+6': (left + 6, top + ch // 2),
                'client_right_edge-2': (left + cw - 3, top + ch // 2),
                'client_right_edge-6': (left + cw - 7, top + ch // 2),
                'client_bottom_edge-2': (left + cw // 2, top + ch - 3),
                'client_bottom_edge-6': (left + cw // 2, top + ch - 7),
                'client_top_edge+2': (left + cw // 2, top + 2),
                'outside_left': (left - 6, top + ch // 2),
                'outside_right': (left + cw + 6, top + ch // 2),
                'outside_bottom': (left + cw // 2, top + ch + 6),
            }
            hits = {name: hit_test(hwnd, x, y) for name, (x, y) in probes.items()}

            # Drag the visible right edge of the drawn content outwards.
            u.SetForegroundWindow(hwnd)
            time.sleep(.3)
            start = (left + cw - 2, top + ch // 2)
            u.SetCursorPos(*start)
            time.sleep(.3)
            cursor = w.POINT()
            u.GetCursorPos(c.byref(cursor))
            actual_start = [cursor.x, cursor.y]
            steps = []
            u.SetCursorPos(*start)
            time.sleep(.2)
            # Physical button press: the modal resize loop is driven by the mouse
            # state, so the press must be a real one.
            ctypes_input = None
            MOUSEEVENTF_LEFTDOWN = 0x0002
            MOUSEEVENTF_LEFTUP = 0x0004

            class Mouse(c.Structure):
                _fields_ = [('x', w.LONG), ('y', w.LONG), ('data', w.DWORD), ('flags', w.DWORD),
                            ('time', w.DWORD), ('extra', c.c_size_t)]

            class Payload(c.Union):
                _fields_ = [('mouse', Mouse)]

            class Input(c.Structure):
                _fields_ = [('type', w.DWORD), ('payload', Payload)]

            u.SendInput.argtypes = [w.UINT, c.POINTER(Input), c.c_int]
            u.SendInput.restype = w.UINT

            def send(flags):
                u.SendInput(1, c.byref(Input(0, Payload(mouse=Mouse(0, 0, 0, flags, 0, 0)))),
                            c.sizeof(Input))

            send(MOUSEEVENTF_LEFTDOWN)
            time.sleep(.3)
            for step in range(1, 13):
                u.SetCursorPos(start[0] + step * 20, start[1])
                time.sleep(.06)
                steps.append({'step': step, 'geometry': geometry(hwnd)})
            send(MOUSEEVENTF_LEFTUP)
            time.sleep(.5)
            after = geometry(hwnd)

            value = {
                'style': style,
                'before': before,
                'after': after,
                'actual_cursor_after_set': actual_start,
                'hit_tests': hits,
                'drag_steps': [{'step': s['step'], 'window_size': s['geometry']['window_size'],
                                'client_size': s['geometry']['client_size']} for s in steps],
                'resized': after['window_size'] != before['window_size'],
            }
        finally:
            if process.poll() is None:
                subprocess.run([os.path.join(os.environ['SystemRoot'], 'System32/taskkill.exe'),
                                '/PID', str(process.pid), '/T', '/F'], capture_output=True, timeout=10)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
    output.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(value, indent=2))
    raise SystemExit(0 if value['resized'] else 1)


if __name__ == '__main__':
    main()
