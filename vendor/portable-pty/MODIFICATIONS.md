# Attributed PTY component

Source: portable-pty 0.9.0, https://github.com/wezterm/wezterm/tree/main/pty.
Original MIT license and copyright are preserved in LICENSE.md.

Git-Terminal changes, 2026:

- Load the operating system ConPTY API from kernel32.dll. Do not search PATH for
  an unrelated application's conpty.dll/OpenConsole.exe pair.
- Create each new screen with documented flags `0`. The GUI has no parent console
  cursor to inherit; cursor inheritance creates an unnecessary startup handshake.
- POSIX PTY behavior and public interfaces are unchanged.

See Microsoft's [ConPTY session lifecycle](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)
and [CreatePseudoConsole](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole).
