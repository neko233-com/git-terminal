# Git-Terminal

The current implementation is Go 1.27.3 with [MyGo](https://github.com/egoist/mygo)'s GPUI-style native UI and terminal plugin. `go/` is the single source of truth; Rust, Cargo, GPUI Fast and their vendored crates are removed from this repository. [Go implementation note](go/README.md) · [HTML manual](docs/site/index.html).

## Build and verify

`Taskfile.yml` is the single local entry point. It pins Go 1.27.3 and keeps the client CGO-free.

```sh
task verify        # gofmt, vet, shuffled tests, locales, no-Rust guard, build, CLI smoke
task test:race     # explicit gate; fails clearly without a C compiler
task build         # target/go/git-terminal(.exe) plus ghostty-vt.dll and NOTICE
```

Manual equivalents:

```sh
cd go
GOTOOLCHAIN=go1.27.3 go test -shuffle=on -count=1 ./...
GOTOOLCHAIN=go1.27.3 go vet ./...
GOTOOLCHAIN=go1.27.3 go run ./cmd/checklocales --coverage
GOTOOLCHAIN=go1.27.3 go build -trimpath -ldflags "-s -w" -o ../target/go/git-terminal.exe ./cmd/git-terminal
cp resources/windows-amd64/ghostty-vt.dll ../target/go/ghostty-vt.dll
```

The Ghostty VT native library is required at runtime beside the executable, or set `MYGO_GHOSTTY_VT` to its path. Its source and checksum are recorded in `go/resources/NOTICE.md`. Windows ConPTY requires Windows 10 1809 or later; POSIX builds use ptmx through the MyGo plugin. No Rust toolchain, Windows SDK for Rust, or GPUI Fast build is needed.

## Behavior

Local tabs run real PTY sessions through MyGo's terminal view. The application provides ANSI/true color, Unicode and Chinese input, selection, scrollback, bracketed paste, copy/paste, resize, exit handling and owned process cleanup. There is no Git GUI interface.

The default shell is the owned Bash runtime when installed; PowerShell and CMD profiles are available. A session adds `runtime/<prefix>/bin` and `runtime/usr/bin` to PATH only when those directories exist, and only then sets `MSYSTEM`. `--git` verifies the candidate identifies as `git-next`; the verified git-next `bin` directory is placed before inherited PATH. git-next owns installation and the protected old-Git handoff; this application leaves repositories and credential storage in place.

```powershell
git-terminal --cwd D:\Code\project
git-terminal --shell powershell
git-terminal --shell cmd
git-terminal --command "git status --short"
git-terminal --print-setup --json
git-terminal language list
git-terminal language set zh-Hans
git-terminal language set en
git-terminal language set auto
git-terminal language get
```

Settings live in `%APPDATA%\git-terminal\settings.json` on Windows (or the XDG config directory on POSIX). Interface language follows the device preference until a language is selected; legacy names `chinese`, `english` and `auto` keep working. A language is a JSON document in `go/locales/`; adding one needs no Go code change. The application never writes terminal output, commands, credentials or environment contents into application logs. Shells manage their own history files.

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

`--no-profile` skips user startup files; `--no-shell-integration` selects ordinary shell startup. The managed Bash prompt shows directory and branch, including worktrees and detached HEAD, without scanning the working tree. Explicit `--command` execution keeps normal exit semantics.

## Not implemented in the Go build

SSH hosts, SFTP, host monitor, askpass dialogs, themes and the former Rust prompt syntax coloring are intentionally absent. They are not claimed by Go tests or desktop evidence. Historical Rust 0.2.0 downloads remain available for users who still need those features; see the [0.2.0 release](https://github.com/neko233-com/git-terminal/releases/tag/v0.2.0).

## Installation

The git-next Windows MSI installs Git-Terminal beside the Git engine and creates the Git-Terminal Start menu entry. For an existing installation, close Git-Terminal and run the matching architecture/scope MSI with `REINSTALL=ALL REINSTALLMODE=vamus`. New installations omit those properties. Machine packages require administrator elevation; `-user.msi` is the per-user variant. Match the existing scope. Git/SSH configuration and credentials stay in place. `tools/install_bundle.ps1 -Package <MSI path>` preserves the registered MSI source filename when replacing same-version companion files. No standalone terminal automatic update is claimed.

Compilation and CLI smoke are not desktop interaction acceptance. No GitHub Actions are used; all checks and packaging run locally.
