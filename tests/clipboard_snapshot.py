"""Copy clipboard formats eagerly in memory, without retaining a remote OLE proxy."""
import ctypes as c
from ctypes import wintypes as w

u = c.WinDLL('user32', use_last_error=True)
k = c.WinDLL('kernel32', use_last_error=True)
g = c.WinDLL('gdi32', use_last_error=True)
u.OpenClipboard.argtypes = [w.HWND]
u.GetClipboardData.argtypes = [w.UINT]; u.GetClipboardData.restype = w.HANDLE
u.SetClipboardData.argtypes = [w.UINT, w.HANDLE]; u.SetClipboardData.restype = w.HANDLE
u.EnumClipboardFormats.argtypes = [w.UINT]; u.EnumClipboardFormats.restype = w.UINT
u.CopyImage.argtypes = [w.HANDLE, w.UINT, c.c_int, c.c_int, w.UINT]; u.CopyImage.restype = w.HANDLE
k.GlobalSize.argtypes = [w.HGLOBAL]; k.GlobalSize.restype = c.c_size_t
k.GlobalLock.argtypes = [w.HGLOBAL]; k.GlobalLock.restype = c.c_void_p
k.GlobalUnlock.argtypes = [w.HGLOBAL]
k.GlobalAlloc.argtypes = [w.UINT, c.c_size_t]; k.GlobalAlloc.restype = w.HGLOBAL
k.GlobalFree.argtypes = [w.HGLOBAL]; k.GlobalFree.restype = w.HGLOBAL
g.CopyEnhMetaFileW.argtypes = [w.HANDLE, w.LPCWSTR]; g.CopyEnhMetaFileW.restype = w.HANDLE
g.CopyMetaFileW.argtypes = [w.HANDLE, w.LPCWSTR]; g.CopyMetaFileW.restype = w.HANDLE
g.GetPaletteEntries.argtypes = [w.HANDLE, w.UINT, w.UINT, c.c_void_p]
g.CreatePalette.argtypes = [c.c_void_p]; g.CreatePalette.restype = w.HANDLE
for name in ('DeleteObject', 'DeleteEnhMetaFile', 'DeleteMetaFile'):
    getattr(g, name).argtypes = [w.HANDLE]


class MetaPicture(c.Structure):
    _fields_ = [('mode', w.LONG), ('x', w.LONG), ('y', w.LONG), ('metafile', w.HANDLE)]


def allocate(data):
    handle = k.GlobalAlloc(0x42, max(1, len(data)))
    if not handle: raise c.WinError(c.get_last_error())
    pointer = k.GlobalLock(handle)
    if not pointer:
        k.GlobalFree(handle); raise c.WinError(c.get_last_error())
    try: c.memmove(pointer, data, len(data))
    finally: k.GlobalUnlock(handle)
    return handle


class ClipboardSnapshot:
    def __init__(self):
        self.formats = []

    def capture(self):
        assert u.OpenClipboard(None), 'Clipboard is busy; no content was changed'
        try:
            fmt = 0
            while True:
                c.set_last_error(0); fmt = u.EnumClipboardFormats(fmt)
                if not fmt:
                    assert c.get_last_error() == 0, 'Clipboard enumeration failed before mutation'
                    break
                assert len(self.formats) < 256, 'Clipboard format limit; no content was changed'
                handle = u.GetClipboardData(fmt)
                assert handle, 'Clipboard format unavailable; no content was changed'
                if fmt in (2, 0x82):
                    value = u.CopyImage(handle, 0, 0, 0, 0x2000)
                    assert value; self.formats.append((fmt, 'bitmap', value)); continue
                if fmt in (14, 0x8e):
                    value = g.CopyEnhMetaFileW(handle, None)
                    assert value; self.formats.append((fmt, 'enhanced', value)); continue
                if fmt == 9:
                    count = g.GetPaletteEntries(handle, 0, 0, None)
                    assert 0 < count <= 65535
                    entries = c.create_string_buffer(count*4)
                    assert g.GetPaletteEntries(handle, 0, count, entries) == count
                    value = (0x300).to_bytes(2, 'little')+count.to_bytes(2, 'little')+entries.raw
                    self.formats.append((fmt, 'palette', value)); continue
                size = k.GlobalSize(handle)
                assert 0 < size <= 256*1024*1024, 'Unsupported clipboard handle; no content was changed'
                pointer = k.GlobalLock(handle)
                assert pointer
                try:
                    if fmt in (3, 0x83):
                        picture = MetaPicture.from_address(pointer)
                        copied = g.CopyMetaFileW(picture.metafile, None)
                        assert copied
                        self.formats.append((fmt, 'metafile', (picture.mode, picture.x, picture.y, copied)))
                    else: self.formats.append((fmt, 'memory', c.string_at(pointer, size)))
                finally: k.GlobalUnlock(handle)
        except BaseException:
            self.close(); raise
        finally: u.CloseClipboard()

    def restore(self, hwnd):
        assert u.OpenClipboard(hwnd), 'Clipboard is busy'
        prepared = []
        try:
            for fmt, kind, value in self.formats:
                if kind == 'memory': handle = allocate(value)
                elif kind == 'bitmap': handle = u.CopyImage(value, 0, 0, 0, 0x2000)
                elif kind == 'enhanced': handle = g.CopyEnhMetaFileW(value, None)
                elif kind == 'palette': handle = g.CreatePalette(c.create_string_buffer(value))
                else:
                    mode, x, y, original = value
                    copied = g.CopyMetaFileW(original, None)
                    assert copied
                    picture = MetaPicture(mode, x, y, copied)
                    handle = allocate(bytes(picture))
                assert handle, 'Clipboard copy allocation failed before mutation'
                prepared.append((fmt, kind, handle))
            assert u.EmptyClipboard()
            while prepared:
                fmt, kind, handle = prepared[0]
                assert u.SetClipboardData(fmt, handle), 'Clipboard restoration failed'
                prepared.pop(0)
        finally:
            for _, kind, handle in prepared:
                if kind == 'bitmap': g.DeleteObject(handle)
                elif kind == 'enhanced': g.DeleteEnhMetaFile(handle)
                elif kind == 'palette': g.DeleteObject(handle)
                else: k.GlobalFree(handle)
            u.CloseClipboard()

    def close(self):
        for _, kind, value in self.formats:
            if kind == 'bitmap': g.DeleteObject(value)
            elif kind == 'enhanced': g.DeleteEnhMetaFile(value)
            elif kind == 'metafile': g.DeleteMetaFile(value[3])
        self.formats.clear()
