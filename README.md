# Git-Terminal

[Git-Terminal 0.2.0 downloads](https://github.com/neko233-com/git-terminal/releases/tag/v0.2.0) include Windows x64/ARM64 portable packages and complete git-next MSI bundles. The Git engine retains the accepted git-next 0.3.0 bytes; this release changes the terminal component to 0.2.0.

For an existing installation, close Git-Terminal and run the matching architecture/scope MSI with `REINSTALL=ALL REINSTALLMODE=vomus` (a Windows Installer small update). New installations omit those properties. Machine packages require administrator elevation; `-user.msi` is the per-user variant. Match the existing scope. This recaches the complete repair/uninstall payload and preserves git-next, shortcuts, Git/SSH configuration and credentials. A standalone ZIP needs an existing git-next runtime for Bash; otherwise select native PowerShell/CMD.

The existing Git product update task remains responsible for Git releases. Install this separate terminal component update explicitly; it does not change the Git engine version or updater comparison. No standalone terminal automatic update is claimed. Linux/macOS Git packages are unchanged.

A native Rust + GPUI Fast terminal and SSH workspace for Windows. This independent Git submodule sits beside git-next. The Windows git-next MSI installs both products and a Git-Terminal Start menu shortcut.

The application renders real ConPTY sessions through GPUI Fast, without opening an external terminal. It provides tabs, ANSI/true color, alternate screens, Unicode/Chinese input, selection, scrollback, bracketed paste, copy/paste, font zoom, resize, exit handling and owned process cleanup. There is no Git GUI interface.

Windows uses one integrated native title/tab bar with an embedded application icon, drag regions and native minimize/maximize/close hit areas, including the system Snap menu. The original terminal/branch icon is embedded in the EXE in nine sizes for Explorer, taskbar, Alt+Tab and installed shortcuts; no icon download is needed. Its SVG source is in `assets/git-terminal.svg`; `python tools/render_icon.py` regenerates the PNG/ICO using Pillow.

Desktop acceptance copies clipboard formats eagerly into memory and restores them without retaining a cross-process OLE object. Clipboard contents are never logged or written to fixture files. Unsupported formats fail before mutation.

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

The managed Bash prompt uses native Rust coloring for commands, options, quoted strings, variables and comments, including wrapped input and Unicode. An unfinished quote is marked in red. Bash retains its native readline editing, history and completion; no external line editor or first-launch plugin download is needed. Coloring changes only the rendered input cells, never shell bytes, command output or alternate-screen applications. Interactive PowerShell loads the system PSReadLine module. CMD retains its native editor.

The default Bash prompt shows the current directory and Git branch, with a short OID for detached HEAD. It searches parent directories and supports a worktree or submodule `.git` file; it does not scan the working tree on each prompt. PowerShell adds the branch to its existing prompt. Tab titles follow the directory/branch. Bash startup reads `/etc/profile` and the first existing user login profile, in the usual order, without editing those files. The owned rc file runs in an interactive non-login Bash; scripts that require the `login_shell` flag can opt out with `--no-shell-integration` to use the original login shell behavior.

Sessions inherit the launcher's environment, including explicit HOME, temporary-directory and proxy overrides. Startup does not replace those values with registry defaults. On Windows the installation initializes the bundled runtime's pinned mount configuration, including the per-user temporary directory. Package acceptance stages the same defaults in a disposable copy.

`--no-profile` skips user startup files while retaining the owned prompt/highlighter. `--no-shell-integration` selects ordinary shell startup. Set `syntax_highlighting` to false to disable native coloring. Set `git_prompt` to false to use your own prompt; native Bash coloring follows the managed prompt. Explicit `--command` execution keeps the normal command/exit semantics.


## SSH workspace, SFTP and monitoring

Open **SSH** in the title bar to manage hosts. Add a name, group and host/address or OpenSSH alias; user, port, identity file and config file are optional. A blank port keeps the SSH config value. Import reads explicit Host aliases from the selected config; OpenSSH itself still resolves Include, ProxyJump, agents, certificates and other connection options. Editing or deleting a saved host affects future connections, not an already open SSH tab. Ctrl+T duplicates the active SSH connection; the shell menu opens a local profile.

Connect opens a real SSH PTY inside a tab. On Windows the system OpenSSH is preferred so its native agent works; the owned runtime provides the fallback. Known-host verification remains enabled according to OpenSSH configuration. Terminal login takes place in the remote tab. Background SFTP/monitor connections use the bundled native authentication dialog when needed. Passwords/passphrases are not written to hosts.json or logs; use an SSH agent to reuse unlocked keys. Each connection and its subprocesses belong to this application's lifetime. Failed SSH tabs retain their output and offer Reconnect.

Select a host to open its SFTP pane. Click directories to navigate, use ↑ to go up, **Upload…** to choose files/directories, or drag files from Explorer onto the file pane. Right-click an entry and choose **Download…** to save a file or directory. The directory refreshes after successful transfers. The queue runs at most two transfers concurrently, shows bytes/state, and supports cancellation. Up to 256 operations can wait. Large directories use a virtual list, and file data uses bounded 64 KiB blocks with 16 requests in flight.

Individual files are staged to unique .part names and installed only when complete. Existing remote files require explicit replacement and server support for atomic POSIX rename. Directory transfers merge per file; they are not a transaction for the entire tree. Symbolic links and Windows reparse points are not followed. Unsafe names, malformed packets and unexpected response IDs fail. A cancelled or disconnected transfer preserves the original destination; cleanup is best effort if the peer/network has already disappeared. Local incomplete files are removed. SFTP requests have a 60-second inactivity timeout; cancellation allows a short cleanup interval before terminating the owned SSH process. Closing the last terminal tab keeps an active transfer queue visible; exiting the app cancels owned work.

**Monitor** opens a read-only Linux panel with OS/timezone, uptime, per-core CPU, memory/cache, network counters/rates, per-device disk I/O and filesystem capacity. It reads /proc and /sys over one SSH channel; it installs no agent and needs no root privileges. First-sample rates show an ellipsis until a second sample establishes a delta. The interval is saved per host (1/2/5/10 seconds in the UI), and Pause/Resume controls sampling. Closing the panel or changing hosts stops the old monitor. Non-Linux hosts retain SSH/SFTP; their monitor reports the missing adapter clearly.

```powershell
git-terminal remote add production user@example.invalid --group Production
git-terminal remote import C:\Users\you\.ssh\config
git-terminal remote hosts
git-terminal --ssh production
git-terminal remote list production /srv
git-terminal remote put production .\build /srv/build
git-terminal remote get production /srv/logs .\logs
git-terminal remote monitor production --count 2
```

Saved connections are in `%APPDATA%\git-terminal\hosts.json` (or the XDG config directory on POSIX). Existing corrupt/unreadable host configuration is not silently replaced. Monitoring is Linux-only; desktop runtime acceptance targets Windows x64, and Windows ARM64 is separately cross-built and packaged. Mac runtime acceptance is not claimed.

The managed local prompts read HEAD directly for normal branches, worktrees and detached HEAD. Root paths are joined without a double leading slash, preventing Windows/MSYS from turning /HEAD into a UNC network lookup. The Windows host uses system ConPTY rather than a conpty.dll found on PATH. Supplementary Unicode input uses the negotiated Win32 input protocol, preserving emoji and supplementary CJK characters on the system console host. Pipe writes and resize calls run off the UI thread; hidden tabs answer terminal protocol requests without requiring a repaint. Output processing yields to keyboard/window work and coalesces hidden-tab titles/bells. It never records terminal contents.
