# Local product verification

The git-next bundle runs the terminal's local Rust format, clippy, workspace unit
and real PTY shell gates through its Go `terminal-product-acceptance` command.
Those gates use disposable profiles and an explicitly selected owned Git/Bash
distribution. Desktop acceptance remains a separate real Windows window test.

The Windows renderer keeps its rounded-clip test fixture inside the vendored
crate, copied byte-for-byte from locked gpui-fast 0.1.10. Image-render trait
methods follow the dependency's `test-support` feature. This makes clean source
checkouts self-contained without changing production rendering.

SSH configuration accepts both `Host=alias` and `Host = alias`. Equals signs in
values remain data; quoted paths and comments retain their existing behavior.
