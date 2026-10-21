// Copyright 2026 git-next contributors. MIT license.
use crate::session::{Session, Setup};
use alacritty_terminal::{
    event::{Event, WindowSize},
    grid::Dimensions,
    index::{Column, Point},
    vte::ansi::Rgb,
};
use gpui_terminal::{
    event::{GpuiEventProxy, TerminalEvent},
    terminal::TerminalState,
};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::Command,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct OwnedFixture(PathBuf);
impl Drop for OwnedFixture {
    fn drop(&mut self) {
        let Ok(path) = self.0.canonicalize() else {
            return;
        };
        let Ok(temporary) = std::env::temp_dir().canonicalize() else {
            return;
        };
        if !path.starts_with(temporary) || self.0.is_symlink() {
            return;
        }
        for _ in 0..20 {
            if std::fs::remove_dir_all(&path).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[test]
#[ignore = "requires the owned full Git/Bash distribution; run by consolidated acceptance"]
fn shell_features_real_pty() {
    let git = PathBuf::from(
        std::env::var_os("GIT_TERMINAL_TEST_GIT").expect("owned Git fixture is required"),
    );
    let root = std::env::temp_dir().join(format!(
        "git-terminal-shell-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let nested = root.join("repo/中文/sub");
    std::fs::create_dir_all(&nested).unwrap();
    let _fixture = OwnedFixture(root.clone());
    let run_git = |args: &[&str]| {
        let output = Command::new(&git)
            .args(args)
            .current_dir(root.join("repo"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "generated Git fixture operation failed"
        );
        output.stdout
    };
    run_git(&["init", "--initial-branch=feature/first"]);
    run_git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--allow-empty",
        "-m",
        "fixture",
    ]);
    let worktree = root.join("linked");
    run_git(&[
        "worktree",
        "add",
        "-b",
        "feature/linked",
        worktree.to_str().unwrap(),
    ]);
    let detached = String::from_utf8(run_git(&["rev-parse", "--short", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    let mut setup = Setup::discover(&nested, "bash", Some(&git)).unwrap();
    setup.no_profile = true;
    let (session, mut writer, mut reader) = Session::spawn(&setup, None).unwrap();
    let (bytes_tx, bytes_rx) = mpsc::sync_channel(32);
    let read_task = std::thread::spawn(move || {
        let mut buffer = [0; 16384];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || bytes_tx.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let (events_tx, events_rx) = mpsc::channel();
    let mut terminal = TerminalState::new(110, 30, GpuiEventProxy::new(events_tx));
    let mut received = Vec::new();
    let pump = |terminal: &mut TerminalState,
                writer: &mut Box<dyn Write + Send>,
                received: &mut Vec<u8>,
                needle: &str| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(bytes) = bytes_rx.recv_timeout(Duration::from_millis(30)) {
                terminal.process_bytes(&bytes);
                received.extend_from_slice(&bytes);
            }
            while let Ok(event) = events_rx.try_recv() {
                let TerminalEvent::Response(event) = event else {
                    continue;
                };
                let reply = match event {
                    Event::PtyWrite(text) => text,
                    Event::ColorRequest(_, format) => format(Rgb {
                        r: 18,
                        g: 21,
                        b: 27,
                    }),
                    Event::TextAreaSizeRequest(format) => format(WindowSize {
                        num_lines: 30,
                        num_cols: 110,
                        cell_width: 8,
                        cell_height: 16,
                    }),
                    _ => String::new(),
                };
                writer.write_all(reply.as_bytes()).unwrap();
                writer.flush().unwrap();
            }
            if received
                .windows(needle.len())
                .any(|b| b == needle.as_bytes())
            {
                return;
            }
        }
        panic!("generated interactive shell fixture did not become ready");
    };
    pump(&mut terminal, &mut writer, &mut received, "❯");
    writer.write_all(b"builtin printf '\\107\\124_READY:%s:%s\\n' \"$BLE_VERSION\" \"$__git_terminal_branch\"\r").unwrap();
    writer.flush().unwrap();
    received.clear();
    pump(&mut terminal, &mut writer, &mut received, "GT_READY:0.4");
    assert!(
        received
            .windows(b"[feature/first]".len())
            .any(|b| b == b"[feature/first]"),
        "parent repository branch was not found"
    );
    received.clear();
    writer.write_all(b"printf 'highlight-token'").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut colored = false;
    while Instant::now() < deadline && !colored {
        if let Ok(bytes) = bytes_rx.recv_timeout(Duration::from_millis(30)) {
            terminal.process_bytes(&bytes);
        }
        colored = terminal.with_term(|term| {
            let line = term.grid().cursor.point.line;
            let cells: Vec<_> = (0..term.columns())
                .map(|col| &term.grid()[Point::new(line, Column(col))])
                .collect();
            let chars: Vec<char> = cells.iter().map(|cell| cell.c).collect();
            let find = |needle: &str| {
                chars
                    .windows(needle.chars().count())
                    .position(|v| v.iter().copied().eq(needle.chars()))
            };
            if let (Some(command), Some(string)) = (find("printf"), find("'highlight-token'")) {
                cells[command].fg != cells[string].fg
            } else {
                false
            }
        });
    }
    assert!(
        colored,
        "interactive command and string did not receive distinct syntax colors"
    );
    writer.write_all(b"\x15").unwrap();
    let quote = |path: &std::path::Path| {
        format!(
            "'{}'",
            path.to_string_lossy()
                .replace('\\', "/")
                .replace('\'', "'\\''")
        )
    };
    let source = format!(
        "builtin cd {}; __git_terminal_update_prompt; builtin printf '\\107\\124_LINKED:%s\\n' \"$__git_terminal_branch\"\r",
        quote(&worktree)
    );
    received.clear();
    writer.write_all(source.as_bytes()).unwrap();
    writer.flush().unwrap();
    pump(
        &mut terminal,
        &mut writer,
        &mut received,
        "GT_LINKED:[feature/linked]",
    );
    let source = "\"$GIT_NEXT_EXECUTABLE\" checkout --quiet --detach; __git_terminal_update_prompt; builtin printf '\\107\\124_DETACHED:%s\\n' \"$__git_terminal_branch\"\r";
    received.clear();
    writer.write_all(source.as_bytes()).unwrap();
    writer.flush().unwrap();
    pump(
        &mut terminal,
        &mut writer,
        &mut received,
        &format!("GT_DETACHED:[{detached}]"),
    );
    let source = format!(
        "builtin cd {}; __git_terminal_update_prompt; builtin printf '\\107\\124_OUTSIDE:%s:END\\n' \"$__git_terminal_branch\"; exit\r",
        quote(&root)
    );
    received.clear();
    writer.write_all(source.as_bytes()).unwrap();
    writer.flush().unwrap();
    pump(&mut terminal, &mut writer, &mut received, "GT_OUTSIDE::END");
    drop(writer);
    drop(session);
    drop(bytes_rx);
    read_task.join().unwrap();
    let mut powershell = Setup::discover(&nested, "powershell", Some(&git)).unwrap();
    powershell.no_profile = true;
    let (session, mut writer, mut reader) = Session::spawn(&powershell, None).unwrap();
    let (tx, rx) = mpsc::sync_channel(32);
    let reader_task = std::thread::spawn(move || {
        let mut buffer = [0; 16384];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || tx.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    writer.write_all(b"[Console]::WriteLine(('GT_PS_'+'READY:{0}' -f [bool](Get-Module PSReadLine))); exit 0\r").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut bytes = Vec::new();
    while Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(30)) {
            bytes.extend_from_slice(&chunk);
        }
        if bytes
            .windows(b"GT_PS_READY:True".len())
            .any(|v| v == b"GT_PS_READY:True")
        {
            break;
        }
    }
    assert!(
        bytes
            .windows(b"GT_PS_READY:True".len())
            .any(|v| v == b"GT_PS_READY:True"),
        "PowerShell did not load the native syntax highlighter"
    );
    assert!(
        bytes
            .windows(b"[feature/first]".len())
            .any(|v| v == b"[feature/first]"),
        "PowerShell did not find the ancestor Git branch"
    );
    drop(writer);
    drop(session);
    drop(rx);
    reader_task.join().unwrap();
    let absolute = root.canonicalize().unwrap();
    assert!(absolute.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    std::fs::remove_dir_all(absolute).unwrap();
}
