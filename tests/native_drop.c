/* Copyright 2026 git-next contributors. MIT license. Test-only native Explorer-style file drag. */
#define COBJMACROS
#include <windows.h>
#include <ole2.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <stdio.h>

typedef struct { IDropSource iface; LONG refs; ULONGLONG started; } Source;
static HRESULT STDMETHODCALLTYPE query(IDropSource *self, REFIID iid, void **result) {
    if (IsEqualIID(iid, &IID_IUnknown) || IsEqualIID(iid, &IID_IDropSource)) {
        *result = self; IDropSource_AddRef(self); return S_OK;
    }
    *result = NULL; return E_NOINTERFACE;
}
static ULONG STDMETHODCALLTYPE retain(IDropSource *self) { return InterlockedIncrement(&((Source *)self)->refs); }
static ULONG STDMETHODCALLTYPE release(IDropSource *self) { return InterlockedDecrement(&((Source *)self)->refs); }
static HRESULT STDMETHODCALLTYPE proceed(IDropSource *self, BOOL escape, DWORD keys) {
    if (escape || GetTickCount64() - ((Source *)self)->started > 5000) return DRAGDROP_S_CANCEL;
    return keys & MK_LBUTTON ? S_OK : DRAGDROP_S_DROP;
}
static HRESULT STDMETHODCALLTYPE feedback(IDropSource *self, DWORD effect) {
    (void)self; (void)effect; return DRAGDROP_S_USEDEFAULTCURSORS;
}
static IDropSourceVtbl methods = { query, retain, release, proceed, feedback };

typedef struct { IDataObject iface; LONG refs; const wchar_t *path; } Data;
static HRESULT STDMETHODCALLTYPE data_query(IDataObject *self, REFIID iid, void **result) {
    if (IsEqualIID(iid, &IID_IUnknown) || IsEqualIID(iid, &IID_IDataObject)) { *result = self; IDataObject_AddRef(self); return S_OK; }
    *result = NULL; return E_NOINTERFACE;
}
static ULONG STDMETHODCALLTYPE data_retain(IDataObject *self) { return InterlockedIncrement(&((Data *)self)->refs); }
static ULONG STDMETHODCALLTYPE data_release(IDataObject *self) { return InterlockedDecrement(&((Data *)self)->refs); }
static HRESULT STDMETHODCALLTYPE data_format(IDataObject *self, FORMATETC *format) {
    (void)self; return format->cfFormat == CF_HDROP && (format->tymed & TYMED_HGLOBAL) ? S_OK : DV_E_FORMATETC;
}
static HRESULT STDMETHODCALLTYPE data_get(IDataObject *self, FORMATETC *format, STGMEDIUM *medium) {
    if (FAILED(data_format(self, format))) return DV_E_FORMATETC;
    const wchar_t *path = ((Data *)self)->path;
    SIZE_T size = sizeof(DROPFILES) + (wcslen(path) + 2) * sizeof(wchar_t);
    HGLOBAL memory = GlobalAlloc(GHND, size);
    if (!memory) return E_OUTOFMEMORY;
    DROPFILES *files = GlobalLock(memory);
    files->pFiles = sizeof(DROPFILES); files->fWide = TRUE;
    wcscpy((wchar_t *)((BYTE *)files + sizeof(DROPFILES)), path);
    GlobalUnlock(memory);
    medium->tymed = TYMED_HGLOBAL; medium->hGlobal = memory; medium->pUnkForRelease = NULL;
    return S_OK;
}
static HRESULT STDMETHODCALLTYPE data_here(IDataObject *self, FORMATETC *f, STGMEDIUM *m) { (void)self; (void)f; (void)m; return E_NOTIMPL; }
static HRESULT STDMETHODCALLTYPE data_canonical(IDataObject *self, FORMATETC *in, FORMATETC *out) { (void)self; (void)in; out->ptd = NULL; return DATA_S_SAMEFORMATETC; }
static HRESULT STDMETHODCALLTYPE data_set(IDataObject *self, FORMATETC *f, STGMEDIUM *m, BOOL release) { (void)self; (void)f; (void)m; (void)release; return E_NOTIMPL; }
static HRESULT STDMETHODCALLTYPE data_enum(IDataObject *self, DWORD direction, IEnumFORMATETC **out) {
    (void)self; FORMATETC f = { CF_HDROP, NULL, DVASPECT_CONTENT, -1, TYMED_HGLOBAL };
    return direction == DATADIR_GET ? SHCreateStdEnumFmtEtc(1, &f, out) : E_NOTIMPL;
}
static HRESULT STDMETHODCALLTYPE data_advise(IDataObject *self, FORMATETC *f, DWORD flags, IAdviseSink *sink, DWORD *connection) { (void)self; (void)f; (void)flags; (void)sink; (void)connection; return OLE_E_ADVISENOTSUPPORTED; }
static HRESULT STDMETHODCALLTYPE data_unadvise(IDataObject *self, DWORD connection) { (void)self; (void)connection; return OLE_E_ADVISENOTSUPPORTED; }
static HRESULT STDMETHODCALLTYPE data_enum_advise(IDataObject *self, IEnumSTATDATA **out) { (void)self; (void)out; return OLE_E_ADVISENOTSUPPORTED; }
static IDataObjectVtbl data_methods = { data_query, data_retain, data_release, data_get, data_here, data_format, data_canonical, data_set, data_enum, data_advise, data_unadvise, data_enum_advise };


