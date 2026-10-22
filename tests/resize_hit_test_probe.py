"""Probe the owned terminal window's resize hit-test geometry on all four edges and corners.

The probe owns the window it inspects: it launches the terminal itself, resolves the
HWND from that PID, and only reads geometry and hit-test codes. It never sends input
to any window it does not own.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import subprocess
import sys
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
u.GetDpiForWindow.argtypes = [w.HWND]

WM_NCHITTEST = 0x84
HT = {
    'HTCLIENT': 1, 'HTCAPTION': 2, 'HTSYSMENU': 3, 'HTGROWBOX': 4, 'HTMENU': 5,
    'HTHSCROLL': 6, 'HTMINBUTTON': 8, 'HTMAXBUTTON': 9, 'HTLEFT': 10, 'HTRIGHT': 11,
    'HTTOP': 12, 'HTTOPLEFT': 13, 'HTTOPRIGHT': 14, 'HTBOTTOM': 15, 'HTBOTTOMLEFT': 16,
    'HTBOTTOMRIGHT': 17, 'HTCLOSE': 20, 'HTNOWHERE': 0xFFFFFFFF,
}
HT_NAME = {value: name for name, value in HT.items()}


def window_for(pid):
    matches = []
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)

    def visit(hwnd, _):
        actual = w.DWORD()
        u.GetWindowThreadProcessId(hwnd, c.byref(actual))
        if actual.value == pid and u.IsWindowVisible(hwnd):
            matches.append(hwnd)
        return True

    c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)(visit)(0) if False else None
    callback = callback_type(visit)
    u.EnumWindows(callback, 0)
    return matches[0] if matches else None


def hit_test(hwnd, screen_x, screen_y):
    lparam = (screen_y & 0xFFFF) << 16 | (screen_x & 0xFFFF)
    return int(u.SendMessageW(hwnd, WM_NCHITTEST, 0, lparam))


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

    with tempfile.TemporaryDirectory(prefix='git-terminal-resize-') as temporary:
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
                    time.sleep(0.1)
            if hwnd is None:
                raise AssertionError('The owned terminal window did not appear')

            window_rect = w.RECT()
            u.GetWindowRect(hwnd, c.byref(window_rect))
            client_rect = w.RECT()
            u.GetClientRect(hwnd, c.byref(client_rect))
            origin = w.POINT()
            u.ClientToScreen(hwnd, c.byref(origin))
            scale = u.GetDpiForWindow(hwnd) / 96

            width = window_rect.right - window_rect.left
            height = window_rect.bottom - window_rect.top
            samples = []
            # Probe a band just inside each edge and just outside each edge, plus the
            # four corners, so both the reported code and the reported thickness are
            # observable rather than assumed.
            probes = []
            mid_x = window_rect.left + width // 2
            mid_y = window_rect.top + height // 2
            for offset in (-2, -1, 0, 1, 2, 3, 4, 6, 8, 10, 12):
                probes.append(('left', window_rect.left + offset, mid_y))
                probes.append(('right', window_rect.right - 1 - offset, mid_y))
                probes.append(('top', mid_x, window_rect.top + offset))
                probes.append(('bottom', mid_x, window_rect.bottom - 1 - offset))
            for offset in (-1, 0, 1, 2, 4, 8):
                probes.append(('topleft', window_rect.left + offset, window_rect.top + offset))
                probes.append(('topright', window_rect.right - 1 - offset, window_rect.top + offset))
                probes.append(('bottomleft', window_rect.left + offset, window_rect.bottom - 1 - offset))
                probes.append(('bottomright', window_rect.right - 1 - offset, window_rect.bottom - 1 - offset))
            for edge, x, y in probes:
                code = hit_test(hwnd, x, y)
                samples.append({'edge': edge, 'dx': x - window_rect.left, 'dy': y - window_rect.top,
                                'code': code, 'name': HT_NAME.get(code & 0xFFFFFFFF, str(code))})

            def first_code(edge, wanted):
                for sample in samples:
                    if sample['edge'] == edge and sample['name'] == wanted:
                        return sample
                return None

            expectation = {
                'left': 'HTLEFT', 'right': 'HTRIGHT', 'top': 'HTTOP', 'bottom': 'HTBOTTOM',
            }
            edges = {}
            for edge, wanted in expectation.items():
                sample = first_code(edge, wanted)
                edges[edge] = {'reported': sample, 'ok': sample is not None}
            corners = {}
            for edge, wanted in (('topleft', 'HTTOPLEFT'), ('topright', 'HTTOPRIGHT'),
                                 ('bottomleft', 'HTBOTTOMLEFT'), ('bottomright', 'HTBOTTOMRIGHT')):
                sample = first_code(edge, wanted)
                corners[edge] = {'reported': sample, 'ok': sample is not None}

            value = {
                'status': 'pass' if all(item['ok'] for item in edges.values()) and all(item['ok'] for item in corners.values()) else 'fail',
                'window_rect': [window_rect.left, window_rect.top, window_rect.right, window_rect.bottom],
                'client_rect': [client_rect.left, client_rect.top, client_rect.right, client_rect.bottom],
                'client_origin_screen': [origin.x, origin.y],
                'dpi_scale': scale,
                'edges': edges,
                'corners': corners,
                'samples': samples,
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
    raise SystemExit(0 if value['status'] == 'pass' else 1)


if __name__ == '__main__':
    main()
