"""Map the owned terminal window's hit-test surface across a grid.

The probe owns the window it inspects: it launches the terminal itself and resolves
the HWND from that PID. It reports which screen regions behave as caption (window
drag), as resize edges, and as plain client area, so the interactive border of a
borderless window is measured instead of assumed.
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

HT = {0: 'HTNOWHERE', 1: 'HTCLIENT', 2: 'HTCAPTION', 3: 'HTSYSMENU', 4: 'HTGROWBOX',
      8: 'HTMINBUTTON', 9: 'HTMAXBUTTON', 10: 'HTLEFT', 11: 'HTRIGHT', 12: 'HTTOP',
      13: 'HTTOPLEFT', 14: 'HTTOPRIGHT', 15: 'HTBOTTOM', 16: 'HTBOTTOMLEFT',
      17: 'HTBOTTOMRIGHT', 20: 'HTCLOSE'}


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
    code = int(u.SendMessageW(hwnd, 0x84, 0, lparam)) & 0xFFFFFFFF
    return HT.get(code, str(code))


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--git', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--step', type=int, default=16)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    git = args.git.resolve(strict=True)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix='git-terminal-surface-') as temporary:
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

            window = w.RECT()
            client = w.RECT()
            origin = w.POINT()
            u.GetWindowRect(hwnd, c.byref(window))
            u.GetClientRect(hwnd, c.byref(client))
            u.ClientToScreen(hwnd, c.byref(origin))
            width = window.right - window.left
            height = window.bottom - window.top
            step = args.step

            rows = []
            for y in range(window.top, window.bottom, step):
                row = []
                for x in range(window.left, window.right, step):
                    row.append(hit_test(hwnd, x, y))
                rows.append(row)

            legend = {}
            for row in rows:
                for value in row:
                    legend[value] = legend.get(value, 0) + 1

            value = {
                'window_rect': [window.left, window.top, window.right, window.bottom],
                'client_origin_screen': [origin.x, origin.y],
                'client_size': [client.right - client.left, client.bottom - client.top],
                'step': step,
                'legend': legend,
                'rows': rows,
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
    symbol = {'HTCLIENT': '.', 'HTCAPTION': 'C', 'HTLEFT': 'L', 'HTRIGHT': 'R', 'HTTOP': 'T',
              'HTBOTTOM': 'B', 'HTTOPLEFT': '1', 'HTTOPRIGHT': '2', 'HTBOTTOMLEFT': '3',
              'HTBOTTOMRIGHT': '4', 'HTCLOSE': 'X', 'HTMAXBUTTON': 'M', 'HTMINBUTTON': 'N',
              'HTNOWHERE': '0', 'HTSYSMENU': 'S', 'HTGROWBOX': 'G'}
    for index, row in enumerate(value['rows']):
        print('%3d ' % (value['window_rect'][1] + index * step) + ''.join(symbol.get(cell, '?') for cell in row))
    print(json.dumps({'legend': value['legend']}, indent=2))
    # The border of a borderless window has to be symmetric: all four edges must offer a
    # resize handle and all four corners must offer their corner handle. An asymmetric map
    # means one of the edges is dead, which is the defect this probe was written to catch.
    legend = value['legend']
    required = ('HTLEFT', 'HTRIGHT', 'HTTOP', 'HTBOTTOM',
                'HTTOPLEFT', 'HTTOPRIGHT', 'HTBOTTOMLEFT', 'HTBOTTOMRIGHT')
    missing = [code for code in required if not legend.get(code)]
    if missing:
        print('missing resize handle codes: %s' % ', '.join(missing))
        raise SystemExit(1)
    raise SystemExit(0)


if __name__ == '__main__':
    main()
