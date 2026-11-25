# Git-Terminal (Go / mygo)

The Go build replaces the Rust + GPUI Fast desktop shell with a native
[mygo](https://github.com/egoist/mygo) window. It uses mygo's Ghostty VT
terminal view and its pure-Go PTY (ConPTY on Windows, ptmx on POSIX).

## Build and test

```sh
cd go
GOTOOLCHAIN=go1.27.3 go test ./...
GOTOOLCHAIN=go1.27.3 go run ./cmd/checklocales [--coverage] [--fix]
GOTOOLCHAIN=go1.27.3 go build -trimpath -ldflags "-s -w" \
  -o ../target/go/git-terminal.exe ./cmd/git-terminal
cp resources/windows-amd64/ghostty-vt.dll ../target/go/ghostty-vt.dll
```

`checklocales` is the Go port of `tools/check_locales.py`: it checks every
`go/locales/*.json` against the English messages in the Go source (structure,
stale keys, named placeholders, trailing whitespace, sorted keys) and reports
per-language coverage. `--fix` sorts keys; `--root PATH` selects another
module directory.

The mygo plugin loads `ghostty-vt.dll` from beside the executable. For a
different copy set `MYGO_GHOSTTY_VT` to the DLL path. The binaries are pure Go
(no cgo); `go test -race` needs a C compiler, which is not required to build.

Language documents are embedded from `go/locales`; the managed-prompt scripts
in `go/shell-integration` are byte-for-byte copies of the repository's
originals. The locale documents started as copies of `../locales`, then the
messages the Go UI no longer produces (SSH, SFTP, monitor and other unmigrated
screens) were removed so the Go checker can verify that every key still has a
source message. The root `locales/` documents are unchanged. At runtime the
scripts are materialized into
`<APPDATA or XDG_CONFIG_HOME>/git-terminal/shell-integration` unless they are
already beside the executable.

## Binary size

Measured on Windows/amd64 with `-trimpath -ldflags "-s -w"`:

| Artifact | Bytes | MiB |
| --- | ---: | ---: |
| Go `git-terminal.exe` | 14,676,992 | 14.00 |
| Go `ghostty-vt.dll` | 2,173,952 | 2.07 |
| Go executable + DLL | 16,850,944 | 16.07 |
| Rust baseline `target/release/git-terminal.exe` | 16,777,216 | 16.00 |

The Go executable by itself is 2,100,224 bytes (2.00 MiB) smaller than the
Rust executable. The Rust baseline is a single file, while the Go build needs
the separate Ghostty VT DLL: adding that DLL makes the shipped pair 73,728
bytes (0.07 MiB) larger than the Rust executable alone. So the honest answer
depends on what is compared: the executable alone shrank, the complete Go
artifact set is marginally larger. Build output paths change the Go binary by
a few kilobytes; the numbers above come from the exact command in this file.

## Interface contracts kept

- A language is a document; English is the source and each missing message
  falls back per message. The device's preferred UI language wins over
  locale environment variables. `language list|get|set` and settings in
  `git-terminal/settings.json` work as before.
- `--git` is verified with `version --json`: a candidate that does not
  identify as `git-next` is rejected. The git-next `bin` directory is placed
  at the front of the shell's `PATH`, before inherited entries.
- Terminal contents, credentials and environment secrets are never logged.
- Each tab owns its terminal; `SessionManager` closes every session once on
  tab close, shell exit, or window close. PTY resize is driven by the mygo
  terminal view following the window size.
- No Git GUI, shortcut or context menu is installed.

## Not migrated from Rust v0.2.0

SSH/SFTP remote workspace, host editor, monitor, askpass dialogs, themes and
syntax highlighting in the terminal view, the profile menu and the Git GUI
acceptance harness. `--ssh` and the `remote` subcommand are intentionally not
implemented. The `--print-setup` renderer is honestly reported as `mygo`;
the git-next tooling now accepts `gpui-fast` or `mygo` for the terminal
renderer; the full desktop/MSI lifecycle still needs a separate pass.
