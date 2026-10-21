"""Native Windows terminal acceptance with disposable profiles and owned windows only."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from PIL import ImageGrab

u = c.WinDLL('user32', use_last_error=True)
k = c.WinDLL('kernel32', use_last_error=True)
ole = c.OleDLL('ole32')
u.SetProcessDPIAware()
u.GetForegroundWindow.restype = w.HWND
u.SetForegroundWindow.argtypes = [w.HWND]
u.IsWindowVisible.argtypes = [w.HWND]
u.OpenClipboard.argtypes = [w.HWND]
u.GetWindowThreadProcessId.argtypes = [w.HWND,c.POINTER(w.DWORD)]
u.GetClientRect.argtypes = [w.HWND,c.POINTER(w.RECT)]
u.ClientToScreen.argtypes = [w.HWND,c.POINTER(w.POINT)]
u.GetDpiForWindow.argtypes = [w.HWND]
u.SetWindowPos.argtypes = [w.HWND,w.HWND,c.c_int,c.c_int,c.c_int,c.c_int,w.UINT]
u.PostMessageW.argtypes = [w.HWND,w.UINT,w.WPARAM,w.LPARAM]
u.SetCursorPos.argtypes = [c.c_int,c.c_int]
u.GetClipboardData.argtypes = [w.UINT]
u.GetClipboardData.restype = w.HANDLE
u.SetClipboardData.argtypes = [w.UINT,w.HANDLE]
u.SetClipboardData.restype = w.HANDLE
k.GlobalAlloc.argtypes = [w.UINT,c.c_size_t]
k.GlobalAlloc.restype = w.HGLOBAL
k.GlobalLock.argtypes = [w.HGLOBAL]
k.GlobalLock.restype = c.c_void_p
k.GlobalUnlock.argtypes = [w.HGLOBAL]
ole.OleGetClipboard.argtypes = [c.POINTER(c.c_void_p)]
ole.OleSetClipboard.argtypes = [c.c_void_p]
u.GetClipboardOwner.restype = w.HWND
u.AttachThreadInput.argtypes = [w.DWORD,w.DWORD,w.BOOL]
u.SetFocus.argtypes = [w.HWND]
u.SetFocus.restype = w.HWND
u.ShowWindow.argtypes = [w.HWND,c.c_int]
k.GetCurrentThreadId.restype = w.DWORD
WINDOW_OWNERS = {}


class Keyboard(c.Structure):
    _fields_ = [('vk',w.WORD),('scan',w.WORD),('flags',w.DWORD),('time',w.DWORD),('extra',c.c_size_t)]


class Mouse(c.Structure):
    _fields_ = [('x',w.LONG),('y',w.LONG),('data',w.DWORD),('flags',w.DWORD),('time',w.DWORD),('extra',c.c_size_t)]


class Payload(c.Union):
    _fields_ = [('keyboard',Keyboard),('mouse',Mouse)]


class Input(c.Structure):
    _fields_ = [('type',w.DWORD),('payload',Payload)]


u.SendInput.argtypes = [w.UINT,c.POINTER(Input),c.c_int]
u.SendInput.restype = w.UINT


def wait_for(action, seconds=12):
    end = time.monotonic()+seconds
    while time.monotonic()<end:
        value = action()
        if value: return value
        time.sleep(.04)
    raise AssertionError('Owned terminal action did not complete in time')


def window_for(pid):
    matches=[]
    callback_type=c.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
    def visit(hwnd,_):
        actual=w.DWORD()
        u.GetWindowThreadProcessId(hwnd,c.byref(actual))
        if actual.value==pid and u.IsWindowVisible(hwnd): matches.append(hwnd)
        return True
    callback=callback_type(visit)
    u.EnumWindows(callback,0)
    if matches: WINDOW_OWNERS[matches[0]]=pid
    return matches[0] if matches else None


def focused(hwnd):
    actual=w.DWORD();u.GetWindowThreadProcessId(hwnd,c.byref(actual))
    assert actual.value==WINDOW_OWNERS.get(hwnd), 'Window ownership changed; input refused'
    assert u.GetForegroundWindow()==hwnd, 'Focus changed; no input sent to another application'


def activate_owned(hwnd):
    actual=w.DWORD();target_thread=u.GetWindowThreadProcessId(hwnd,c.byref(actual))
    assert actual.value==WINDOW_OWNERS.get(hwnd), 'Window ownership changed; activation refused'
    u.ShowWindow(hwnd,9)
    current=k.GetCurrentThreadId();foreground=u.GetForegroundWindow()
    foreground_thread=u.GetWindowThreadProcessId(foreground,None) if foreground else 0
    attached=[]
    try:
        for thread in set((target_thread,foreground_thread))-{0,current}:
            if u.AttachThreadInput(current,thread,True):attached.append(thread)
        u.SetForegroundWindow(hwnd);u.SetFocus(hwnd)
    finally:
        for thread in reversed(attached):u.AttachThreadInput(current,thread,False)
    wait_for(lambda:u.GetForegroundWindow()==hwnd,2)


def send(hwnd, events):
    focused(hwnd)
    values=(Input*len(events))(*events)
    assert u.SendInput(len(values),values,c.sizeof(Input))==len(values), 'Owned window input was refused'


def key(vk, up=False):
    return Input(1,Payload(keyboard=Keyboard(vk,0,2 if up else 0,0,0)))


def shortcut(hwnd,*keys):
    send(hwnd,[*(key(v) for v in keys),*(key(v,True) for v in reversed(keys))])


def type_text(hwnd,text):
    units=text.encode('utf-16-le')
    for offset in range(0,len(units),64):
        events=[]
        for index in range(offset,min(offset+64,len(units)),2):
            unit=int.from_bytes(units[index:index+2],'little')
            events += [Input(1,Payload(keyboard=Keyboard(0,unit,4,0,0))),Input(1,Payload(keyboard=Keyboard(0,unit,6,0,0)))]
        send(hwnd,events)
        time.sleep(.01)


def command(hwnd,text):
    type_text(hwnd,text)
    shortcut(hwnd,0x0D)


def bounds(hwnd):
    rect=w.RECT();point=w.POINT()
    assert u.GetClientRect(hwnd,c.byref(rect)) and u.ClientToScreen(hwnd,c.byref(point))
    assert rect.right>=500 and rect.bottom>=250, 'Owned terminal window is not available for capture'
    return point.x,point.y,point.x+rect.right,point.y+rect.bottom


def clipboard_text():
    wait_for(lambda:u.OpenClipboard(None),2)
    try:
        handle=u.GetClipboardData(13)
        if not handle:return ''
        pointer=k.GlobalLock(handle)
        try:return c.wstring_at(pointer)
        finally:k.GlobalUnlock(handle)
    finally:u.CloseClipboard()


def set_clipboard(hwnd,text):
    wait_for(lambda:u.OpenClipboard(hwnd),2)
    try:
        assert u.EmptyClipboard()
        data=text.encode('utf-16-le')+b'\0\0';handle=k.GlobalAlloc(0x42,len(data));pointer=k.GlobalLock(handle)
        c.memmove(pointer,data,len(data));k.GlobalUnlock(handle)
        assert u.SetClipboardData(13,handle)
    finally:u.CloseClipboard()


def main():
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--git',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();binary=args.binary.resolve(strict=True);git=args.git.resolve(strict=True)
    output=args.output.resolve();output.parent.mkdir(parents=True,exist_ok=True)
    results=[];process=None;original_clipboard=c.c_void_p();clipboard_saved=False;fixture_clipboard=''
    ole.OleInitialize(None)
    start=time.monotonic()
    with tempfile.TemporaryDirectory(prefix='git-terminal-desktop-') as temporary:
        root=Path(temporary);home=root/'home';repo=root/'中文 repo';home.mkdir();repo.mkdir()
        env={key:value for key,value in os.environ.items() if not key.startswith(('GIT_','GCM_','SSH_','GNUPG'))}
        env.update(HOME=str(home),USERPROFILE=str(home),APPDATA=str(home/'appdata'),LOCALAPPDATA=str(home/'local'),XDG_CONFIG_HOME=str(home/'.config'),GIT_CONFIG_GLOBAL=str(home/'.gitconfig'),GIT_NEXT_CONFIG_HOME=str(home/'git-next'),SSH_ROOT_PATH=str(home/'.ssh'),GIT_TERMINAL_PROMPT='0',GCM_INTERACTIVE='never')
        def record(name,action):
            then=time.monotonic()
            try:
                if process and process.poll() is None:
                    owned=window_for(process.pid)
                    if owned:
                        activate_owned(owned)
                action();results.append(dict(case=name,status='pass',seconds=round(time.monotonic()-then,3)))
            except (AssertionError,OSError,ValueError,IndexError,subprocess.SubprocessError) as error:
                results.append(dict(case=name,status='fail',error_type=type(error).__name__,reason=str(error)[:180],seconds=round(time.monotonic()-then,3)))
        def cli():
            value=subprocess.run([str(binary),'--print-setup','--json','--no-profile','--git',str(git),'--cwd',str(repo)],env=env,capture_output=True,timeout=15)
            assert value.returncode==0
            info=json.loads(value.stdout);assert info['product']=='git-terminal' and info['renderer']=='gpui-fast' and info['git'] and info['no_profile']
            for language in ('chinese','english','auto'):
                p=subprocess.run([str(binary),'language','set',language],env=env,capture_output=True,timeout=10);assert p.returncode==0 and json.loads(p.stdout)['configured']==language
        record('identity-profiles-unicode-cwd-language-atomic-settings',cli)
        try:
            subprocess.run([str(git),'init','--initial-branch=feature/tabs',str(repo)],env=env,capture_output=True,check=True,timeout=10)
            process=subprocess.Popen([str(binary),'--no-profile','--git',str(git),'--cwd',str(repo)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            hwnd=wait_for(lambda:window_for(process.pid));u.SetWindowPos(hwnd,None,40,40,1120,720,4);activate_owned(hwnd)
            time.sleep(2)
            def quote(path):
                return "'"+str(path).replace('\\','/').replace("'","'\\''")+"'"
            def powershell(source):
                executable=os.path.join(os.environ['SystemRoot'],'System32/WindowsPowerShell/v1.0/powershell.exe')
                return '"'+executable+'" -NoLogo -NoProfile -NonInteractive -Command "'+source+'"'
            def typed():
                path=root/'unicode.txt';command(hwnd,"printf '%s' '中文🙂' > "+quote(path))
                wait_for(path.exists);assert path.read_text(encoding='utf-8')=='中文🙂'
            record('real-window-native-unicode-text-input-and-pty',typed)
            def paste():
                nonlocal clipboard_saved,fixture_clipboard
                focused(hwnd)
                assert ole.OleGetClipboard(c.byref(original_clipboard))>=0;clipboard_saved=True
                path=root/'clipboard.txt';fixture_clipboard="printf '%s' 'clipboard-中文' > "+quote(path);set_clipboard(hwnd,fixture_clipboard)
                shortcut(hwnd,0x11,0x10,0x56);shortcut(hwnd,0x0D);wait_for(path.exists);assert path.read_text(encoding='utf-8')=='clipboard-中文'
            record('native-clipboard-paste-with-original-preserved-in-memory',paste)
            def tabs():
                first=root/'first.txt';second=root/'second.txt';returned=root/'returned.txt'
                command(hwnd,"GIT_TERMINAL_TAB=first; printf first > "+quote(first));wait_for(first.exists)
                shortcut(hwnd,0x11,0x54);time.sleep(1)
                command(hwnd,"printf '%s' \"${GIT_TERMINAL_TAB-independent}\" > "+quote(second));wait_for(second.exists)
                assert second.read_text().strip()=='independent'
                shortcut(hwnd,0x11,0x09);time.sleep(.15)
                shortcut(hwnd,0x11,0x10,0x09);time.sleep(.15)
                shortcut(hwnd,0x11,0x57);time.sleep(.3)
                command(hwnd,"printf '%s' \"$GIT_TERMINAL_TAB\" > "+quote(returned));wait_for(returned.exists);assert returned.read_text().strip()=='first'
            record('native-keyboard-tabs-independent-shells-and-close',tabs)
            def resize():
                before=root/'width-before.txt';after=root/'width-after.txt'
                command(hwnd,"printf '%s' \"$COLUMNS\" > "+quote(before));wait_for(before.exists)
                u.SetWindowPos(hwnd,None,40,40,920,650,4);time.sleep(.5)
                command(hwnd,"printf '%s' \"$COLUMNS\" > "+quote(after));wait_for(after.exists)
                assert int(before.read_text())!=int(after.read_text())
            record('native-window-resize-reaches-conpty',resize)
            def rendering():
                ready=root/'render.txt';command(hwnd,"printf '\\033[2J\\033[HGit-Terminal 中文 / UTF-8\\nCOPY_MARKER_GIT_TERMINAL\\n'; printf ready > "+quote(ready))
                wait_for(ready.exists);time.sleep(.4)
                image=ImageGrab.grab(bbox=bounds(hwnd));assert len(image.getcolors(image.width*image.height) or [])>20
                image.save(output.with_suffix('.png'))
            record('actual-gpui-gpu-window-chinese-and-color-screenshot',rendering)
            def selection():
                assert clipboard_saved, "Clipboard snapshot is required before copy"
                image=ImageGrab.grab(bbox=bounds(hwnd));x0,y0,x1,y1=bounds(hwnd);scale=u.GetDpiForWindow(hwnd)/96
                background=image.getpixel((image.width-3,image.height//2))
                top=next(y for y in range(10,min(200,image.height//2)) if all(image.getpixel((image.width-3,y+offset))==background for offset in range(5)))
                bands=[];active=None
                for y in range(top+8,min(top+160,image.height-45)):
                    lit=any(max(image.getpixel((x,y))[:3])>160 for x in range(12,min(350,image.width-20)))
                    if lit and active is None:active=y
                    if not lit and active is not None:bands.append((active,y));active=None
                target_y=(bands[1][0]+bands[1][1])/2 if len(bands)>1 else top+43*scale
                copied=False
                for dy in (0,-3,3):
                    focused(hwnd);u.SetCursorPos(int(x0+25*scale),int(y0+target_y+dy*scale))
                    for _ in range(2):
                        send(hwnd,[Input(0,Payload(mouse=Mouse(0,0,0,2,0,0))),Input(0,Payload(mouse=Mouse(0,0,0,4,0,0)))]);time.sleep(.08)
                    shortcut(hwnd,0x11,0x10,0x43);time.sleep(.15)
                    if 'COPY_MARKER_GIT_TERMINAL' in clipboard_text():copied=True;break
                assert copied
            record('native-mouse-selection-and-copy',selection)
            def history():
                ready=root/'history.txt';command(hwnd,"printf 'history-%s\\n' {1..180}; printf ready > "+quote(ready));wait_for(ready.exists);time.sleep(.3)
                image=ImageGrab.grab(bbox=bounds(hwnd));region=(5,100,min(360,image.width),image.height-45)
                before=hashlib.sha256(image.crop(region).tobytes()).hexdigest()
                shortcut(hwnd,0x10,0x21);time.sleep(.25)
                after=hashlib.sha256(ImageGrab.grab(bbox=bounds(hwnd)).crop(region).tobytes()).hexdigest();assert before!=after
                shortcut(hwnd,0x10,0x22);time.sleep(.2)
                x0,y0,x1,y1=bounds(hwnd);focused(hwnd);u.SetCursorPos(x0+150,y0+250)
                send(hwnd,[Input(0,Payload(mouse=Mouse(0,0,120*3,0x0800,0,0)))]);time.sleep(.25)
                wheel=hashlib.sha256(ImageGrab.grab(bbox=bounds(hwnd)).crop(region).tobytes()).hexdigest();assert wheel!=before
            record('native-shift-page-and-mouse-wheel-scrollback',history)
        finally:
            try:
                if clipboard_saved:
                    deadline=time.monotonic()+3
                    while True:
                        try:
                            owner=u.GetClipboardOwner();owner_pid=w.DWORD()
                            if owner:u.GetWindowThreadProcessId(owner,c.byref(owner_pid))
                            owned_text=clipboard_text()
                            if owner_pid.value in (os.getpid(),process.pid if process else 0) or owned_text==fixture_clipboard or owned_text.strip()=='COPY_MARKER_GIT_TERMINAL':
                                restored=ole.OleSetClipboard(original_clipboard)
                                if restored>=0:restored=ole.OleFlushClipboard()
                                assert restored>=0, 'Original clipboard restoration was refused'
                                results.append(dict(case='original-clipboard-restored',status='pass'))
                            else:
                                results.append(dict(case='new-external-clipboard-change-preserved',status='pass'))
                            break
                        except (OSError,AssertionError):
                            if time.monotonic()>=deadline:
                                results.append(dict(case='original-clipboard-restored',status='fail',reason='Clipboard remained busy'))
                                break
                            time.sleep(.1)
            finally:
                if original_clipboard:
                    table=c.cast(original_clipboard,c.POINTER(c.POINTER(c.c_void_p))).contents
                    c.WINFUNCTYPE(w.ULONG,c.c_void_p)(table[2])(original_clipboard)
                if process:
                    hwnd=window_for(process.pid)
                    if hwnd:u.PostMessageW(hwnd,0x10,0,0)
                    try:process.wait(timeout=15)
                    except subprocess.TimeoutExpired:
                        subprocess.run([os.path.join(os.environ['SystemRoot'],'System32/taskkill.exe'),'/PID',str(process.pid),'/T','/F'],capture_output=True,timeout=10)
                        process.wait(timeout=5)
                        results.append(dict(case='owned-window-exit-and-process-cleanup',status='fail'))
                    else:results.append(dict(case='owned-window-exit-and-process-cleanup',status='pass'))
        def profiles():
            for profile,command_text in [('powershell',"[IO.File]::WriteAllText('"+(root/'powershell.txt').as_posix()+"','powershell-ok')"),('bash',"printf 'bash-ok' > '"+(root/'bash.txt').as_posix()+"'"),('cmd','echo cmd-ok>"'+str(root/'cmd.txt')+'"')]:
                p=subprocess.Popen([str(binary),'--shell',profile,'--no-profile','--git',str(git),'--cwd',str(repo),'--command',command_text],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                try:
                    status=p.wait(timeout=20);assert status==0,profile+" command exit="+str(status)
                finally:
                    if p.poll() is None:subprocess.run([os.path.join(os.environ['SystemRoot'],'System32/taskkill.exe'),'/PID',str(p.pid),'/T','/F'],capture_output=True,timeout=10)
                assert (root/(profile+'.txt')).read_text().strip()==profile+'-ok'
        record('owned-bash-and-native-cmd-real-gui-pty-exit',profiles)
        def exit_status():
            p=subprocess.Popen([str(binary),'--shell','cmd','--no-profile','--git',str(git),'--cwd',str(repo),'--command','exit 7'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            try:
                status=p.wait(timeout=20);assert status==7,"command exit="+str(status)
            finally:
                if p.poll() is None:subprocess.run([os.path.join(os.environ['SystemRoot'],'System32/taskkill.exe'),'/PID',str(p.pid),'/T','/F'],capture_output=True,timeout=10)
        record('explicit-command-exit-status-propagation',exit_status)
        def burst_exit():
            p=subprocess.Popen([str(binary),'--shell','powershell','--no-profile','--git',str(git),'--cwd',str(repo),'--command',"[Console]::Write(('x' * 1048576)); exit 9"],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            try:
                status=p.wait(timeout=30);assert status==9,"large-output command exit="+str(status)
            finally:
                if p.poll() is None:subprocess.run([os.path.join(os.environ['SystemRoot'],'System32/taskkill.exe'),'/PID',str(p.pid),'/T','/F'],capture_output=True,timeout=10)
        record('bounded-output-drain-and-command-exit-without-ui-deadlock',burst_exit)

    value=dict(status='pass' if all(item['status']=='pass' for item in results) else 'fail',platform='windows-x64',renderer='gpui-fast',binary_sha256=hashlib.file_digest(binary.open('rb'),'sha256').hexdigest(),seconds=round(time.monotonic()-start,3),personal_profile_accessed=False,terminal_contents_logged=False,results=results)
    output.write_text(json.dumps(value,indent=2)+'\n',encoding='utf-8');print(json.dumps(value))
    raise SystemExit(value['status']!='pass')


if __name__=='__main__':main()