typedef struct { Source source; Data data; HRESULT result; DWORD effect; } Drag;
static LRESULT CALLBACK drag_window(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    Drag *drag = (Drag *)GetWindowLongPtrW(window, GWLP_USERDATA);
    if (message == WM_LBUTTONDOWN && drag) {
        drag->source.started = GetTickCount64();
        drag->result = DoDragDrop(&drag->data.iface, &drag->source.iface, DROPEFFECT_COPY, &drag->effect);
        fprintf(stderr, "result:%lx effect:%lu\n", (unsigned long)drag->result, drag->effect); fflush(stderr);
        DestroyWindow(window);
        return 0;
    }
    if (message == WM_DESTROY) { PostQuitMessage(0); return 0; }
    return DefWindowProcW(window, message, wparam, lparam);
}
int wmain(int argc, wchar_t **argv)
{
    SetProcessDPIAware();
    if (argc != 3 || FAILED(OleInitialize(NULL))) return 1;
    Drag drag = { { { &methods }, 1, 0 }, { { &data_methods }, 1, argv[1] }, E_FAIL, 0 };
    WNDCLASSW wc = {0}; wc.lpfnWndProc = drag_window; wc.hInstance = GetModuleHandleW(NULL); wc.lpszClassName = L"GitTerminalDragFixture";
    if (!RegisterClassW(&wc)) { OleUninitialize(); return 1; }
    HWND window = CreateWindowExW(WS_EX_TOOLWINDOW, wc.lpszClassName, L"Native file drag fixture", WS_POPUP | WS_VISIBLE, 60, 90, 240, 80, NULL, NULL, wc.hInstance, NULL);
    if (!window) { OleUninitialize(); return 1; }
    SetWindowLongPtrW(window, GWLP_USERDATA, (LONG_PTR)&drag);
    FILE *ready = _wfopen(argv[2], L"w");
    if (ready) { fprintf(ready, "%llu", (unsigned long long)(UINT_PTR)window); fclose(ready); }
    MSG message;
    while (GetMessageW(&message, NULL, 0, 0) > 0) { TranslateMessage(&message); DispatchMessageW(&message); }
    OleUninitialize();
    return drag.result == DRAGDROP_S_DROP && drag.effect == DROPEFFECT_COPY ? 0 : 1;
}
