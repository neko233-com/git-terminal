# Git-Terminal engineering contracts

- This is an independent Go 1.27.3 + MyGo native-UI application, a sibling of git-next. `go/` is the single source of truth. Rust, Cargo, GPUI Fast and the old vendor crates are removed; there is no Rust rollback tree in this repository.
- Preserve unrelated changes and never edit git-next's client implementation or packaging as part of a terminal-only change.
- On Windows use ConPTY and on POSIX a real PTY, owned by the MyGo terminal plugin. Shell processes, streams, exit, resize and cleanup stay under explicit `SessionManager` ownership; closing a session twice is harmless and leaks no process.
- Default the bundled Windows application to its owned Bash shell when that runtime exists; support native PowerShell and CMD profiles. Always put the verified git-next `bin` directory before inherited PATH.
- `--git` must verify the candidate identifies as `git-next`; a non-git-next executable is refused. No Git GUI application, shortcut or context menu.
- Never log terminal contents, credentials, environment secrets or private paths in public evidence.
- `Taskfile.yml` is the single local entry point. `task verify` must pass gofmt, go vet, `go test -shuffle=on -count=1 ./...`, locale checks, the no-Rust guard, build and CLI smoke. `task test-race` is an explicit gate; without a C compiler it must fail clearly and never be marked passed. Never create or run `.github/workflows`.
- Keep README, `docs/site/` and the Go implementation note accurate, and use Conventional Commits with `Assisted-by: Codex:GPT-6`.
- If product behavior needs to change these constraints, update this file, the Taskfile, the tests and README before changing the implementation.

## Interface languages

- A language is a document: `go/locales/<code>.json`, embedded with `go:embed locales/*.json`. Adding a language never touches Go code, a registry or a test fixture. Root-level legacy locale copies are not the source of truth.
- English is the source language. Interface strings are the keys; a document lists only the messages it overrides, and anything missing falls back to English per message. Never write a second language inline.
- Verify a language once, when its document is added. Do not add per-language cases to desktop or consolidated acceptance suites: those verify behaviour, not wording, and a wrong string is a defect of that one document.
- Every locale change must pass `task locales` (`go run ./cmd/checklocales`). It fails on an unreadable document, a duplicate key, a key the Go source no longer produces, a named placeholder that does not survive translation, a translation that disagrees with the source on a trailing newline, and unsorted keys. `--fix` sorts, `--coverage` reports what each language still leaves to English.
- The device language is resolved from the native preferred-UI-language API before any locale environment variable, because a shell on Windows can export a locale that has nothing to do with the user's choice.
- Interface strings are the only thing a document may translate. Never translate paths, branch names, terminal output, command text or anything the user types.

## Scope

- Implemented: local tabs, ConPTY/ptmx sessions, shell profiles, settings, language documents, managed prompt setup, git-next identity and PATH contract, and bounded session cleanup.
- Not implemented in the Go build: SSH/SFTP remote workspace, host monitor, askpass dialogs, themes and Rust prompt syntax coloring. They must remain explicitly listed as gaps; do not claim them from historical Rust evidence.
