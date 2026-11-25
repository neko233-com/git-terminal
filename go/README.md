# Git-Terminal (Go 1.27.3 + MyGo)

`go/` is the single source of truth for the terminal client. The application is
built with Go 1.27.3 and [MyGo](https://github.com/egoist/mygo); there is no
second implementation or Rust tree in this repository. On Windows MyGo's
terminal plugin uses ConPTY, on POSIX a real PTY. The plugin owns every shell
process, stream, resize and exit; `SessionManager` owns the tabs and closes
each session exactly once.

`Taskfile.yml` at the repository root is the single local entry point.

## Build and verify

```sh
cd go
GOTOOLCHAIN=go1.27.3 go test -shuffle=on -count=1 ./...
GOTOOLCHAIN=go1.27.3 go vet ./...
GOTOOLCHAIN=go1.27.3 go run ./cmd/checklocales --coverage
GOTOOLCHAIN=go1.27.3 go build -trimpath -ldflags "-s -w" \
  -o ../target/go/git-terminal.exe ./cmd/git-terminal
cp resources/windows-amd64/ghostty-vt.dll ../target/go/ghostty-vt.dll
cp resources/NOTICE.md ../target/go/NOTICE.ghostty-vt.md
```

From the repository root, `task verify` runs formatting, vet, shuffled uncached
tests with `CGO_ENABLED=0`, locale checks, the no-Rust and no-cgo guards, the
build and headless CLI smoke in one gate. The race detector is not a gate
because it requires cgo, which this product does not use. No GUI,
desktop-interaction or MSI result is claimed by these gates. No GitHub Actions
are used.

`checklocales` verifies every document in `go/locales` against the English
messages in the Go source: structure, stale keys, named placeholders, trailing
whitespace and sorted keys. `--fix` sorts keys and `--coverage` reports what
each language still leaves to English. Adding a language never touches Go code.

The MyGo plugin loads `ghostty-vt.dll` from beside the executable. To use a
different copy, set `MYGO_GHOSTTY_VT` to its path. The binary itself is pure Go
(no cgo). Shell scripts are embedded from `go/shell-integration` and
materialized into `<APPDATA or XDG_CONFIG_HOME>/git-terminal/shell-integration`
unless they already exist beside the executable; writing them again is a no-op.

## Binary size

Measured on Windows/amd64 with `-trimpath -ldflags "-s -w"`:

| Artifact | Bytes | MiB |
| --- | ---: | ---: |
| `git-terminal.exe` | 14,678,016 | 14.00 |
| `ghostty-vt.dll` | 2,173,952 | 2.07 |
| executable + DLL | 16,851,968 | 16.07 |

Build output paths shift the executable by a few kilobytes; the numbers above
come from the exact command in this file.

## Ghostty notice

`windows-amd64/ghostty-vt.dll` is MyGo's Windows x64 build of Ghostty's
`libghostty-vt`. Its upstream source, checksum and license terms are recorded in
[`resources/NOTICE.md`](resources/NOTICE.md). Keep that notice with any package
that ships the DLL; the library is used only by MyGo's terminal plugin and is
never downloaded at runtime.

## Contracts

- A language is a document; English is the source and each missing message
  falls back per message. The device preferred-UI language takes priority over
  locale environment variables. `language list|get|set` and
  `git-terminal/settings.json` are the supported interface.
- `--git` is verified with `version --json`: a candidate that does not identify
  as `git-next` is refused. The verified git-next `bin` directory is placed at
  the front of the shell's `PATH`, before inherited entries.
- `--print-setup --json` reports `renderer: "mygo"`. `--cd-to-home` conflicts
  with `--no-cd`; unknown options fail.
- Terminal contents, credentials and environment secrets are never logged.
- Closing a tab, a shell exit and window teardown each close a session once;
  closing twice is harmless. Explicit `--command` keeps normal exit semantics.
- No Git GUI, shortcut or context menu is installed.

## Not implemented

SSH/SFTP remote workspace, host monitor, askpass dialogs, themes and managed
prompt syntax coloring are outside this client. `--ssh` and the `remote`
subcommand are intentionally absent. CLI and locale checks are not desktop
interaction acceptance; no such result is claimed here.
