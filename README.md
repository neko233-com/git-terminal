# Git-Terminal

Rust + GPUI Fast desktop terminal for Windows. This independent Git submodule sits beside git-next. The Windows git-next MSI installs both products and a Git-Terminal Start menu shortcut.

The application renders real ConPTY sessions through GPUI Fast, without opening an external terminal. It provides tabs, ANSI/true color, alternate screens, Unicode/Chinese input, selection, scrollback, bracketed paste, copy/paste, font zoom, resize, exit handling and owned process cleanup. There is no Git GUI interface.

The bundled default is its owned Bash shell, retaining familiar Git Bash commands and existing HOME configuration. PowerShell and CMD profiles are available. Every session prepends the installed git-next runtime to PATH. git-next owns installation and the protected old-Git handoff; this application leaves repositories and credential storage in place.

~~~powershell
git-terminal --cwd D:\Code\project
git-terminal --shell powershell
git-terminal --shell cmd
git-terminal --print-setup --json
git-terminal language set chinese
git-terminal language set english
git-terminal language set auto
git-terminal language get
~~~

The explicit --command argument runs a command in the chosen shell. --git selects an absolute git-next executable; its identity is checked. Without a bundled Bash runtime, standalone installation selects the platform's native shell. An explicitly unavailable profile fails clearly.

Settings live in %APPDATA%\git-terminal\settings.json on Windows. Language follows the system locale until manually selected. Open the dropdown beside + to choose a default shell or switch Chinese/English. Font zoom and preferences persist atomically. The application never writes terminal output, commands, credentials or environment contents into application logs. Shells manage their normal history files.

| Action | Shortcut |
| --- | --- |
| New tab / close tab | Ctrl+T / Ctrl+W (Ctrl+Shift+T/W also supported) |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Copy selection / paste | Ctrl+Shift+C / Ctrl+Shift+V |
| Paste | Shift+Insert or right click |
| Scroll history | Mouse wheel or Shift+PageUp/PageDown |
| Select / word / line | Drag / double click / triple click |
| Bypass application mouse reporting | Hold Shift |
| Font zoom / reset | Ctrl +/- / Ctrl+0 |

Requires Windows 10 1809+ with ConPTY and a Direct3D-capable graphics driver. Windows x64 and ARM64 binaries belong to the matching git-next MSI.

The tab-first interface uses a dark native title bar, a readable 16px monospace terminal and a coordinated ANSI palette. Tabs show the directory and branch, with horizontal scrolling when many are open. New tabs inherit the active directory. Closing another tab preserves the selected session. The footer shows the active directory and shell; additional shell/language choices stay in the dropdown.

~~~powershell
cargo build --locked --release --target x86_64-pc-windows-msvc
cargo build --locked --release --target aarch64-pc-windows-gnullvm
cargo fmt --all --check
cargo clippy --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo test --locked --target x86_64-pc-windows-msvc
~~~

Build with Rust 1.96+, the Windows SDK and Visual Studio C++ tools. Complete implementation before consolidated terminal, desktop and MSI acceptance. Compilation alone is not desktop interaction acceptance. Results are recorded in docs/acceptance.json when available. No GitHub Actions are used. [HTML manual](docs/site/index.html).

GPUI Fast is pinned to 0.1.10. The attributed component under vendor/gpui-terminal derives from zortax/gpui-terminal 0.1.0 and uses Alacritty's terminal engine. [Local changes and preserved licenses](vendor/gpui-terminal/MODIFICATIONS.md).

ARM64 builds use the pinned isolated LLVM MinGW SDK: set CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER to aarch64-w64-mingw32-clang.exe, put its bin directory on PATH and package the separately pinned libunwind.dll with its notices. The git-next tools/prepare_git_terminal.py performs these checks and collects dependency notices.

CMD uses native Windows separators and an owned console helper for explicit command scripts, preserving CMD quoting, environment expansion, ConPTY inheritance and exit status. The helper is bundled with the app.

Interactive Bash includes the pinned [Bash Line Editor](https://github.com/akinomyoga/ble.sh) for command, argument and string highlighting, suggestions and readline editing. Interactive PowerShell loads the system PSReadLine module. CMD retains its native editor. No plugin download is needed at first launch; the BSD-3-Clause notices and exact upstream identity ship with the package.

The default Bash prompt shows the current directory and Git branch, with a short OID for detached HEAD. It searches parent directories and supports a worktree or submodule `.git` file; it does not scan the working tree on each prompt. PowerShell adds the branch to its existing prompt. Tab titles follow the directory/branch. Bash startup reads `/etc/profile` and the first existing user login profile, in the usual order, without editing those files. The owned rc file runs in an interactive non-login Bash; scripts that require the `login_shell` flag can opt out with `--no-shell-integration` to use the original login shell behavior.

`--no-profile` skips user startup files while retaining the owned prompt/highlighter. `--no-shell-integration` selects ordinary shell startup. The `syntax_highlighting` and `git_prompt` booleans in settings.json independently disable these features. Explicit `--command` execution keeps the normal command/exit semantics.
