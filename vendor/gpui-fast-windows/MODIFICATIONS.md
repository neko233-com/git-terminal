# Vendored modifications: `gpui-fast-windows` 0.1.10

Vendored from the crates.io release of `gpui-fast-windows` 0.1.10 and patched so a
borderless window can be resized by dragging any edge or corner.

## Why

`git-terminal` opens a borderless window (`titlebar.appears_transparent = true` on Windows),
so `gpui-fast-windows` takes over `WM_NCHITTEST` with its own hit test instead of letting
`DefWindowProcW` do it. That hit test only ever returned `HTTOP` / `HTTOPLEFT` /
`HTTOPRIGHT`; it never returned `HTLEFT`, `HTRIGHT`, `HTBOTTOM`, `HTBOTTOMLEFT` or
`HTBOTTOMRIGHT`. `handle_calc_client_size` additionally restores only `rgrc[0].top` after
`DefWindowProcW(WM_NCCALCSIZE)`, which removes the non-client resize frame from the other
three edges, so the Win32 default cannot recover them either. Net effect: only a thin strip
along the top edge resizes; the sides, the bottom and all corners are inert.

The application cannot work around this either. `Window::start_window_resize` is a no-op on
the Windows backend, and `Window::resize` calls `SetWindowPos` with `SWP_NOMOVE`, so it
cannot move a corner-dragged window.

## Changes

### `src/events.rs` — `handle_hit_test_msg`

- Report a resize hit-test code for all four edges and all four corners instead of the top
  edge only, so every border and corner of the window starts a resize drag.
- Correct the frame-thickness arguments: `get_frame_thicknessx` measures horizontal metrics
  (`SM_CXSIZEFRAME`) and `get_frame_thicknessy` measures vertical ones (`SM_CYSIZEFRAME`).
  The upstream code passed them to each other's axis, which made the top border's grab
  thickness and the right-edge test wrong at non-square DPI scaling.
- Measure the draggable border against the **client** rectangle (`GetClientRect`) instead of
  the window rectangle (`GetWindowRect`). `WM_NCCALCSIZE` strips the system resize frame out
  of the window and leaves the visible border inside the client area, so the window rectangle
  is 8 px wider and taller than the region the user actually sees. Against the window
  rectangle the right and bottom thresholds sit past the last client pixel, and those two
  borders never become hittable.
- Corners take precedence over edges, matching the Win32 convention.
- `drag_area` still applies everywhere the cursor is not on a border, so the custom
  title bar keeps working.

Behaviour is unchanged for a maximized or fullscreen window, and unchanged when
`is_resizable` is false.

### `src/events.rs` / `src/window.rs` — application-driven resize gesture

Returning a resize hit-test code is not sufficient on its own. The native Windows resize
loop lives inside `DefWindowProcW(WM_NCLBUTTONDOWN)`, and it only runs when the application
*declines* the message. gpui's input callback claims the mouse-down for any widget under the
cursor, so the loop frequently never starts and the window silently refuses to resize. Which
edges fail depends on which widget happens to sit under the border, so the symptom is
intermittent rather than consistent.

The gesture is therefore driven by the application itself:

- `WindowsWindowState::resize_gesture` holds the in-flight gesture: the hit-test code that
  started it, the window rectangle at that moment, and the pointer's screen position.
- `handle_nc_mouse_down_msg` starts the gesture for the eight resize hit-test codes and
  claims the mouse capture, so the drag keeps working after the pointer leaves the window.
- `handle_nc_mouse_move_msg` and `handle_mouse_move_msg` apply the pointer's current screen
  position to the stored rectangle and reposition the window with `SetWindowPos`. The pointer
  stays authoritative, so the window tracks the mouse exactly the way the native loop does.
  Neither handler forwards the event to the application, so no widget reacts to the drag.
- `handle_nc_mouse_up_msg` and `handle_mouse_up_msg` end the gesture and release the capture.
- `WM_CAPTURECHANGED` / `WM_CANCELMODE` end the gesture as well, but only when the capture
  was not claimed by this window — `WM_CAPTURECHANGED` also reaches the window that called
  `SetCapture`, so without that distinction the gesture would tear itself down the instant
  it started. `WindowsWindowState::owns_capture` records that distinction.
- The gesture respects `WindowsWindowState::min_size` and never lets an edge cross the
  opposite one.

## Verification

`tests/resize_edge_probe.py` launches the terminal, resolves its window, and drives the real
mouse with `SetCursorPos` plus `SendInput` button state. Each target is pressed, dragged by
a known distance and released, so the resulting geometry change is attributable to that
specific target:

```
client-top-edge          hit=HTTOP          drag=[0, 60]    delta=[0, -60]   resized=True
client-bottom-edge       hit=HTBOTTOM       drag=[0, -60]   delta=[0, -60]   resized=True
client-left-edge         hit=HTLEFT         drag=[80, 0]    delta=[-80, 0]   resized=True
client-right-edge        hit=HTRIGHT        drag=[-80, 0]   delta=[-80, 0]   resized=True
top-left-corner          hit=HTTOPLEFT      drag=[60, 60]   delta=[-60,-60]  resized=True
bottom-right-corner      hit=HTBOTTOMRIGHT  drag=[-60,-60]  delta=[-60,-60]  resized=True
inset-2-right            hit=HTRIGHT        drag=[-80, 0]   delta=[-80, 0]   resized=True
inset-2-bottom           hit=HTBOTTOM       drag=[0, -60]   delta=[0, -60]   resized=True
```

`tests/resize_surface_map.py` scans the whole window on a grid and confirms the interactive
border is symmetric: the left column is `HTLEFT`, the right column is `HTRIGHT`, the first
row is `HTTOP`, the last row is `HTBOTTOM`, and all four corners report their corner codes.

`tests/windows_desktop_acceptance.py` covers the same behaviour in the project acceptance
suite as the case `border-drag-resizes-window-on-all-four-edges`.

## Clipboard access

`src/clipboard.rs` serializes clipboard open/close scopes within the process and retries
temporary Windows clipboard contention every 5 ms for at most 250 ms. Previously one
failed `OpenClipboard` silently discarded the complete read or write; clipboard observers
could therefore leave the preceding text in place during a rapid copy with metadata.

The existing text, metadata and embedded-NUL assertions stay unchanged. Two native
regressions cover overlapping callers and a separate, bounded test process holding the
clipboard through a real message window for 50 ms. The holder does not change clipboard
contents and is reaped by its parent. The Go terminal product gate preserves/restores all
clipboard formats around the original Rust release tests, including thin LTO.

## Upstream

`gpui-fast` is published by Longbridge (`https://github.com/longbridge/gpui-fast`) and
0.1.10 is the newest release on crates.io, so the fix cannot be obtained by upgrading.
Upstream Zed `gpui` does not have this defect: it forwards `DefWindowProcW` and keeps
`frame_thickness` of non-client area on all four edges.
