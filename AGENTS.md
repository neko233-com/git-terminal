# Git-Terminal engineering contracts

- This independent Go 1.27 + MyGo (GPUI-style native UI) application is a sibling of git-next in the GitSSS repository. The Rust + GPUI Fast tree remains a reference and rollback path until the Go build passes the same local acceptance; do not delete capabilities to claim migration.
- Preserve unrelated changes and never edit git-next/src/bin/git-terminal.rs, the historical launcher owned by another task.
- Use ConPTY on Windows and a real PTY on POSIX. Keep shell processes, streams, exit, resize and cleanup under explicit session ownership.
- Default the bundled Windows application to its owned Bash shell; support native PowerShell and cmd profiles. Always put the installed git-next bin before inherited PATH.
- No Git GUI application, shortcut or context menu. Preserve shell and Git command compatibility.
- Never log terminal contents, credentials, environment secrets or private paths in public evidence.
- Implement the requested scope before consolidated acceptance; run gofmt, go vet and targeted `go test ./...` locally. Rust sources keep their own checks only while retained as reference. Never create or run .github/workflows.
- Keep README and docs/site accurate, and use Conventional Commits with Assisted-by: Codex:GPT-6.

## Interface languages

- A language is a document: `locales/<code>.json` in the Go module, embedded with `go:embed locales/*.json`, so adding a language never touches Go code, a registry or a test fixture.
- English is the source language. Interface strings are the keys; a document lists only the messages it overrides, and anything missing falls back to English per message. Never write a second language inline again: no `if chinese { "中文" } else { "English" }`.
- Verify a language once, when its document is added. Do not add per-language cases to the desktop or consolidated acceptance suites: those verify behaviour, not wording, and a wrong string is a defect of that one document.
- Every locale change must pass the Go locale checker in the Go module. It fails on an unreadable document, a duplicate key, a key the source no longer produces, a named placeholder that does not survive translation, a translation that disagrees with the source on a trailing newline, and unsorted keys. `--fix` sorts, `--coverage` reports what each language still leaves to English.
- The device language is resolved from the native preferred-UI-language API before any locale environment variable, because a shell on Windows can export a locale that has nothing to do with the user's choice.
- Interface strings are the only thing a document may translate. Never translate paths, branch names, terminal output, command text or anything the user types.
