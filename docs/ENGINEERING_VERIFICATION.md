# Local product verification

The Go 1.27.3 + MyGo client is the only terminal implementation in this
repository. `Taskfile.yml` is the single entry point; `task verify` is the
mandatory local gate and `task test-race` is an explicit, separate gate that
fails clearly when no C compiler is available.

`task verify` runs, in order:

1. `gofmt -l` — no Go file may be unformatted.
2. `go vet ./...` — static analysis of the whole module.
3. `go test -shuffle=on -count=1 ./...` — all tests, uncached and shuffled.
4. `go run ./cmd/checklocales` — every `go/locales/*.json` document is checked
   against the English messages in the Go source (structure, stale keys, named
   placeholders, trailing whitespace and sorted keys).
5. The no-Rust guard — no `*.rs`, `Cargo.toml`, `Cargo.lock`, `build.rs` or
   `rust-toolchain.toml` anywhere in the repository, and no `cargo`/`rustc`
   invocation in `go/` or `tools/`. The Go guard test also checks that external
   git-next evidence allowlists no longer name Rust paths of this repository.
6. `go build -trimpath -ldflags "-s -w"` plus the Ghostty VT library beside the
   executable.
7. CLI smoke: `--version`, `language list` and `--print-setup --json`, none of
   which needs a desktop session.

The Go module has no cgo dependency; `-race` is not part of `task verify`
because it needs a C compiler. All checks run locally. No GitHub Actions are
used and no GUI, MSI or desktop-interaction result is claimed by this gate.

The native library shipped beside the executable is MyGo's Windows x64 build
of Ghostty's `libghostty-vt`; its upstream source, checksum and license note are
in `go/resources/NOTICE.md`. Terminal contents, credentials and environment
secrets are never written to public evidence.
