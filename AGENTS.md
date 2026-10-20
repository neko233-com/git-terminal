# Git-Terminal engineering contracts

- This independent Rust + GPUI Fast application is a sibling of git-next in the GitSSS repository.
- Preserve unrelated changes and never edit git-next/src/bin/git-terminal.rs, the historical launcher owned by another task.
- Use ConPTY on Windows and a real PTY on POSIX. Keep shell processes, streams, exit, resize and cleanup under explicit session ownership.
- Default the bundled Windows application to its owned Bash shell; support native PowerShell and cmd profiles. Always put the installed git-next bin before inherited PATH.
- No Git GUI application, shortcut or context menu. Preserve shell and Git command compatibility.
- Never log terminal contents, credentials, environment secrets or private paths in public evidence.
- Implement the requested scope before consolidated acceptance; run cargo fmt, cargo clippy and targeted tests locally. Never create or run .github/workflows.
- Keep README and docs/site accurate, and use Conventional Commits with Assisted-by: Codex:GPT-6.
