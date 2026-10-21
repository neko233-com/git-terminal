# Attributed terminal component

Source: https://github.com/zortax/gpui-terminal, crates.io release 0.1.0.
Copyright Leonard Seibold. Original MIT / Apache-2.0 licenses are preserved.

Local changes by git-next contributors, 2026:

- Pin GPUI Fast 0.1.10 and adapt text, focus and input APIs.
- Add native IME commit/composition and caret geometry, copy/paste and bracketed paste.
- Implement mouse selection, semantic/line selection, history and application mouse reports.
- Render historical viewport rows and selected cells; preserve hidden cursor behavior.
- Bound the PTY reader queue, coalesce output, and yield to input/window work under continuous output. Coalesce hidden-tab titles/bells.
- Send PTY writes through an ordered worker so pipe backpressure cannot block the UI thread.
- Route protocol replies, color queries and size requests back to the owned PTY, including hidden tabs. Clipboard queries return an empty value without exposing clipboard contents.
- Apply configured scrollback and keep lifecycle under the host application.
- Reserve host tab shortcuts Ctrl+T/W, Ctrl+Shift+T/W and Ctrl+Tab before PTY input.
- Color managed Bash input natively, without changing shell input/output or its readline editor.
- Retry transient clipboard contention asynchronously without blocking rendering.

Alacritty provides VT parsing and grid semantics. Portable-pty provides ConPTY.
The host owns shells, tabs, Windows jobs, settings and installation integration.
